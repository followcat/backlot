//! The Stage side of Hollywood's lifecycle: the once-a-tick pass
//! (`hollywood`), the reconcile that carries `hplan`'s moves out
//! (`hreconcile`), the `terminal.run` (`hrun`) and the folder
//! preparation. The moves themselves are `hstage.rs`'s pure
//! decision; the watch half — session discovery and the work
//! sample — is `hwatch.rs`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use backlot_model::event::{ActorIdentity, RESTARTED};
use backlot_model::hollywood::{HOLLYWOOD, HOLLYWOOD_TERMINAL, RoleMotion};
use hc_proto::client::CallError;
use hc_proto::terminal::RunParams;

use crate::act::fatal;
use crate::cli::Fail;
use crate::config::{self, HollywoodCfg};
use crate::daemon::Stage;
#[cfg(test)]
pub(crate) use crate::hstage::hprepare_launch;
use crate::hstage::{self, HMove, HRun, hprepare_launch_recast, refresh_agents_md};
use crate::reconcile::LiveTerminal;
use crate::runners::{self, StartDecision};
use crate::store;

impl Stage {
    /// `eprintln!` each distinct Hollywood line once — the
    /// `hlogged` set is `&self`-safe so every sub-pass can use it.
    pub(crate) fn hlog(&self, msg: &str) {
        if self.hlogged.borrow_mut().insert(msg.to_string()) {
            crate::stagelog::stage_log!("hollywood: {msg}");
        }
    }

    /// Reload Hollywood's reduced state when its ledger grew —
    /// the same len+mtime freshness mark the studios get.
    pub(crate) fn hrefresh(&self, hdir: &Path) {
        let meta = std::fs::metadata(hdir.join("events.jsonl")).ok();
        let len = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let mtime = meta
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        if (len, mtime) == *self.hseen.borrow() {
            return;
        }
        if let Ok(state) = store::load_hstate(hdir) {
            *self.hstate.borrow_mut() = state;
            *self.hseen.borrow_mut() = (len, mtime);
        }
    }

    /// The once-a-tick Hollywood pass: lifecycle, then card
    /// execution (Part B), then the inbox — notices and `tell
    /// hollywood` — under the guarded send. Config errors log once;
    /// no `[hollywood]` means no terminal and no inbox work, though
    /// applied cards still execute. A dead wire is fatal like any
    /// studio's; every other failure is logged once.
    pub(crate) async fn hollywood(
        &self,
        terminals: &[LiveTerminal],
        finished: &[LiveTerminal],
        real_slot_free: &mut bool,
        now_ms: u64,
    ) -> Result<(), Fail> {
        let hdir = store::hollywood_dir(&self.base);
        self.hrefresh(&hdir);
        let cfg = match config::hollywood(&self.env_vars) {
            Ok(c) => c,
            Err(e) => {
                self.hlog(&e);
                None
            }
        };
        *self.hcfg.borrow_mut() = cfg.clone();
        if let Err(e) = self.propose_outward(&hdir) {
            self.hlog(&format!("outward: {}", e.message()));
        }
        if let Err(e) = self.propose_repo_notes(&hdir) {
            self.hlog(&format!("repo-note: {}", e.message()));
        }
        if let Err(e) = self.hcards(&hdir) {
            self.hlog(&format!("cards: {}", e.message()));
        }
        match self
            .drive_handoff(
                &hdir,
                cfg.as_ref(),
                terminals,
                finished,
                real_slot_free,
                now_ms,
            )
            .await
        {
            Err(e @ Fail::Core(_)) => return Err(e),
            Err(e) => self.hlog(&format!("handoff: {}", e.message())),
            Ok(()) => {}
        }
        self.hrefresh(&hdir);
        let cfg = match config::hollywood(&self.env_vars) {
            Ok(c) => c,
            Err(e) => {
                self.hlog(&e);
                None
            }
        };
        *self.hcfg.borrow_mut() = cfg.clone();
        if !crate::handoff::handoff_blocks_delivery(&hdir) {
            match self
                .hreconcile(&hdir, cfg.as_ref(), terminals, finished, real_slot_free)
                .await
            {
                Err(e @ Fail::Core(_)) => return Err(e),
                Err(e) => self.hlog(&format!("reconcile: {}", e.message())),
                Ok(()) => {}
            }
        }
        match self.sweep_hollywood_finished(finished, now_ms).await {
            Err(e @ Fail::Core(_)) => return Err(e),
            Err(e) => self.hlog(&format!("sweep: {}", e.message())),
            Ok(()) => {}
        }
        let Some(cfg) = &cfg else {
            return Ok(());
        };
        if !self.hagents_synced.get() {
            if let Err(e) = refresh_agents_md(&hdir) {
                self.hlog(&format!("AGENTS.md: {}", e.message()));
            } else {
                self.hagents_synced.set(true);
            }
        }
        self.hrefresh(&hdir);
        self.hdiscover(&hdir, terminals);
        self.hsample(&hdir, terminals, &cfg.runner, now_ms).await;
        match self.hdeliver(&hdir, terminals, now_ms).await {
            Err(e @ Fail::Core(_)) => return Err(e),
            Err(e) => self.hlog(&format!("deliver: {}", e.message())),
            Ok(()) => {}
        }
        Ok(())
    }

    /// The lifecycle: plan from the role record and the live view,
    /// then carry the moves out — records first for starts and
    /// resumes (a crash leaves a terminal to adopt), before the
    /// removal for closes, like the studios'.
    async fn hreconcile(
        &self,
        hdir: &Path,
        cfg: Option<&HollywoodCfg>,
        terminals: &[LiveTerminal],
        finished: &[LiveTerminal],
        real_slot_free: &mut bool,
    ) -> Result<(), Fail> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let role = self.hstate.borrow().role.clone();
        let term = crate::handoff_term::active_hollywood_terminal(&role, terminals);
        let term_display = term.map(|t| t.name.as_str()).unwrap_or(HOLLYWOOD_TERMINAL);
        for mv in hstage::hplan(cfg, &role, term.is_some(), now_ms, self.started_ms) {
            if matches!(
                mv,
                HMove::Start
                    | HMove::Revive
                    | HMove::ReviveResume
                    | HMove::Restart
                    | HMove::Lost
                    | HMove::ResumeRun
                    | HMove::Close(_)
                    | HMove::Drop
            ) {
                self.sweep_hollywood_ticks.set(3);
            }
            match mv {
                HMove::Start | HMove::Revive => {
                    let term_name = free_hollywood_name(hdir, terminals, finished);
                    let mut actor = hstage::identity(cfg.expect("a start needs the config"));
                    let recast_from = role
                        .actor
                        .as_ref()
                        .filter(|old| !backlot_model::recast::same_identity(old, &actor));
                    match self
                        .hrun(
                            hdir,
                            &term_name,
                            &mut actor,
                            None,
                            recast_from,
                            real_slot_free,
                        )
                        .await?
                    {
                        HRun::Up { address, session } => {
                            hstage::record_role(
                                hdir,
                                RoleMotion::Started,
                                Some(address),
                                Some(actor),
                                session,
                                // A restart's fresh start carries its
                                // own word, so the request is done.
                                role.restart
                                    .as_ref()
                                    .and_then(|r| r.fresh.then_some(RESTARTED)),
                            )?;
                            crate::stagelog::stage_log!("started hollywood as {term_name}");
                        }
                        HRun::Defer => {}
                        HRun::Refused(d) => self.hlog(&format!("launch refused: {d}")),
                    }
                }
                HMove::ReviveResume => {
                    let term_name = free_hollywood_name(hdir, terminals, finished);
                    let mut actor = role.actor.clone().unwrap_or_else(|| {
                        hstage::identity(cfg.expect("a resume needs the config"))
                    });
                    let resume = role.session_id.clone();
                    match self
                        .hrun(
                            hdir,
                            &term_name,
                            &mut actor,
                            resume.as_deref(),
                            None,
                            real_slot_free,
                        )
                        .await?
                    {
                        HRun::Up { address, session } => {
                            hstage::record_role(
                                hdir,
                                RoleMotion::Resumed,
                                Some(address),
                                Some(actor),
                                session.or(resume),
                                Some(RESTARTED),
                            )?;
                            crate::stagelog::stage_log!("restarted {term_name}");
                        }
                        HRun::Defer => {}
                        HRun::Refused(d) => self.hlog(&format!("launch refused: {d}")),
                    }
                }
                HMove::Restart => {
                    hstage::record_role(
                        hdir,
                        RoleMotion::Closed,
                        None,
                        None,
                        None,
                        Some(RESTARTED),
                    )?;
                    if let Some(t) = term {
                        self.remove(&t.address).await?;
                    }
                    crate::stagelog::stage_log!("restarting {term_display}");
                }
                HMove::Adopt => {
                    let t = term.expect("adopt needs the live terminal");
                    let actor = hstage::identity(cfg.expect("adopt needs the config"));
                    refresh_agents_md(hdir)?;
                    self.hagents_synced.set(true);
                    hstage::record_role(
                        hdir,
                        RoleMotion::Started,
                        Some(t.address.clone()),
                        Some(actor.clone()),
                        runners::session_id(&actor, &t.name),
                        Some("adopted after restart"),
                    )?;
                    crate::stagelog::stage_log!("adopted {}", t.name);
                }
                HMove::Rejoin => {
                    let t = term.expect("rejoin needs the live terminal");
                    refresh_agents_md(hdir)?;
                    self.hagents_synced.set(true);
                    hstage::record_role(
                        hdir,
                        RoleMotion::Resumed,
                        Some(t.address.clone()),
                        role.actor.clone(),
                        role.session_id.clone(),
                        Some("recovered"),
                    )?;
                    crate::stagelog::stage_log!("{} rejoined", t.name);
                }
                HMove::Lost => {
                    hstage::record_role(
                        hdir,
                        RoleMotion::Lost,
                        None,
                        None,
                        None,
                        Some("terminal gone"),
                    )?;
                    crate::stagelog::stage_log!("lost {term_display}");
                }
                HMove::ResumeRun => {
                    let term_name = free_hollywood_name(hdir, terminals, finished);
                    let mut actor = role.actor.clone().unwrap_or_else(|| {
                        hstage::identity(cfg.expect("a resume needs the config"))
                    });
                    // Only a session recorded under this runner: a
                    // ledger from before the clear can still hold the
                    // previous runner's id.
                    let resume = hstage::resume_session(&role, &actor).map(str::to_string);
                    match self
                        .hrun(
                            hdir,
                            &term_name,
                            &mut actor,
                            resume.as_deref(),
                            None,
                            real_slot_free,
                        )
                        .await?
                    {
                        HRun::Up { address, session } => {
                            hstage::record_role(
                                hdir,
                                RoleMotion::Resumed,
                                Some(address),
                                Some(actor),
                                session.or(resume),
                                Some("recovered"),
                            )?;
                            crate::stagelog::stage_log!("resumed {term_name}");
                        }
                        HRun::Defer => {}
                        HRun::Refused(d) => {
                            hstage::record_role(
                                hdir,
                                RoleMotion::Lost,
                                None,
                                None,
                                None,
                                Some("resume refused"),
                            )?;
                            self.hlog(&format!("resume refused: {d}"));
                        }
                    }
                }
                HMove::GiveUp => {
                    hstage::record_role(
                        hdir,
                        RoleMotion::Closed,
                        None,
                        None,
                        None,
                        Some("cannot resume"),
                    )?;
                    crate::stagelog::stage_log!("{term_display} cannot resume — staying down");
                }
                HMove::Close(reason) => {
                    hstage::record_role(hdir, RoleMotion::Closed, None, None, None, Some(reason))?;
                    if let Some(t) = term {
                        self.remove(&t.address).await?;
                    }
                    crate::stagelog::stage_log!("closed {term_display}: {reason}");
                }
                HMove::Drop => {
                    if let Some(t) = term {
                        self.remove(&t.address).await?;
                    }
                }
            }
        }
        Ok(())
    }

    /// `terminal.run` Hollywood: plan the launch, let
    /// `decide_start` spend the tick's real slot, prep the folder
    /// and hooks, then run — `term_name` (normally `bl-hollywood`, or a free
    /// `-next`/`-g<k>` name while a `hollywood-previous` tombstone stays in
    /// `finished`) in `hollywood/` with no `backlot.studio` label and no
    /// `BACKLOT_STUDIO`.
    async fn hrun(
        &self,
        hdir: &Path,
        term_name: &str,
        actor: &mut ActorIdentity,
        resume: Option<&str>,
        recast_from: Option<&ActorIdentity>,
        real_slot_free: &mut bool,
    ) -> Result<HRun, Fail> {
        let cwd = hdir.to_string_lossy().into_owned();
        let prompt = hstage::hfirst_prompt_recast(
            hdir,
            &actor.runner,
            resume,
            recast_from.map(|old| (old, &*actor)),
        );
        let grants: Vec<backlot_model::event::PermitRecord> =
            self.hstate.borrow().permits.values().cloned().collect();
        let planned = runners::plan(
            actor,
            &runners::LaunchCtx {
                terminal: term_name,
                cwd: &cwd,
                prompt: prompt.as_deref(),
                resume,
                studio_dir: &cwd,
                stage_env: &self.env_vars,
                role: HOLLYWOOD,
                checks: &[],
                answer_trust: self.stage_cfg.get().answer_trust,
            },
        )
        .map(|mut launch| {
            runners::apply_permits(&mut launch, &actor.runner, &grants);
            launch
        });
        match runners::decide_start(
            planned,
            &actor.runner,
            self.runner_env(&actor.runner),
            real_slot_free,
            &BTreeSet::new(),
            &cwd,
        ) {
            StartDecision::Attention(e) => Ok(HRun::Refused(format!("{}: {}", e.reason, e.detail))),
            StartDecision::Defer => Ok(HRun::Defer),
            StartDecision::Launch { launch, runner_env } => {
                actor.runner_version = launch
                    .argv
                    .first()
                    .and_then(|program| self.runner_version(program));
                hprepare_launch_recast(
                    hdir,
                    &actor.runner,
                    resume,
                    recast_from.map(|old| (old, &*actor)),
                )?;
                if let Some(prep) = &launch.prep {
                    match runners::prepare(prep) {
                        Err(e) => return Err(Fail::refused(format!("prep: {e}"))),
                        Ok(Some(runners::PrepNote::HooksUnavailable(rel))) => {
                            self.hlog(&format!("hooks unavailable ({rel} is tracked)"));
                        }
                        Ok(_) => {}
                    }
                }
                if let Some(hooks) = &launch.hooks {
                    crate::act::prepare_hooks(hooks)
                        .map_err(|e| Fail::refused(format!("hook files: {e}")))?;
                } else if let Some(sig) = launch.prep.as_ref().and_then(runners::prep_signals_file)
                {
                    runners::truncate_signals(sig)
                        .map_err(|e| Fail::refused(format!("hook files: {e}")))?;
                }
                let role_dir = hdir.join("roles").join(HOLLYWOOD);
                crate::guard::prepare_role(&role_dir, &self.exe_dir)
                    .map_err(|e| Fail::refused(format!("guard files: {e}")))?;
                let labels = BTreeMap::from([
                    ("backlot.role".to_string(), HOLLYWOOD.to_string()),
                    ("herdr.space".to_string(), "backlot".to_string()),
                    ("herdr.tab".to_string(), term_name.to_string()),
                    ("herdr.seen".to_string(), "0".to_string()),
                ]);
                let run = self
                    .client
                    .terminal_run(hrun_params_named(
                        launch.argv.clone(),
                        cwd,
                        hrole_env_named(
                            launch.env.clone(),
                            runner_env,
                            &self.state_dir,
                            &self.exe_dir,
                            &self.path_env,
                            term_name,
                        ),
                        labels,
                        term_name,
                    ))
                    .await;
                match run {
                    Ok(r) => Ok(HRun::Up {
                        address: r.terminal.address,
                        session: launch.session_id(),
                    }),
                    Err(CallError::Rpc(e)) => {
                        *real_slot_free = true;
                        Ok(HRun::Refused(format!("terminal.run: {e}")))
                    }
                    Err(e) => Err(fatal(e)),
                }
            }
        }
    }
}

/// Pick `bl-hollywood` when no live or finished terminal holds that name;
/// otherwise choose a free `-next` or `-g<k>` name so restart/resume inside the
/// `hollywood-previous` 10-minute tombstone window never collides.
fn free_hollywood_name(
    hdir: &Path,
    terminals: &[LiveTerminal],
    finished: &[LiveTerminal],
) -> String {
    let used: BTreeSet<String> = terminals
        .iter()
        .chain(finished.iter())
        .map(|t| t.name.clone())
        .collect();
    let generation = crate::handoff::load_handoff(hdir)
        .ok()
        .flatten()
        .map_or(0, |r| r.generation);
    crate::handoff_term::next_candidate_name(&used, generation)
}

/// The smallest grid Hollywood's terminal should ever start at: its
/// answers are read by a person who peeks or focuses Hollywood, and
/// a narrow grid (21 columns was seen) wraps them into
/// unreadable strips (H3).
pub const MIN_COLS: u16 = 100;
pub const MIN_ROWS: u16 = 30;

/// hc's default grid for a terminal with no viewer (`docs/parity.md`
/// § covered-pane sizes: "the core's 200×120 headless size").
pub const HC_DEFAULT_COLS: u16 = 200;
pub const HC_DEFAULT_ROWS: u16 = 120;

/// The initial grid Hollywood's `terminal.run` asks for: the minimum, or
/// hc's default when that is larger. Without it the terminal fell to
/// whatever a viewer's first attach asked for, which could be under the
/// floor before anyone read it.
pub fn hsize() -> hc_proto::terminal::InitialSize {
    hc_proto::terminal::InitialSize {
        cols: MIN_COLS.max(HC_DEFAULT_COLS),
        rows: MIN_ROWS.max(HC_DEFAULT_ROWS),
    }
}

/// The exact `terminal.run` Hollywood's launch sends with the default name.
#[cfg(test)]
pub(crate) fn hrun_params(
    argv: Vec<String>,
    cwd: String,
    env: BTreeMap<String, String>,
    labels: BTreeMap<String, String>,
) -> RunParams {
    hrun_params_named(argv, cwd, env, labels, HOLLYWOOD_TERMINAL)
}

pub(crate) fn hrun_params_named(
    argv: Vec<String>,
    cwd: String,
    env: BTreeMap<String, String>,
    labels: BTreeMap<String, String>,
    name: &str,
) -> RunParams {
    RunParams {
        argv,
        name: Some(name.to_string()),
        cwd: Some(cwd),
        env,
        labels,
        no_parent: true,
        no_notify: true,
        size: Some(hsize()),
        ..RunParams::default()
    }
}

/// Hollywood's `terminal.run` env with the default `bl-hollywood` terminal marker.
#[cfg(test)]
pub(crate) fn hrole_env(
    adapter_env: BTreeMap<String, String>,
    runner_env: BTreeMap<String, String>,
    state_dir: &str,
    exe_dir: &str,
    path_env: &str,
) -> BTreeMap<String, String> {
    hrole_env_named(
        adapter_env,
        runner_env,
        state_dir,
        exe_dir,
        path_env,
        HOLLYWOOD_TERMINAL,
    )
}

pub(crate) fn hrole_env_named(
    adapter_env: BTreeMap<String, String>,
    runner_env: BTreeMap<String, String>,
    state_dir: &str,
    exe_dir: &str,
    path_env: &str,
    terminal_name: &str,
) -> BTreeMap<String, String> {
    let mut env = adapter_env;
    let configured_path = runner_env.get("PATH").cloned();
    env.extend(runner_env);
    env.insert("BACKLOT_ROLE".into(), HOLLYWOOD.into());
    env.insert("BACKLOT_TERMINAL".into(), terminal_name.into());
    env.insert("BACKLOT_STATE_DIR".into(), state_dir.into());
    let role_dir = crate::guard::hollywood_role_dir(state_dir);
    crate::guard::apply_env(
        &mut env,
        &role_dir,
        exe_dir,
        path_env,
        configured_path.as_deref(),
    );
    env
}
