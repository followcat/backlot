//! Auto-reap of cleanly finished terminals: while both links are
//! online, an inventory record that is `finished` with reason `exited`
//! gets one `terminal.rm` per incarnation. `wherdr.run`/`wherdr.task`/
//! `wherdr.coordinator` labels and studio-owned terminals (the
//! `backlot.*` labels, `backlot-stage`, `bl-hollywood`) are never
//! reaped, and a `wherdr-request`
//! record keeps wherdr's launch-failure read window on a quick non-zero
//! exit. The worker connects lazily — no exited terminal, no connection
//! — and a transient failure re-arms the entry so the next publish
//! retries.
use super::*;
use hc_proto::client::Client;
use hc_proto::method;
use hc_proto::terminal::{Terminal, TerminalParams};
use hc_proto::watch::{Link, OpenRpc};
use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Per-call deadline, matching the seen writer's and the layout
/// store's interactive cap.
const OP: Duration = Duration::from_secs(15);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `true` only for a cleanly exited record (or a child killed solely
/// because its parent ended via `die_with_parent`) that nothing else
/// owns: never a manual `killed`, `signaled`, `holder_lost`,
/// `server_stop`, `host_shutdown`, `stopped_by_restart`, `live`, a
/// wherdr worker/coordinator label, or a studio terminal — roles, the
/// stage and hollywood are the stage's records to read and remove, not
/// the client's.
pub(super) fn should_reap_exited(term: &Terminal, now_ms: u64) -> bool {
    let died_with_parent = term.reason.as_deref() == Some("killed")
        && term
            .extra
            .get("ended_by")
            .and_then(|v| v.get("by"))
            .and_then(|v| v.as_str())
            == Some("die_with_parent");
    if term.state != "finished" || (term.reason.as_deref() != Some("exited") && !died_with_parent) {
        return false;
    }
    for label in [
        "wherdr.run",
        "wherdr.task",
        "wherdr.coordinator",
        "backlot.role",
    ] {
        if term.labels.contains_key(label) {
            return false;
        }
    }
    // `backlot.studio` spares studio-owned terminals — a shell is the
    // user's own though, so `backlot.shell` exempts it: an exited
    // shell reaps like any plain terminal.
    if term.labels.contains_key("backlot.studio") && !term.labels.contains_key("backlot.shell") {
        return false;
    }
    if matches!(
        term.name.as_deref(),
        Some("backlot-stage" | "backlot-server")
    ) || term
        .name
        .as_deref()
        .is_some_and(backlot_model::hollywood::is_hollywood_terminal_name)
    {
        return false;
    }
    !term.labels.contains_key("wherdr-request")
        || term.exit_code.unwrap_or(0) == 0
        || now_ms.saturating_sub(term.created_ms) >= 5_000
}

/// A transient failure re-arms the pair so the next publish retries.
fn unmark(reaped: &Mutex<HashSet<(String, u64)>>, terminal: &str, created_ms: u64) {
    reaped
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(&(terminal.to_owned(), created_ms));
}

/// The reap worker: the first queued exit opens the shared RPC opener
/// (nothing exits, nothing connects), one cached client afterwards —
/// reopened after any failure or wire death — and a 15 s cap per call.
/// Each item is `terminal.get` to confirm the incarnation is still the
/// exited one, then `terminal.rm` when it still is; `not_found`/`live`
/// are clean, anything else re-arms the entry.
fn spawn_reap_worker(
    open: OpenRpc,
    mut requests: mpsc::Receiver<(String, u64)>,
    reaped: Arc<Mutex<HashSet<(String, u64)>>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut client = None;
        while let Some((terminal, created_ms)) = requests.recv().await {
            if client.as_ref().is_none_or(Client::is_closed) {
                client = open().await.ok();
            }
            let Some(conn) = client.clone() else {
                unmark(&reaped, &terminal, created_ms);
                continue;
            };
            if conn.explain_unavailable(method::TERMINAL_GET).is_some()
                || conn.explain_unavailable(method::TERMINAL_RM).is_some()
            {
                continue;
            }
            let params = || TerminalParams {
                terminal: terminal.clone(),
                caller: None,
            };
            let current = match tokio::time::timeout(OP, conn.terminal_get(params())).await {
                Ok(Ok(current)) => current,
                Ok(Err(error)) if error.is_not_found() => continue,
                _ => {
                    client = None;
                    unmark(&reaped, &terminal, created_ms);
                    continue;
                }
            };
            if current.created_ms != created_ms || !should_reap_exited(&current, now_ms()) {
                continue;
            }
            match tokio::time::timeout(OP, conn.terminal_rm(params())).await {
                Ok(Ok(_)) => {}
                Ok(Err(error)) if error.is_not_found() || error.is_kind("live") => {}
                _ => {
                    client = None;
                    unmark(&reaped, &terminal, created_ms);
                }
            }
        }
    })
}

impl App {
    /// `set_operations` also starts the reaper; a restart drops the old
    /// worker, a fresh queue and a cleared dedup set.
    pub(super) fn start_reap_worker(&mut self, open: OpenRpc) {
        if let Some(task) = self.reap_task.take() {
            task.abort();
        }
        let (tx, rx) = mpsc::channel(64);
        self.reap_tx = Some(tx);
        self.reaped_exited
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        self.reap_task = Some(spawn_reap_worker(open, rx, self.reaped_exited.clone()));
    }

    /// Enqueue every eligible `exited` record — once per incarnation,
    /// deduped by `reaped_exited`; markers for records that left the
    /// inventory are pruned first.
    pub(super) fn reap_exited(&mut self) {
        let Some(reap_tx) = &self.reap_tx else {
            return;
        };
        if !matches!(self.view.link, Link::Online) || !matches!(self.view.rpc, Link::Online) {
            return;
        }
        let now = now_ms();
        let mut queue = Vec::new();
        {
            let mut reaped = self.reaped_exited.lock().unwrap_or_else(|p| p.into_inner());
            reaped.retain(|(id, created_ms)| {
                self.view
                    .inventory
                    .get(id)
                    .is_some_and(|t| t.created_ms == *created_ms)
            });
            for term in self.view.inventory.terminals().values() {
                if should_reap_exited(term, now)
                    && reaped.insert((term.address.clone(), term.created_ms))
                {
                    queue.push((term.address.clone(), term.created_ms));
                }
            }
        }
        for item in queue {
            if reap_tx.try_send(item.clone()).is_err() {
                self.reaped_exited
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .remove(&item);
            }
        }
    }
}

#[cfg(test)]
#[path = "reap_tests.rs"]
mod tests;
