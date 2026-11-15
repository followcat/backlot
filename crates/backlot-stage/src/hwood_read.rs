//! The cross-studio reads Hollywood and the user share: `card list`,
//! `status --all` and `records --all`/`--studio` (R24).

use std::collections::BTreeMap;
use std::path::Path;

use crate::args::{Cmd, Common};
use crate::cli::{Env, Fail};
use crate::config;
use crate::hrender;
use crate::hwood::{base_of, store_fail};
use crate::render;
use crate::store::{self, StoreError};

/// The read side of `cli::run`'s dispatch: the commands that never
/// append a studio event — `status`, `records` and `studio new
/// --dry-run` (D19, R24). `None` falls through to the append path.
pub fn read(common: &Common, cmd: &Cmd, env: &Env, base: &Path) -> Option<Result<String, Fail>> {
    match cmd {
        Cmd::Status { json, all: true } => Some(status_all(env, base, *json)),
        Cmd::Status { json, .. } => Some(status(common, env, base, *json)),
        Cmd::Records { json, all } => Some(records(common, env, base, *json, *all)),
        Cmd::StudioNew {
            dry_run: true,
            title,
            repo,
            cast_file,
        } => Some(dry_run(common, env, base, title, repo, cast_file)),
        _ => None,
    }
}

/// `status [--studio <name>]` for one studio.
fn status(common: &Common, env: &Env, base: &Path, json: bool) -> Result<String, Fail> {
    let name = crate::cli::studio_name(common, env)?;
    let dir = store::studio_dir(base, &name);
    if !dir.join("events.jsonl").exists() {
        return Err(Fail::refused(format!("no studio '{name}'")));
    }
    let state = store::load_state(&dir).map_err(store_fail)?;
    Ok(if json {
        serde_json::to_string_pretty(&state).map_err(|e| Fail::refused(e.to_string()))?
    } else {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        render::status(
            &name,
            &state,
            &crate::signals::failed_roles(&dir, &state),
            now_ms,
        )
    })
}

/// `records [--studio <name> | --all]`: `--studio` scopes to one;
/// plain and `--all` keep the every-studio read records has always
/// had.
fn records(common: &Common, env: &Env, base: &Path, json: bool, all: bool) -> Result<String, Fail> {
    let _ = env;
    let names = match (all, &common.studio) {
        (false, Some(name)) => {
            let name = crate::cli::check_studio_name(name)?;
            if !store::studio_dir(base, &name)
                .join("events.jsonl")
                .is_file()
            {
                return Err(Fail::refused(format!("no studio '{name}'")));
            }
            vec![name]
        }
        _ => store::studios(base).map_err(|e| Fail::refused(e.to_string()))?,
    };
    let mut logs = Vec::new();
    for name in names {
        let (events, _) = store::load_events(&store::studio_dir(base, &name)).map_err(|e| {
            let detail = match e {
                StoreError::Io(e) => e.to_string(),
                StoreError::Corrupt(c) => c.to_string(),
                _ => "unexpected error".to_string(),
            };
            Fail::refused(format!("studio '{name}': {detail}"))
        })?;
        logs.push(backlot_model::StudioLog {
            studio: name,
            events,
        });
    }
    let recs = backlot_model::records(&logs);
    Ok(if json {
        serde_json::to_string_pretty(&recs).map_err(|e| Fail::refused(e.to_string()))?
    } else {
        render::records(&recs)
    })
}

/// `studio new --dry-run`: every `studio new` check plus the draft's
/// own (D19) — nothing recorded, nothing created.
fn dry_run(
    common: &Common,
    env: &Env,
    base: &Path,
    title: &str,
    repo: &str,
    cast_file: &Path,
) -> Result<String, Fail> {
    let name = crate::cli::studio_name(common, env)?;
    crate::draft::check(base, &name, repo, cast_file, env)?;
    Ok(format!(
        "dry run: studio '{name}' ('{title}', repo {repo}) is valid — nothing recorded"
    ))
}

/// `card list` — every card Hollywood has proposed, open first.
pub fn card_list(common: &Common, env: &Env) -> Result<String, Fail> {
    let base = base_of(common, env)?;
    let hdir = store::hollywood_dir(&base);
    let hstate = store::load_hstate(&hdir).map_err(store_fail)?;
    let cfg = config::hollywood(&env.vars);
    Ok(hrender::hollywood_block(&hstate, &cfg))
}

/// `status --all`: every studio, one block each, then Hollywood's
/// cards and headline — the shape the expanded layer will draw (D16).
pub fn status_all(env: &Env, base: &Path, json: bool) -> Result<String, Fail> {
    let names = store::studios(base).map_err(|e| Fail::refused(e.to_string()))?;
    let mut blocks = Vec::new();
    let mut states = BTreeMap::new();
    for name in &names {
        let dir = store::studio_dir(base, name);
        let state = store::load_state(&dir).map_err(store_fail)?;
        if json {
            states.insert(name.clone(), state);
        } else {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            blocks.push(render::status(
                name,
                &state,
                &crate::signals::failed_roles(&dir, &state),
                now_ms,
            ));
        }
    }
    let hstate = store::load_hstate(&store::hollywood_dir(base)).map_err(store_fail)?;
    let cfg = config::hollywood(&env.vars);
    if json {
        let j = serde_json::json!({
            "studios": states,
            "hollywood": hrender::hollywood_json(&hstate, &cfg),
        });
        serde_json::to_string_pretty(&j).map_err(|e| Fail::refused(e.to_string()))
    } else {
        blocks.push(hrender::hollywood_block(&hstate, &cfg));
        Ok(blocks.join("\n"))
    }
}
