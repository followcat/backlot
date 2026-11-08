// Derived from herdr-app src/window.rs `share_seen` (Apache-2.0).
//! The `herdr.seen` turn rule: while a pane with a status word is visible on
//! the active window, the client writes its current turn to the
//! `herdr.seen` label — once per turn, optimistic locally, never lowering a
//! turn the core already holds. Writes go over a dedicated RPC worker so a
//! failure only re-arms the pane, never raises an action notice.
use super::*;
use hc_proto::watch::OpenRpc;
use serde_json::json;

/// A `terminal.label` request: `{"terminal": t, "labels": {"herdr.seen": turn}}`.
pub(super) struct SeenWrite {
    pub terminal: String,
    pub turn: u64,
}

pub(super) struct SeenResult {
    pub terminal: String,
    pub turn: u64,
    pub result: Result<(), String>,
}

/// The write worker: one cached client (reopened after any failure), a
/// 15s cap per write like the layout store.
fn spawn_seen_worker(
    open: OpenRpc,
    mut requests: mpsc::Receiver<SeenWrite>,
    results: mpsc::Sender<SeenResult>,
) {
    tokio::spawn(async move {
        let mut client = None;
        while let Some(request) = requests.recv().await {
            if client.is_none() {
                client = open().await.ok();
            }
            let outcome = match &client {
                None => Err("core unavailable".to_owned()),
                Some(client) => tokio::time::timeout(
                    std::time::Duration::from_secs(15),
                    client.call(
                        "terminal.label",
                        Some(json!({
                            "terminal": request.terminal,
                            "labels": {"herdr.seen": request.turn.to_string()},
                        })),
                    ),
                )
                .await
                .map_err(|_| "label write timed out".to_owned())
                .and_then(|result| result.map(|_| ()).map_err(|e| e.to_string())),
            };
            if outcome.is_err() {
                client = None;
            }
            if results
                .send(SeenResult {
                    terminal: request.terminal,
                    turn: request.turn,
                    result: outcome,
                })
                .await
                .is_err()
            {
                break;
            }
        }
    });
}

impl App {
    /// `set_operations` also starts the seen writer, once.
    pub(super) fn start_seen_worker(&mut self, open: OpenRpc) {
        if let Some(requests) = self.seen_req_rx.take() {
            spawn_seen_worker(open, requests, self.seen_res_tx.clone());
        }
    }

    /// `share_seen`: on the active window, each visible pane with a word
    /// and a turn the core hasn't already recorded gets its `herdr.seen`
    /// label written — at most once per turn. Markers for panes that left
    /// the screen are dropped so a revisit sees the pane fresh.
    pub(super) fn share_seen(&mut self) {
        if self.outer_focused == Some(false) {
            return;
        }
        let visible: std::collections::HashSet<String> = self.visible_panes().into_iter().collect();
        self.seen_looked.retain(|id, _| visible.contains(id));
        for id in visible {
            let Some(term) = self.view.inventory.get(&id) else {
                continue;
            };
            // No word, not live, or already looked at this turn — skip.
            if crate::project::effective_status(term).is_none() || term.state != "live" {
                continue;
            }
            // The core's own record is authoritative: never lower it.
            let core_seen: u64 = term
                .labels
                .get("herdr.seen")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0)
                .max(*self.seen_looked.get(&id).unwrap_or(&0));
            if core_seen >= term.turn {
                self.seen_looked
                    .insert(id.clone(), core_seen.max(term.turn));
                continue;
            }
            // Optimistic: record the turn before the write so a republish
            // cannot re-arm the same one.
            self.seen_looked.insert(id.clone(), term.turn);
            if !self.seen_inflight.insert((id.clone(), term.turn)) {
                continue;
            }
            if self
                .seen_tx
                .try_send(SeenWrite {
                    terminal: id.clone(),
                    turn: term.turn,
                })
                .is_err()
            {
                self.seen_inflight.remove(&(id.clone(), term.turn));
            }
        }
    }

    /// A finished write clears `inflight`; a failed one also clears the
    /// optimistic marker so the next publish retries — silently, like the
    /// reference's `warn!`.
    pub(super) fn on_seen_result(&mut self, result: SeenResult) {
        self.seen_inflight
            .remove(&(result.terminal.clone(), result.turn));
        if result.result.is_err() && self.seen_looked.get(&result.terminal) == Some(&result.turn) {
            self.seen_looked.remove(&result.terminal);
        }
    }
}
