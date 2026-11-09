//! Carrying out one reconcile action against the core and the ledger.
//! Ledger appends land before their terminal calls so a crash leaves
//! a recoverable state: a `role.closed` written first can never be
//! mistaken for a loss, and a `role.started` written last leaves a
//! live terminal to adopt.

use std::collections::BTreeMap;
use std::path::Path;

use backlot_model::EventBy;
use backlot_model::event::{ActorIdentity, Event, EventKind, NewEvent};
use backlot_model::state::StudioState;
use hc_proto::client::CallError;
use hc_proto::terminal::{RunParams, TerminalParams};

use crate::cli::Fail;
use crate::config;
use crate::daemon::Stage;
use crate::discover;
use crate::reconcile::{Action, Start};
use crate::runners::{self, LaunchCtx, StartDecision};
use crate::store::{self, StoreError};

/// A ledger event the stage appends for one studio.
pub(crate) fn record(dir: &Path, kind: EventKind) -> Result<Event, Fail> {
    store::append(dir, move |_state, _seq| {
        Ok::<_, std::convert::Infallible>(store::Pending {
            event: NewEvent::new(EventBy::role("stage"), kind),
            files: Vec::new(),
        })
    })
    .map_err(|e| match e {
        StoreError::Io(e) => Fail::refused(format!("append: {e}")),
        StoreError::Corrupt(c) => Fail::refused(format!("{c}")),
        StoreError::Refused(r) => Fail::refused(format!("{r}")),
        StoreError::Build(_) => unreachable!("the build closure cannot fail"),
    })
}

/// Call answers that mean the wire is gone — an `Rpc` answer is the
/// method's own verdict (usually "no such terminal"), tolerated.
fn fatal(e: CallError) -> Fail {
    Fail::refused(format!("core call failed: {e}"))
}

/// A call result where an `Rpc` verdict is fine — anything else fails.
fn tolerate(r: Result<serde_json::Value, CallError>) -> Result<(), Fail> {
    match r {
        Err(e) if !matches!(e, CallError::Rpc(_)) => Err(fatal(e)),
        _ => Ok(()),
    }
}

/// The identity a ledger role key maps to: studio role configuration
/// or the cast the `actor-<task>-c<n>` key names (used by `Adopt`,
/// where `Start`'s own `actor` is unavailable).
fn desired_actor<'s>(state: &'s StudioState, role: &str) -> Option<&'s ActorIdentity> {
    let cfg = state.studio.as_ref()?;
    match role {
        "producer" => Some(&cfg.roles.producer),
        "director" => Some(&cfg.roles.director),
        "writer" => Some(&cfg.roles.writer),
        _ => {
            let (task, n) = role.strip_prefix("actor-")?.rsplit_once("-c")?;
            let n: usize = n.parse().ok()?;
            state
                .tasks
                .get(task)?
                .casts
                .get(n.checked_sub(1)?)
                .map(|c| &c.actor)
        }
    }
}

/// The env a role terminal gets: the adapter's, then the runner's own
/// config map, then the role's structural variables on top — the
/// launch's working set never overrides `BACKLOT_*`. `PATH` is the
/// one structural variable a config may bend: a configured value
/// replaces the inherited PATH (a runner installed under nvm, say)
/// but this exe's directory still leads, so `backlot` and the
/// shipped tools stay reachable.
pub(crate) fn role_env(
    studio: &str,
    start: &Start,
    state_dir: &str,
    exe_dir: &str,
    path_env: &str,
    adapter_env: BTreeMap<String, String>,
    runner_env: BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut env = adapter_env;
    let configured_path = runner_env.get("PATH").cloned();
    env.extend(runner_env);
    env.insert("BACKLOT_STUDIO".into(), studio.into());
    env.insert("BACKLOT_ROLE".into(), start.kind.name().into());
    if let Some(task) = &start.task {
        env.insert("BACKLOT_TASK".into(), task.clone());
    }
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

/// `true` when `reason` attention is already open for `role` — the
/// one-open-per-role-and-reason rule every raiser shares. Only an
/// explicit resolve closes attention; nothing here clears it.
pub(crate) fn attention_open(state: &StudioState, role: &str, reason: &str) -> bool {
    state
        .open_attention()
        .any(|a| a.role == role && a.reason == reason)
}

/// The labels a role terminal carries (design.md "Role terminals").
pub(crate) fn labels(studio: &str, start: &Start) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::from([
        ("backlot.studio".to_string(), studio.to_string()),
        ("backlot.role".to_string(), start.role.clone()),
        ("herdr.space".to_string(), "backlot".to_string()),
        ("herdr.tab".to_string(), start.terminal.clone()),
        ("herdr.seen".to_string(), "0".to_string()),
    ]);
    if let Some(task) = &start.task {
        labels.insert("backlot.task".to_string(), task.clone());
    }
    labels
}

impl Stage {
    /// `terminal.kill` then `terminal.rm`, tolerating a terminal that
    /// is already gone. `address` is the hc address (`t7`).
    async fn remove(&self, address: &str) -> Result<(), Fail> {
        let kill = self
            .client
            .terminal_kill(TerminalParams {
                terminal: address.to_string(),
                caller: None,
            })
            .await;
        tolerate(kill.map(|t| serde_json::to_value(t).unwrap_or_default()))?;
        tolerate(
            self.client
                .call(
                    "terminal.rm",
                    Some(serde_json::json!({ "terminal": address })),
                )
                .await,
        )
    }

    /// `terminal.run` a role, then record `role.started` with its
    /// address. A method-level refusal (a name taken in a race, an
    /// argv the core rejects) raises `launch refused` attention —
    /// once per role and reason — reports `false` so the tick's
    /// real-launch slot is handed back, and the start is retried
    /// next tick. `true` is accepted; a dead wire is fatal.
    async fn launch(
        &self,
        studio: &str,
        dir: &Path,
        state: &StudioState,
        start: &Start,
        launch: &runners::Launch,
        runner_env: BTreeMap<String, String>,
    ) -> Result<bool, Fail> {
        let run = self
            .client
            .terminal_run(RunParams {
                argv: launch.argv.clone(),
                name: Some(start.terminal.clone()),
                cwd: Some(start.repo.clone()),
                env: role_env(
                    studio,
                    start,
                    &self.state_dir,
                    &self.exe_dir,
                    &self.path_env,
                    launch.env.clone(),
                    runner_env,
                ),
                labels: labels(studio, start),
                no_parent: true,
                no_notify: true,
                ..RunParams::default()
            })
            .await;
        let address = match run {
            Ok(r) => r.terminal.address,
            Err(CallError::Rpc(e)) => {
                eprintln!("stage: terminal.run {} refused: {e}", start.terminal);
                if !attention_open(state, &start.role, "launch refused") {
                    record(
                        dir,
                        EventKind::AttentionRaised {
                            role: start.role.clone(),
                            reason: "launch refused".into(),
                            detail: format!("terminal.run: {e}"),
                        },
                    )?;
                }
                return Ok(false);
            }
            Err(e) => return Err(fatal(e)),
        };
        record(
            dir,
            EventKind::RoleStarted {
                role: start.role.clone(),
                terminal: Some(address),
                actor: Some(start.actor.clone()),
                session_id: launch.session_id(),
                reason: None,
            },
        )?;
        eprintln!("stage: started {} as {}", start.role, start.terminal);
        Ok(true)
    }

    /// The `[runner_env.<runner>]` map from `<base>/config.toml` —
    /// secrets the terminal gets through `terminal.run` env only. A
    /// missing file is no extra env; a malformed one is the caller's
    /// `attention.raised` detail, so the role starts nothing.
    fn runner_env(&self, runner: &str) -> Result<BTreeMap<String, String>, String> {
        let path = Path::new(&self.state_dir).join("config.toml");
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        config::runner_env_in(&text, runner)
    }

    /// `<runner> --version`'s first line, asked once per runner per
    /// stage run; a failure stays an empty `runner_version`.
    fn runner_version(&self, runner: &str) -> Option<String> {
        if let Some(v) = self.versions.borrow().get(runner) {
            return v.clone();
        }
        let v = discover::runner_version(&*self.exec, runner);
        self.versions.borrow_mut().insert(runner.into(), v.clone());
        v
    }

    /// One reconcile action. Ledger appends land before their terminal
    /// calls so a crash leaves a recoverable state. `real_slot_free`
    /// is the tick's one-real-launch allowance: a deferred start
    /// waits for the next tick. `blocked` is every `(runner, folder)`
    /// with a live post-launch role still missing its session id.
    pub(crate) async fn act(
        &self,
        studio: &str,
        dir: &Path,
        state: &StudioState,
        action: &Action,
        real_slot_free: &mut bool,
        blocked: &std::collections::BTreeSet<(String, String)>,
    ) -> Result<(), Fail> {
        match action {
            Action::Lost { role, reason } => {
                record(
                    dir,
                    EventKind::RoleLost {
                        role: role.clone(),
                        reason: Some(reason.clone()),
                    },
                )?;
                eprintln!("stage: lost {role}: {reason}");
            }
            Action::Stop {
                role,
                address,
                close,
                reason,
                ..
            } => {
                if *close {
                    record(
                        dir,
                        EventKind::RoleClosed {
                            role: role.clone(),
                            reason: Some(reason.clone()),
                        },
                    )?;
                }
                eprintln!("stage: stopping {role}");
                self.remove(address).await?;
            }
            Action::DropTerminal { terminal, address } => {
                eprintln!("stage: dropping stray terminal {terminal} ({address})");
                self.remove(address).await?;
            }
            Action::Adopt {
                role,
                terminal,
                address,
            } => {
                let actor = desired_actor(state, role).cloned();
                record(
                    dir,
                    EventKind::RoleStarted {
                        role: role.clone(),
                        terminal: Some(address.clone()),
                        session_id: actor
                            .as_ref()
                            .and_then(|a| runners::session_id(a, terminal)),
                        actor,
                        reason: Some("adopted after restart".into()),
                    },
                )?;
                eprintln!("stage: adopted {terminal} for {role}");
            }
            Action::Start(start) => {
                let ctx = LaunchCtx {
                    terminal: &start.terminal,
                    cwd: &start.repo,
                    prompt: None,
                    resume: None,
                    stage_env: &self.env_vars,
                };
                match runners::decide_start(
                    runners::plan(&start.actor, &ctx),
                    &start.actor.runner,
                    self.runner_env(&start.actor.runner),
                    real_slot_free,
                    blocked,
                    &start.repo,
                ) {
                    // A refusal is an `attention.raised`, not a
                    // launch — one open attention per role and
                    // reason, re-checked each tick.
                    StartDecision::Attention(e) => {
                        if !attention_open(state, &start.role, e.reason) {
                            record(
                                dir,
                                EventKind::AttentionRaised {
                                    role: start.role.clone(),
                                    reason: e.reason.into(),
                                    detail: e.detail,
                                },
                            )?;
                        }
                    }
                    StartDecision::Defer => {}
                    StartDecision::Launch { launch, runner_env } => {
                        // `role.started` carries the runner's version
                        // when `--version` answered. A refused launch
                        // hands the tick's real-launch slot back so a
                        // stuck role cannot starve the ones behind it.
                        let mut start = start.clone();
                        start.actor.runner_version = self.runner_version(&start.actor.runner);
                        if !self
                            .launch(studio, dir, state, &start, &launch, runner_env)
                            .await?
                        {
                            *real_slot_free = true;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
