//! The stage loop: each tick reloads any ledger that grew, reconciles
//! desired roles against the live terminal inventory and carries the
//! actions out — `terminal.run`/`kill`/`rm` plus the ledger events that
//! record them. Inputs are the ledger file lengths (polled about once
//! a second) and hc events via `events.subscribe`; a core without it
//! is polled on the same cadence. Losing the core exits non-zero —
//! recovery is S6's.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use backlot_model::EventBy;
use backlot_model::event::{ActorIdentity, Event, EventKind, NewEvent};
use backlot_model::state::StudioState;
use hc_proto::client::{CallError, Client};
use hc_proto::events::SubscribeParams;
use hc_proto::subscribe::Subscription;
use hc_proto::terminal::{ListParams, RunParams, TerminalParams};

use crate::args::Common;
use crate::cli::{Env, Fail};
use crate::reconcile::{self, Action, Start};
use crate::store::{self, StoreError};
use crate::{link, runners};

/// Poll cadence: ledgers grow-check and, without `events.subscribe`,
/// the terminal inventory.
const TICK: Duration = Duration::from_secs(1);

/// Name → address for every live terminal the core knows.
async fn live_terminals(client: &Client) -> Result<BTreeMap<String, String>, Fail> {
    let list = client
        .terminal_list(ListParams::default())
        .await
        .map_err(|e| Fail::refused(format!("terminal.list failed: {e}")))?;
    Ok(list
        .terminals
        .into_iter()
        .filter(|t| t.state == "live")
        .filter_map(|t| t.name.map(|n| (n, t.address)))
        .collect())
}

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

/// Connections and per-studio cache the loop owns.
struct Stage {
    client: Client,
    /// The state base (`studios/<name>` lives under it).
    base: PathBuf,
    /// `PATH` for role terminals: this exe's directory first.
    path_env: String,
    /// `BACKLOT_STATE_DIR` for role terminals.
    state_dir: String,
    /// Reduced state per studio, reloaded when its log grew.
    cached: BTreeMap<String, StudioState>,
    /// `events.jsonl` length at each state's last load.
    lengths: BTreeMap<String, u64>,
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

    /// The env a role terminal gets: the runner's plus the role's own.
    fn role_env(
        &self,
        studio: &str,
        start: &Start,
        runner_env: BTreeMap<String, String>,
    ) -> BTreeMap<String, String> {
        let mut env = runner_env;
        env.insert("BACKLOT_STUDIO".into(), studio.into());
        env.insert("BACKLOT_ROLE".into(), start.kind.name().into());
        if let Some(task) = &start.task {
            env.insert("BACKLOT_TASK".into(), task.clone());
        }
        env.insert("BACKLOT_STATE_DIR".into(), self.state_dir.clone());
        env.insert("PATH".into(), self.path_env.clone());
        env
    }

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
                env: self.role_env(studio, start, launch.env.clone()),
                labels: Self::labels(studio, start),
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
    async fn act(
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

    /// Reload changed ledgers, reconcile each studio and act.
    async fn tick(&mut self, live: &BTreeMap<String, String>) -> Result<(), Fail> {
        for name in store::studios(&self.base)
            .map_err(|e| Fail::refused(format!("cannot read {}: {e}", self.base.display())))?
        {
            let dir = store::studio_dir(&self.base, &name);
            let len = fs::metadata(dir.join("events.jsonl"))
                .map(|m| m.len())
                .unwrap_or(0);
            if self.lengths.get(&name) != Some(&len) {
                self.cached.insert(name.clone(), load_studio(&dir, &name)?);
                self.lengths.insert(name, len);
            }
        }
        for name in self.cached.keys().cloned().collect::<Vec<_>>() {
            let dir = store::studio_dir(&self.base, &name);
            let state = &self.cached[&name];
            for action in reconcile::reconcile(&name, state, live) {
                self.act(&name, &dir, state, &action).await?;
            }
        }
        Ok(())
    }
}

/// One studio's state read for `tick`, errors saying which studio.
fn load_studio(dir: &Path, name: &str) -> Result<StudioState, Fail> {
    store::load_state(dir).map_err(|e| match e {
        StoreError::Io(e) => Fail::refused(format!("studio '{name}': {e}")),
        StoreError::Corrupt(c) => Fail::refused(format!("studio '{name}': {c}")),
        _ => Fail::refused("unexpected store error"),
    })
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

/// `stage run`: connect, then tick on every hc event and once a
/// second on the ledger lengths.
pub async fn run(common: &Common, env: &Env) -> Result<(), Fail> {
    let link = link::connect(
        env,
        common.session.as_deref().map(std::ffi::OsStr::new),
        common.remote.as_deref(),
    )
    .await?;
    let base = match env.get("BACKLOT_STATE_DIR") {
        Some(d) => PathBuf::from(d),
        None => store::base_dir(&env.vars, &link.core_id),
    };
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let path_env = match env.get("PATH") {
        Some(p) => format!("{}:{}", exe_dir.display(), p),
        None => exe_dir.to_string_lossy().into_owned(),
    };
    let mut stage = Stage {
        client: link.client.clone(),
        base: base.clone(),
        path_env,
        state_dir: base.to_string_lossy().into_owned(),
        cached: BTreeMap::new(),
        lengths: BTreeMap::new(),
    };
    // A core without `events.subscribe` still works, polled on TICK.
    let mut sub = Subscription::connect(&link.endpoint, None, &SubscribeParams::default())
        .await
        .ok();
    let mut live = live_terminals(&link.client).await?;
    println!("stage: live on core {}", link.core_id);
    loop {
        stage.tick(&live).await?;
        live = match sub.as_mut() {
            Some(sub) => match tokio::time::timeout(TICK, sub.recv()).await {
                // A terminal event or the cadence: refetch the view.
                Ok(Ok(_)) | Err(_) => live_terminals(&link.client).await?,
                Ok(Err(e)) => {
                    return Err(Fail::refused(format!("lost the core event stream: {e}")));
                }
            },
            None => {
                tokio::time::sleep(TICK).await;
                live_terminals(&link.client).await?
            }
        };
    }
}
