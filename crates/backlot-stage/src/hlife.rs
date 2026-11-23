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
use crate::hstage::{self, HMove, HRun};
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
        match self
            .hreconcile(&hdir, cfg.as_ref(), terminals, real_slot_free)
            .await
        {
            Err(e @ Fail::Core(_)) => return Err(e),
            Err(e) => self.hlog(&format!("reconcile: {}", e.message())),
            Ok(()) => {}
        }
        if let Err(e) = self.hcards(&hdir) {
            self.hlog(&format!("cards: {}", e.message()));
        }
        let Some(cfg) = &cfg else {
            return Ok(());
        };
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
        real_slot_free: &mut bool,
    ) -> Result<(), Fail> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let term = terminals.iter().find(|t| t.name == HOLLYWOOD_TERMINAL);
        let role = self.hstate.borrow().role.clone();
        for mv in hstage::hplan(cfg, &role, term.is_some(), now_ms, self.started_ms) {
            match mv {
                HMove::Start | HMove::Revive => {
                    let mut actor = hstage::identity(cfg.expect("a start needs the config"));
                    match self.hrun(hdir, &mut actor, None, real_slot_free).await? {
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
                            crate::stagelog::stage_log!(
                                "started hollywood as {HOLLYWOOD_TERMINAL}"
                            );
                        }
                        HRun::Defer => {}
                        HRun::Refused(d) => self.hlog(&format!("launch refused: {d}")),
                    }
                }
                HMove::ReviveResume => {
                    let mut actor = role.actor.clone().unwrap_or_else(|| {
                        hstage::identity(cfg.expect("a resume needs the config"))
                    });
                    let resume = role.session_id.clone();
                    match self
                        .hrun(hdir, &mut actor, resume.as_deref(), real_slot_free)
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
                            crate::stagelog::stage_log!("restarted {HOLLYWOOD_TERMINAL}");
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
                    crate::stagelog::stage_log!("restarting {HOLLYWOOD_TERMINAL}");
                }
                HMove::Adopt => {
                    let t = term.expect("adopt needs the live terminal");
                    let actor = hstage::identity(cfg.expect("adopt needs the config"));
                    hstage::record_role(
                        hdir,
                        RoleMotion::Started,
                        Some(t.address.clone()),
                        Some(actor.clone()),
                        runners::session_id(&actor, HOLLYWOOD_TERMINAL),
                        Some("adopted after restart"),
                    )?;
                    crate::stagelog::stage_log!("adopted {HOLLYWOOD_TERMINAL}");
                }
                HMove::Rejoin => {
                    let t = term.expect("rejoin needs the live terminal");
                    hstage::record_role(
                        hdir,
                        RoleMotion::Resumed,
                        Some(t.address.clone()),
                        role.actor.clone(),
                        role.session_id.clone(),
                        Some("recovered"),
                    )?;
                    crate::stagelog::stage_log!("{HOLLYWOOD_TERMINAL} rejoined");
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
                    crate::stagelog::stage_log!("lost {HOLLYWOOD_TERMINAL}");
                }
                HMove::ResumeRun => {
                    let mut actor = role.actor.clone().unwrap_or_else(|| {
                        hstage::identity(cfg.expect("a resume needs the config"))
                    });
                    // Only a session recorded under this runner: a
                    // ledger from before the clear can still hold the
                    // previous runner's id.
                    let resume = hstage::resume_session(&role, &actor).map(str::to_string);
                    match self
                        .hrun(hdir, &mut actor, resume.as_deref(), real_slot_free)
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
                            crate::stagelog::stage_log!("resumed {HOLLYWOOD_TERMINAL}");
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
                    crate::stagelog::stage_log!(
                        "{HOLLYWOOD_TERMINAL} cannot resume — staying down"
                    );
                }
                HMove::Close(reason) => {
                    hstage::record_role(hdir, RoleMotion::Closed, None, None, None, Some(reason))?;
                    if let Some(t) = term {
                        self.remove(&t.address).await?;
                    }
                    crate::stagelog::stage_log!("closed {HOLLYWOOD_TERMINAL}: {reason}");
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
    /// and hooks, then run — `bl-hollywood` in `hollywood/` with no
    /// `backlot.studio` label and no `BACKLOT_STUDIO`. A refused
    /// run hands the tick's real slot back like a studio's does.
    async fn hrun(
        &self,
        hdir: &Path,
        actor: &mut ActorIdentity,
        resume: Option<&str>,
        real_slot_free: &mut bool,
    ) -> Result<HRun, Fail> {
        let cwd = hdir.to_string_lossy().into_owned();
        let prompt = hstage::hfirst_prompt(hdir, &actor.runner, resume);
        let planned = runners::plan(
            actor,
            &runners::LaunchCtx {
                terminal: HOLLYWOOD_TERMINAL,
                cwd: &cwd,
                prompt: prompt.as_deref(),
                resume,
                studio_dir: &cwd,
                stage_env: &self.env_vars,
                role: HOLLYWOOD,
                checks: &[],
            },
        );
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
                hprepare_launch(hdir, &actor.runner, resume)?;
                if let Some(prep) = &launch.prep {
                    runners::prepare(prep).map_err(|e| Fail::refused(format!("prep: {e}")))?;
                }
                if let Some(hooks) = &launch.hooks {
                    crate::act::prepare_hooks(hooks)
                        .map_err(|e| Fail::refused(format!("hook files: {e}")))?;
                }
                let labels = BTreeMap::from([
                    ("backlot.role".to_string(), HOLLYWOOD.to_string()),
                    ("herdr.space".to_string(), "backlot".to_string()),
                    ("herdr.tab".to_string(), HOLLYWOOD_TERMINAL.to_string()),
                    ("herdr.seen".to_string(), "0".to_string()),
                ]);
                let run = self
                    .client
                    .terminal_run(RunParams {
                        argv: launch.argv.clone(),
                        name: Some(HOLLYWOOD_TERMINAL.to_string()),
                        cwd: Some(cwd),
                        env: hrole_env(
                            launch.env.clone(),
                            runner_env,
                            &self.state_dir,
                            &self.exe_dir,
                            &self.path_env,
                        ),
                        labels,
                        no_parent: true,
                        no_notify: true,
                        ..RunParams::default()
                    })
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

/// Hollywood's `terminal.run` env: the role's own `BACKLOT_*`
/// markers (no `BACKLOT_STUDIO`), the runner's map, `[runner_env]`
/// and a PATH that leads with this exe's directory — `role_env`'s
/// shape minus the studio keys.
pub(crate) fn hrole_env(
    adapter_env: BTreeMap<String, String>,
    runner_env: BTreeMap<String, String>,
    state_dir: &str,
    exe_dir: &str,
    path_env: &str,
) -> BTreeMap<String, String> {
    let mut env = adapter_env;
    let configured_path = runner_env.get("PATH").cloned();
    env.extend(runner_env);
    env.insert("BACKLOT_ROLE".into(), HOLLYWOOD.into());
    env.insert("BACKLOT_STATE_DIR".into(), state_dir.into());
    env.insert(
        "PATH".into(),
        match configured_path {
            Some(p) => format!("{exe_dir}:{p}"),
            None => path_env.into(),
        },
    );
    env
}

/// The folder's durable files: `AGENTS.md` (rewritten every start),
/// `prompts/hollywood.first.txt` when `devin` starts fresh, and
/// `memory.md` (created once and never overwritten).
pub(crate) fn hprepare_launch(hdir: &Path, runner: &str, resume: Option<&str>) -> Result<(), Fail> {
    for sub in [
        "messages",
        "reports",
        "attention",
        "drafts",
        "roles",
        "prompts",
    ] {
        std::fs::create_dir_all(hdir.join(sub))
            .map_err(|e| Fail::refused(format!("hollywood dir: {e}")))?;
    }
    let agents = hdir.join("AGENTS.md");
    std::fs::write(&agents, crate::prompts::HOLLYWOOD)
        .map_err(|e| Fail::refused(format!("AGENTS.md: {e}")))?;
    if resume.is_none() && runner == "devin" {
        let line = crate::prompts::first_line(&agents);
        crate::prompts::first_file(hdir, HOLLYWOOD, &line)
            .map_err(|e| Fail::refused(format!("prompt: {e}")))?;
    }
    let memory = hdir.join("memory.md");
    if !memory.exists() {
        std::fs::write(&memory, "").map_err(|e| Fail::refused(format!("memory.md: {e}")))?;
    }
    Ok(())
}
