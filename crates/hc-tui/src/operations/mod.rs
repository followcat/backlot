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
    pub order: Vec<String>,
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
        fallback: BTreeMap<String, crate::structure::TabLayout>,
        next: Option<String>,
    },
    RenamePane {
        terminal: String,
        created_ms: u64,
        name: String,
    },
}

pub struct Request {
    pub core: String,
    pub action: Action,
}

pub struct Written {
    pub core: String,
    pub document: Document,
    pub version: u64,
}

pub struct Store {
    send: mpsc::Sender<Request>,
    pub results: mpsc::Receiver<Result<Option<Written>, String>>,
    task: JoinHandle<()>,
}

impl Store {
    pub fn new(open: OpenRpc) -> Self {
        let (send, mut receive) = mpsc::channel::<Request>(16);
        let (result, results) = mpsc::channel(16);
        let task = tokio::spawn(async move {
            let mut client = None;
            while let Some(request) = receive.recv().await {
                let work = tokio::time::timeout(
                    std::time::Duration::from_secs(15),
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
mod creation_tests;

#[cfg(test)]
mod rebase_tests;
