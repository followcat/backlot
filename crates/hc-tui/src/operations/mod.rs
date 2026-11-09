//! Bounded, owned RPC work. Only CAS conflicts rebase and retry; ambiguous
//! writes and terminal creation are never automatically replayed.
mod adoption;
mod edit;
mod rebase;
mod rpc;

use crate::structure::Document;
pub use edit::{Change, Edit, tab_key};
use hc_proto::{terminal::RunParams, watch::OpenRpc};
use std::collections::BTreeMap;
use tokio::{sync::mpsc, task::JoinHandle};

#[derive(Clone)]
pub struct Creation {
    pub params: RunParams,
    pub edit: Edit,
    pub promotion: Option<Promotion>,
    /// Adoption written for a brand-new workspace: records its stable
    /// identity and chosen label. `None` for creates inside existing spaces.
    pub space_adoption: Option<crate::structure::Adoption>,
    pub order: Vec<String>,
    /// Report the spawned terminal on `Written.created` — the `pane`
    /// command tracks it as an overlay that restores on exit.
    pub report_created: bool,
}

#[derive(Clone)]
pub struct Promotion {
    pub key: String,
    pub proposal: crate::structure::Adoption,
    pub members: Vec<Member>,
}

#[derive(Clone)]
pub struct Member {
    pub terminal: String,
    pub tab: String,
    pub labels: BTreeMap<String, String>,
}

pub enum Action {
    Edit(Edit),
    Create(Box<Creation>),
    Close {
        terminals: Vec<(String, u64)>,
        memberships: BTreeMap<String, (String, String)>,
        fallback: BTreeMap<String, crate::structure::TabLayout>,
        next: Option<String>,
    },
    RenamePane {
        terminal: String,
        created_ms: u64,
        name: String,
    },
    /// Persist a workspace label in the owned document's adoptions. `key` is
    /// the workspace's pre-canonical layout key; the proposal preserves the
    /// space's stable identity and only the name fields are overwritten.
    RenameSpace {
        key: String,
        proposal: crate::structure::Adoption,
    },
    /// Machine management RPCs — no foreign document is touched. The
    /// reference TUI has no machine dialogs (parity.md).
    Machine(MachineOp),
    /// Agent-menu operations — hc-tui extensions (parity.md). Each
    /// verifies the terminal's incarnation (`created_ms`) before acting,
    /// like `Close`.
    SendLine {
        terminal: String,
        created_ms: u64,
        text: String,
    },
    Interrupt {
        terminal: String,
        created_ms: u64,
    },
    Keep {
        terminal: String,
        created_ms: u64,
    },
    /// herdr-app's park: drop the `herdr.space`/`herdr.tab` placement
    /// labels, then `terminal.keep keep=false`.
    Park {
        terminal: String,
        created_ms: u64,
    },
    Kill {
        terminal: String,
        created_ms: u64,
        /// Popup teardown can lose the race to the core's `rm` cleanup.
        allow_gone: bool,
    },
    /// `machine.exec` — the `type = "shell"` custom command. Fire and
    /// report; the line runs in that machine's shell.
    Exec {
        line: String,
        machine: Option<String>,
    },
    /// `terminal.run` for the floating popup — the terminal is labeled
    /// `hc-tui.popup` so the projection keeps it out of the sidebar.
    RunPopup {
        params: RunParams,
    },
    /// Peek keep-as-tab: label the terminal into a fresh tab of the space,
    /// CAS the layout, then `terminal.keep keep=true` — herdr-app's
    /// `terminal.keep` from the peek bar (Apache-2.0 semantics).
    PeekKeep {
        edit: Edit,
        terminal: String,
        created_ms: u64,
        labels: BTreeMap<String, String>,
        order: Vec<String>,
    },
    /// Drop a dead terminal out of the layout: the `pane` command's
    /// overlay restore (reference `restore_overlay_after_exit`).
    Forget {
        terminal: String,
        created_ms: u64,
    },
    /// `terminal.put` — the R9 image-paste upload: write the image next
    /// to the terminal so the returned path can be pasted into it.
    /// `data` is base64; the incarnation check matches `Kill`/`SendLine`.
    PutImage {
        terminal: String,
        created_ms: u64,
        name: String,
        data: String,
    },
}

/// What a store request produced. `Written` carries the confirmed layout
/// version like before; `Ran` reports a created terminal's address.
pub enum Outcome {
    Written(Box<Written>),
    Ran(String),
    Done,
    /// A `terminal.put` landed: the terminal it wrote beside plus the
    /// path the core returned.
    Put {
        terminal: String,
        path: String,
    },
}

#[cfg(test)]
impl Outcome {
    /// Test shortcut for the common `Written` expectation.
    pub(crate) fn unwrap_written(self) -> Written {
        match self {
            Self::Written(written) => *written,
            _ => panic!("expected a Written outcome"),
        }
    }
}

pub enum MachineOp {
    Add { target: String },
    Connect { machine: String },
    Rename { machine: String, new_name: String },
    Remove { machine: String },
}

pub struct Request {
    pub core: String,
    pub action: Action,
}

pub struct Written {
    pub core: String,
    pub document: Document,
    pub version: u64,
    /// The terminal a `Create` spawned, when the creation asked for it
    /// (`keys.command` type="pane" tracks it as the overlay pane).
    pub created: Option<String>,
}

pub struct Store {
    send: mpsc::Sender<Request>,
    pub results: mpsc::Receiver<Result<Outcome, String>>,
    task: JoinHandle<()>,
}

/// The per-action deadline: `PutImage` pushes up to ~11 MB of base64
/// through the link, so it borrows the protocol call-timeout scale
/// (hc-proto's `CALL_TIMEOUT` = 120 s) instead of the interactive
/// default.
fn action_deadline(action: &Action) -> std::time::Duration {
    match action {
        Action::PutImage { .. } => std::time::Duration::from_secs(120),
        _ => std::time::Duration::from_secs(15),
    }
}

impl Store {
    pub fn new(open: OpenRpc) -> Self {
        let (send, mut receive) = mpsc::channel::<Request>(16);
        let (result, results) = mpsc::channel(16);
        let task = tokio::spawn(async move {
            let mut client = None;
            while let Some(request) = receive.recv().await {
                let work = tokio::time::timeout(
                    action_deadline(&request.action),
                    rpc::run(&open, &mut client, request),
                )
                .await
                .unwrap_or_else(|_| {
                    Err("Action timed out; its outcome may be unknown. It was not replayed.".into())
                });
                if work.is_err() {
                    client = None;
                }
                if result.send(work).await.is_err() {
                    break;
                }
            }
        });
        Self {
            send,
            results,
            task,
        }
    }

    pub fn submit(&self, request: Request) -> Result<(), String> {
        self.send
            .try_send(request)
            .map_err(|_| "Actions are busy; try again after the pending action finishes".into())
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod action_tests;

#[cfg(test)]
mod creation_tests;

#[cfg(test)]
mod rebase_tests;
