//! `backlot-cat` catalog builder and change-gated CAS publisher (B1a).
//!
//! Probes installed runners once at startup (refreshing only when
//! `config.toml` changes on disk) and republishes `backlot-cat` whenever
//! `runners`, `hollywood_cfg`, `default_cast`, `archived`, `server_id`,
//! or `home` changes.

use std::path::Path;
use std::time::SystemTime;

use backlot_model::event::StudioMode;
use backlot_model::req_wire::{
    CAT_DOC_NAME, CAT_SCHEMA, CatalogDoc, DefaultCastCatalog, HollywoodCfgCatalog,
    RunnerCatalogEntry,
};
use hc_proto::client::{CallError, Client};
use hc_proto::doc::{DocGetParams, DocSetParams};
use hc_proto::jsonrpc::error_kind;

use crate::cli::{Env, Fail};
use crate::commands::CastFile;
use crate::config;
use crate::daemon::Stage;
use crate::runners::detect_report;
use crate::store;
use crate::studio_list;
use crate::wizard_cast;

const MAX_CAS_ATTEMPTS: usize = 3;

/// Cached catalog state owned by the stage loop.
pub(crate) struct Publisher {
    server_id: String,
    runners: Vec<RunnerCatalogEntry>,
    config_mtime: Option<SystemTime>,
    last_doc: Option<CatalogDoc>,
    version: Option<u64>,
}

impl Publisher {
    pub(crate) fn new(server_id: String, env: &Env) -> Self {
        let runners = detect_report::rows(env);
        let config_mtime = config_file_mtime(env);
        Publisher {
            server_id,
            runners,
            config_mtime,
            last_doc: None,
            version: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_runners(server_id: String, runners: Vec<RunnerCatalogEntry>) -> Self {
        Publisher {
            server_id,
            runners,
            config_mtime: None,
            last_doc: None,
            version: None,
        }
    }

    /// Re-build the current `CatalogDoc` (re-probing runners only if
    /// `config.toml` changed on disk since the last check).
    pub(crate) fn build_doc(&mut self, stage: &Stage, now_ms: u64) -> CatalogDoc {
        let env = Env {
            vars: stage.env_vars.clone(),
        };
        let mtime = config_file_mtime(&env);
        if mtime != self.config_mtime && !self.runners.is_empty() && mtime.is_some() {
            self.runners = detect_report::rows(&env);
            self.config_mtime = mtime;
        } else if self.config_mtime.is_none() && mtime.is_some() {
            self.config_mtime = mtime;
        }
        build_catalog(&self.server_id, &stage.base, &env, &self.runners, now_ms)
    }

    /// Publish `backlot-cat` on start and whenever any catalog field changed.
    pub(crate) async fn publish_if_changed(
        &mut self,
        client: &Client,
        stage: &Stage,
        now_ms: u64,
    ) -> Result<bool, Fail> {
        let doc = self.build_doc(stage, now_ms);
        if self
            .last_doc
            .as_ref()
            .is_some_and(|prev| prev.content_eq(&doc))
        {
            return Ok(false);
        }
        let value = serde_json::to_value(&doc)
            .map_err(|e| Fail::refused(format!("catalog does not serialize: {e}")))?;

        let mut ver = match self.version {
            Some(v) => v,
            None => {
                client
                    .doc_get(DocGetParams {
                        name: CAT_DOC_NAME.into(),
                        on: None,
                    })
                    .await
                    .map_err(Fail::call)?
                    .version
            }
        };

        for attempt in 0..MAX_CAS_ATTEMPTS {
            match client
                .doc_set(DocSetParams {
                    name: CAT_DOC_NAME.into(),
                    on: None,
                    value: value.clone(),
                    version: ver,
                })
                .await
            {
                Ok(res) => {
                    self.version = Some(res.version);
                    self.last_doc = Some(doc);
                    return Ok(true);
                }
                Err(CallError::Rpc(err))
                    if err.is_kind(error_kind::CONFLICT) && attempt + 1 < MAX_CAS_ATTEMPTS =>
                {
                    ver = client
                        .doc_get(DocGetParams {
                            name: CAT_DOC_NAME.into(),
                            on: None,
                        })
                        .await
                        .map_err(Fail::call)?
                        .version;
                }
                Err(CallError::Rpc(err)) => {
                    self.version = None;
                    crate::stagelog::stage_log!("backlot-cat write deferred: {err}");
                    return Ok(false);
                }
                Err(e) => return Err(Fail::call(e)),
            }
        }
        Ok(false)
    }
}

fn config_file_mtime(env: &Env) -> Option<SystemTime> {
    let path = config::config_path(&env.vars);
    std::fs::metadata(path).ok()?.modified().ok()
}

/// Construct a [`CatalogDoc`] from resolved runners, config, studios, and archive.
pub(crate) fn build_catalog(
    server_id: &str,
    base: &Path,
    env: &Env,
    runners: &[RunnerCatalogEntry],
    now_ms: u64,
) -> CatalogDoc {
    let home = env.get("HOME").unwrap_or("").to_string();
    let cfg_opt = config::hollywood(&env.vars).ok().flatten();
    let hollywood_cfg = match &cfg_opt {
        Some(c) => HollywoodCfgCatalog {
            configured: true,
            runner: Some(c.runner.clone()),
            model: Some(c.model.clone()),
            effort: c.effort.clone(),
            default_cast: c.default_cast.clone(),
        },
        None => HollywoodCfgCatalog::default(),
    };
    let default_cast =
        resolve_default_cast(base, &home, hollywood_cfg.default_cast.as_deref(), runners);
    let archived = studio_list::archived_for_base(base);

    CatalogDoc {
        schema: CAT_SCHEMA.into(),
        server_id: server_id.into(),
        ts_ms: now_ms,
        runners: runners.to_vec(),
        hollywood_cfg,
        default_cast,
        archived,
        home,
    }
}

fn resolve_default_cast(
    base: &Path,
    home: &str,
    configured_path: Option<&str>,
    runners: &[RunnerCatalogEntry],
) -> Option<DefaultCastCatalog> {
    if let Some(raw_path) = configured_path
        && let Some(cast) = read_cast_from_path(raw_path, home)
    {
        return Some(cast_from_file(cast));
    }
    if let Some(cast) = latest_studio_cast(base) {
        return Some(cast);
    }
    if let Ok(text) = wizard_cast::draft_text_for(
        |runner| {
            runners
                .iter()
                .any(|r| r.runner == runner && r.status == "found")
        },
        StudioMode::Standard,
    ) && let Ok(cast) = toml::from_str::<CastFile>(&text)
    {
        return Some(cast_from_file(cast));
    }
    if runners
        .iter()
        .any(|r| r.runner == "fake" && r.status == "found")
    {
        let id = backlot_model::event::ActorIdentity {
            runner: "fake".into(),
            runner_version: None,
            model: "model-f".into(),
            effort: Some("medium".into()),
            permission_profile: "standard".into(),
        };
        let cand = |name: &str| backlot_model::event::Candidate {
            name: name.into(),
            runner: "fake".into(),
            runner_version: None,
            model: "model-f".into(),
            effort: Some("medium".into()),
            permission_profile: "standard".into(),
        };
        return Some(DefaultCastCatalog {
            mode: Some(StudioMode::Standard),
            producer: id.clone(),
            director: id.clone(),
            writer: id,
            candidates: vec![cand("actor-a"), cand("actor-b")],
            limits: Default::default(),
            rework_cap: None,
            stall_limit_s: None,
            nudge_after_s: None,
            escalate_after_s: None,
            task_budget_s: None,
        });
    }
    None
}

fn read_cast_from_path(raw_path: &str, home: &str) -> Option<CastFile> {
    let expanded = if let Some(rest) = raw_path.strip_prefix("~/")
        && !home.is_empty()
    {
        Path::new(home).join(rest)
    } else {
        Path::new(raw_path).to_path_buf()
    };
    let text = std::fs::read_to_string(expanded).ok()?;
    toml::from_str::<CastFile>(&text).ok()
}

fn latest_studio_cast(base: &Path) -> Option<DefaultCastCatalog> {
    let studios = store::studios(base).ok()?;
    let mut best: Option<(u64, String, DefaultCastCatalog)> = None;
    for name in studios {
        let dir = store::studio_dir(base, &name);
        let Ok((events, _)) = store::load_events(&dir) else {
            continue;
        };
        let state = backlot_model::replay(&events);
        let Some(cfg) = state.studio else {
            continue;
        };
        let ts = events.last().map(|e| e.ts_ms).unwrap_or(0);
        let candidate = DefaultCastCatalog {
            mode: Some(cfg.mode),
            producer: cfg.roles.producer,
            director: cfg.roles.director,
            writer: cfg.roles.writer,
            candidates: cfg.candidates,
            limits: cfg.limits,
            rework_cap: Some(cfg.rework_cap),
            stall_limit_s: Some(cfg.stall_limit_s),
            nudge_after_s: Some(cfg.nudge_after_s),
            escalate_after_s: Some(cfg.escalate_after_s),
            task_budget_s: cfg.task_budget_s,
        };
        let replace = match &best {
            None => true,
            Some((best_ts, best_name, _)) => ts > *best_ts || (ts == *best_ts && name > *best_name),
        };
        if replace {
            best = Some((ts, name, candidate));
        }
    }
    best.map(|(_, _, cast)| cast)
}

fn cast_from_file(cast: CastFile) -> DefaultCastCatalog {
    DefaultCastCatalog {
        mode: cast.mode,
        producer: cast.producer,
        director: cast.director,
        writer: cast.writer,
        candidates: cast.candidates,
        limits: cast.limits,
        rework_cap: cast.rework_cap,
        stall_limit_s: cast.stall_limit_s,
        nudge_after_s: cast.nudge_after_s,
        escalate_after_s: cast.escalate_after_s,
        task_budget_s: cast.task_budget_s,
    }
}
