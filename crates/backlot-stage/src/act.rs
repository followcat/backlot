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
use crate::daemon::Stage;
use crate::reconcile::{Action, Start};
use crate::runners;
use crate::store::{self, StoreError};

/// A ledger event the stage appends for one studio.
fn record(dir: &Path, kind: EventKind) -> Result<Event, Fail> {
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

/// The env a role terminal gets: the runner's plus the role's own.
fn role_env(
    studio: &str,
    start: &Start,
    state_dir: &str,
    path_env: &str,
    runner_env: BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut env = runner_env;
    env.insert("BACKLOT_STUDIO".into(), studio.into());
    env.insert("BACKLOT_ROLE".into(), start.kind.name().into());
    if let Some(task) = &start.task {
        env.insert("BACKLOT_TASK".into(), task.clone());
    }
    env.insert("BACKLOT_STATE_DIR".into(), state_dir.into());
    env.insert("PATH".into(), path_env.into());
    env
}

/// The labels a role terminal carries (design.md "Role terminals").
fn labels(studio: &str, start: &Start) -> BTreeMap<String, String> {
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
    /// address. A method-level failure (a name taken in a race) is
    /// logged and retried next tick; a dead wire is fatal.
    async fn launch(
        &self,
        studio: &str,
        dir: &Path,
        start: &Start,
        launch: &runners::Launch,
    ) -> Result<(), Fail> {
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
                    &self.path_env,
                    launch.env.clone(),
                ),
                labels: labels(studio, start),
                no_parent: true,
                no_notify: true,
                ..RunParams::default()
            })
            .await;
        let address = match run {
            Ok(r) => r.terminal.address,
            Err(e @ CallError::Rpc(_)) => {
                eprintln!("stage: terminal.run {} refused: {e}", start.terminal);
                return Ok(());
            }
            Err(e) => return Err(fatal(e)),
        };
        record(
            dir,
            EventKind::RoleStarted {
                role: start.role.clone(),
                terminal: Some(address),
                actor: Some(start.actor.clone()),
                session_id: runners::session_id(&start.actor, &start.terminal),
                reason: None,
            },
        )?;
        eprintln!("stage: started {} as {}", start.role, start.terminal);
        Ok(())
    }

    /// One reconcile action. Ledger appends land before their terminal
    /// calls so a crash leaves a recoverable state.
    pub(crate) async fn act(
        &self,
        studio: &str,
        dir: &Path,
        state: &StudioState,
        action: &Action,
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
            Action::Start(start) => match runners::plan(&start.actor, &start.terminal) {
                // An unsupported runner is an `attention.raised`, not a
                // launch — one open attention per role, re-checked
                // each tick.
                Err(detail) => {
                    let open = state
                        .open_attention()
                        .any(|a| a.role == start.role && a.reason == "runner not supported");
                    if !open {
                        record(
                            dir,
                            EventKind::AttentionRaised {
                                role: start.role.clone(),
                                reason: "runner not supported".into(),
                                detail,
                            },
                        )?;
                    }
                }
                Ok(launch) => self.launch(studio, dir, start, &launch).await?,
            },
        }
        Ok(())
    }
}
