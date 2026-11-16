//! The Waiting probe worker and the overlay application. The worker
//! connects lazily like the seen writer and the reaper — no eligible
//! terminal, no connection — then runs, per cycle, one `machine.exec`
//! process listing per machine and one `file.read` per layer-2
//! terminal. A method the core does not offer turns its layer off
//! quietly; a wire failure skips the cycle, never raises a notice.
//! Verdicts never mutate core status: they only decide whether a shown
//! `Idle` draws as `Waiting` (see `crate::waiting` for the rules and
//! docs/sidebar.md for the semantics).

use super::agent_presentation::project_aggregate_status;
use super::*;
use crate::waiting::{self, Fingerprint, Layer2, ProbePlan, Verdict};
use hc_proto::client::Client;
use hc_proto::watch::{Link, OpenRpc};
use serde_json::json;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// Per-call deadline for a probe: a stalled exec or read must not
/// hold the cycle open.
const OP: Duration = Duration::from_secs(5);
/// The re-probe cadence while a terminal has a candidate shell or a
/// Waiting verdict — it must show and clear quickly.
const ACTIVE: Duration = Duration::from_secs(3);
/// The cadence once a listing found nothing for a terminal.
const QUIET: Duration = Duration::from_secs(10);
/// How soon a tick retries after a full request channel.
const RETRY: Duration = Duration::from_secs(1);

/// One cycle's request: the eligible terminals to probe now.
pub(super) type Cycle = Vec<ProbePlan>;

/// A probe's verdict for one terminal.
#[derive(Debug, Clone)]
pub(super) struct Outcome {
    pub address: String,
    pub fingerprint: Fingerprint,
    pub waiting: bool,
    /// The listing sighted a tool shell once — it needs a second
    /// consecutive sighting before it is evidence.
    pub candidate: bool,
}

/// A verdict's re-probe interval: fast while a shell is a candidate
/// or the verdict is Waiting, quiet once the listing is empty.
fn cadence_of(verdict: &Verdict) -> Duration {
    if verdict.waiting || verdict.candidate {
        ACTIVE
    } else {
        QUIET
    }
}

/// Standard-alphabet base64 decode — `ExecResult.stdout` arrives
/// base64; kept local for the same reason `base64_encode` is.
fn base64_decode(text: &str) -> Option<Vec<u8>> {
    fn value(byte: u8) -> Option<u32> {
        match byte {
            b'A'..=b'Z' => Some(u32::from(byte - b'A')),
            b'a'..=b'z' => Some(u32::from(byte - b'a' + 26)),
            b'0'..=b'9' => Some(u32::from(byte - b'0' + 52)),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let pad = chunk.iter().filter(|b| **b == b'=').count();
        let data_len = chunk.len() - pad;
        // Two symbols make a byte; padding only ever ends the stream.
        if data_len < 2 || pad > 2 || (pad > 0 && chunk.len() != 4) {
            return None;
        }
        let n: u32 = chunk[..data_len]
            .iter()
            .try_fold(0u32, |acc, b| value(*b).map(|v| (acc << 6) | v))?;
        let shifted = n << (6 * (4 - data_len));
        out.extend(shifted.to_be_bytes()[1..data_len].iter());
    }
    Some(out)
}

/// `ExecResult.stdout` as text, `None` on any failure — a failed exec
/// is not evidence either way.
async fn exec_stdout(conn: &Client, machine: &Option<String>, argv: &[&str]) -> Option<String> {
    let result = conn
        .call_timeout(
            "machine.exec",
            Some(json!({"argv": argv, "on": machine, "timeout": OP.as_secs_f64()})),
            OP,
        )
        .await
        .ok()?;
    String::from_utf8(base64_decode(result["stdout"].as_str()?)?).ok()
}

/// `FileReadResult.text`, `None` on any failure — a missing sessions
/// file simply means this process has none.
async fn read_file(conn: &Client, machine: &Option<String>, path: String) -> Option<String> {
    conn.call_timeout(
        "file.read",
        Some(json!({"path": path, "on": machine, "limit": 8192})),
        OP,
    )
    .await
    .ok()?
    .get("text")
    .and_then(|text| text.as_str())
    .map(str::to_owned)
}

/// The machine's home directory, probed once per connection: a
/// relative `file.read` resolves against the core's working
/// directory, not the home.
async fn home_of<'a>(
    conn: &Client,
    machine: &'a Option<String>,
    homes: &'a mut BTreeMap<Option<String>, Option<String>>,
) -> Option<&'a str> {
    if !homes.contains_key(machine) {
        let home = exec_stdout(conn, machine, &["sh", "-c", "printf %s \"$HOME\""])
            .await
            .map(|out| out.trim().to_owned())
            .filter(|out| !out.is_empty());
        homes.insert(machine.clone(), home);
    }
    homes.get(machine)?.as_deref()
}

/// The probe worker: the first queued cycle opens the shared opener,
/// one cached client afterwards — reopened on death like the seen
/// writer's. Stale queued cycles collapse to the newest before each
/// run.
fn spawn_waiting_worker(
    open: OpenRpc,
    mut requests: mpsc::Receiver<Cycle>,
    results: mpsc::Sender<Outcome>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut client = None;
        let mut homes: BTreeMap<Option<String>, Option<String>> = BTreeMap::new();
        // One listing's tool shells are candidates: a shell becomes
        // evidence only when the same pid shows in two consecutive
        // listings for the same terminal identity — a transient
        // `sh -c` (a statusLine or a hook) never does.
        let mut seen_once: BTreeMap<String, (Fingerprint, std::collections::HashSet<u32>)> =
            BTreeMap::new();
        while let Some(mut cycle) = requests.recv().await {
            while let Ok(newer) = requests.try_recv() {
                cycle = newer;
            }
            if client.as_ref().is_none_or(Client::is_closed) {
                homes.clear();
                seen_once.clear();
                client = open().await.ok();
            }
            let Some(conn) = client.clone() else {
                continue;
            };
            seen_once.retain(|address, _| cycle.iter().any(|plan| &plan.address == address));
            let exec = conn.explain_unavailable("machine.exec").is_none();
            let file = conn.explain_unavailable("file.read").is_none();
            let mut by_machine: BTreeMap<Option<String>, Vec<ProbePlan>> = BTreeMap::new();
            for plan in cycle {
                by_machine
                    .entry(plan.machine.clone())
                    .or_default()
                    .push(plan);
            }
            for (machine, plans) in by_machine {
                // One listing per machine per cycle covers every
                // terminal it holds.
                let listing = if exec && plans.iter().any(|p| p.process_tree) {
                    exec_stdout(&conn, &machine, waiting::ps::PS_ARGV)
                        .await
                        .map(|text| waiting::ps::parse_processes(&text))
                } else {
                    None
                };
                for plan in plans {
                    let mut candidate = false;
                    let mut waiting = false;
                    if plan.process_tree
                        && let Some(rows) = listing.as_ref()
                        && let Ok(pid) = u32::try_from(plan.fingerprint.pid)
                    {
                        let current = waiting::ps::tool_shell_descendant_pids(
                            rows,
                            pid,
                            waiting::STARTUP_WINDOW_SECS,
                        );
                        // A changed fingerprint (state, pid,
                        // incarnation, record) means a new identity —
                        // the count starts over for it too.
                        let prior = seen_once
                            .get(&plan.address)
                            .filter(|(fp, _)| fp == &plan.fingerprint)
                            .map(|(_, pids)| pids);
                        waiting =
                            prior.is_some_and(|pids| current.iter().any(|pid| pids.contains(pid)));
                        candidate = !current.is_empty();
                        seen_once.insert(plan.address.clone(), (plan.fingerprint.clone(), current));
                    }
                    if !waiting
                        && file
                        && plan.layer2 == Some(Layer2::ClaudeSessions)
                        && let Some(home) = home_of(&conn, &machine, &mut homes).await
                    {
                        let path = format!(
                            "{}/.claude/sessions/{}.json",
                            home.trim_end_matches('/'),
                            plan.fingerprint.pid
                        );
                        if let Some(text) = read_file(&conn, &machine, path).await {
                            waiting =
                                waiting::claude::session_is_shell(&text, plan.fingerprint.pid);
                        }
                    }
                    if results
                        .send(Outcome {
                            address: plan.address,
                            fingerprint: plan.fingerprint,
                            waiting,
                            candidate,
                        })
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            }
        }
    })
}

impl App {
    /// `set_operations` also starts the probe worker; a restart aborts
    /// the previous task first.
    pub(super) fn start_waiting_worker(&mut self, open: OpenRpc) {
        if let Some(requests) = self.waiting_req_rx.take() {
            if let Some(task) = self.waiting_task.take() {
                task.abort();
            }
            self.waiting_task = Some(spawn_waiting_worker(
                open,
                requests,
                self.waiting_res_tx.clone(),
            ));
        }
    }

    /// Prune verdicts the terminal outgrew, queue the due probes and
    /// arm the next cadence deadline. Runs on every publish and every
    /// armed deadline — never on a bare input wake.
    pub(super) fn tick_waiting(&mut self, now: Instant) {
        // The moment a terminal is no longer eligible — state, pid,
        // incarnation or record changed — its verdict is dropped.
        let fingerprints: std::collections::HashMap<String, Fingerprint> = self
            .view
            .inventory
            .terminals()
            .iter()
            .filter_map(|(id, term)| waiting::Fingerprint::of(term).map(|fp| (id.clone(), fp)))
            .collect();
        let before = self.waiting.len();
        self.waiting
            .retain(|address, verdict| fingerprints.get(address) == Some(&verdict.fingerprint));
        if self.waiting.len() < before && self.apply_waiting_overlay() {
            self.sched.mark_dirty(now);
        }
        // Without a started worker (`set_operations` has not run) or a
        // live pair of lanes, nothing probes and nothing re-arms.
        if self.waiting_req_rx.is_some()
            || !matches!(self.view.link, Link::Online)
            || !matches!(self.view.rpc, Link::Online)
        {
            self.waiting_due = None;
            return;
        }
        let mut cycle = Vec::new();
        for term in self.view.inventory.terminals().values() {
            let Some(plan) = waiting::probe_plan(term) else {
                continue;
            };
            let due = self
                .waiting
                .get(&plan.address)
                .is_none_or(|v| now.duration_since(v.probed_at) >= cadence_of(v));
            if due {
                cycle.push(plan);
            }
        }
        if !cycle.is_empty() && self.waiting_tx.try_send(cycle.clone()).is_err() {
            // A full queue means a cycle is still in flight; retry
            // soon instead of parking on the cadence.
            self.waiting_due = Some(now + RETRY);
            return;
        }
        for plan in cycle {
            // The cycle is queued; stamp `probed_at` so the cadence
            // starts now regardless of the worker's run time.
            self.waiting
                .entry(plan.address.clone())
                .and_modify(|v| v.probed_at = now)
                .or_insert(Verdict {
                    fingerprint: plan.fingerprint,
                    waiting: false,
                    candidate: false,
                    probed_at: now,
                });
        }
        self.waiting_due = self.waiting_next_due();
    }

    /// The soonest probe deadline across live verdicts — fast while
    /// any terminal has a candidate or a Waiting verdict, quiet for
    /// the ones whose last listing found nothing.
    fn waiting_next_due(&self) -> Option<Instant> {
        self.waiting
            .values()
            .map(|v| v.probed_at + cadence_of(v))
            .min()
    }

    /// A verdict landed: keep it only while the fingerprint it was
    /// probed for is still the live one, then fold it into the shown
    /// statuses — the overlay replaces a shown `Idle`, nothing else.
    pub(super) fn on_waiting_result(&mut self, outcome: Outcome) {
        let current = self
            .view
            .inventory
            .get(&outcome.address)
            .and_then(waiting::Fingerprint::of);
        if current.as_ref() != Some(&outcome.fingerprint) {
            return;
        }
        let entry = self
            .waiting
            .entry(outcome.address)
            .or_insert_with(|| Verdict {
                fingerprint: outcome.fingerprint.clone(),
                waiting: false,
                candidate: false,
                probed_at: Instant::now(),
            });
        entry.fingerprint = outcome.fingerprint;
        let flipped = entry.waiting != outcome.waiting;
        entry.waiting = outcome.waiting;
        entry.candidate = outcome.candidate;
        // A sighted candidate or a fresh verdict re-arms the deadline —
        // the next probe may be closer than the armed one.
        self.waiting_due = self.waiting_next_due();
        if flipped && self.apply_waiting_overlay() {
            self.sched.mark_dirty(Instant::now());
        }
    }

    /// Re-derive every agent's shown status from its base presentation
    /// plus the live verdicts — after a verdict lands or is pruned,
    /// without a reprojection. Returns whether anything changed.
    pub(super) fn apply_waiting_overlay(&mut self) -> bool {
        let mut changed = false;
        for endpoint in &mut self.projection.endpoints {
            let Some(snapshot) = endpoint.snapshot.as_mut() else {
                continue;
            };
            for agent in &mut snapshot.agents {
                let Some(base) = self.presentation.pane_status(&agent.pane_id) else {
                    continue;
                };
                let next = waiting::overlay(
                    base,
                    self.waiting.get(&agent.pane_id).is_some_and(|v| v.waiting),
                );
                if next != agent.agent_status {
                    agent.agent_status = next;
                    changed = true;
                }
            }
            project_aggregate_status(snapshot);
        }
        if changed {
            self.presentation.refresh_active(&mut self.projection);
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips_through_the_local_encoder() {
        for bytes in [b"ps listing\n".as_slice(), b"", &[0u8; 7], &[255u8; 5]] {
            let encoded = crate::selection::base64_encode(bytes);
            assert_eq!(base64_decode(&encoded).as_deref(), Some(bytes), "{encoded}");
        }
        assert_eq!(base64_decode("Zg"), Some(b"f".to_vec()));
        assert_eq!(base64_decode("Z m 8 ="), Some(b"fo".to_vec()));
        assert_eq!(base64_decode("!bad"), None);
    }
}
