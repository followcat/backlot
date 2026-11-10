//! Delivering the outbox (S5): every undelivered `message.queued`
//! and every `report.sent` aimed at a role goes out with
//! `terminal.send`, oldest first, one at a time per role — the send
//! runs off the stage loop (it can hold for up to three minutes)
//! and its result lands on a later tick. `message.delivered` is
//! recorded only when the call answers sent; a refusal leaves the
//! message queued for a later tick with no second attention. A
//! crash between send and record can send twice — harmless, and
//! noted in the docs.
//!
//! S9d: a queued notice is checked against its source before the
//! send — when the source no longer stands `message.dropped` is
//! recorded and the message leaves the outbox without being sent.
//! Two or more pending items for one role merge into
//! `messages/digest-<top seq>.md` behind a single send.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use backlot_model::event::EventKind;
use backlot_model::state::StudioState;
use backlot_model::summary::Work;
use hc_proto::client::CallError;
use serde_json::json;
use tokio::task::JoinHandle;

use crate::act::record;
use crate::cli::Fail;
use crate::daemon::Stage;
use crate::digest::{self, OutItem};
use crate::notices;
use crate::reconcile::{self, LiveTerminal};
use crate::stale;

/// The role must be up this long before anything is sent.
pub const MIN_UP_MS: u64 = 5_000;
/// The least gap between two sends to one role.
pub const SEND_GAP_MS: u64 = 5_000;
/// Two identical screen samples must span this to count as stable.
pub const SCREEN_STABLE_MS: u64 = 2_000;
/// `terminal.send` may hold for the person's draft pause — give the
/// wire a little longer than the core's own three minutes.
const SEND_DEADLINE: Duration = Duration::from_secs(200);

/// One queued thing a role is owed: the items it carries (one, or
/// the digest's set in digest order), the `[backlot]` line to send,
/// and the digest file to write first when the items merged.
#[derive(Debug, Clone, PartialEq)]
pub struct Outbound {
    /// The role key the terminal name comes from.
    pub role: String,
    /// Every queued item this send covers, in digest order.
    pub items: Vec<OutItem>,
    pub text: String,
    /// `messages/digest-<top>.md` to write before the send goes out.
    pub digest: Option<String>,
}

/// The `[backlot]` line one queued item sends — a file names the
/// file, a file-less message says what is wanted.
pub(crate) fn line(
    from: &str,
    to: &str,
    file: Option<&str>,
    purpose: Option<&str>,
    dir: &Path,
) -> String {
    match file {
        Some(f) => format!("[backlot] {from} → {to}: read {}", dir.join(f).display()),
        None => match purpose {
            Some("plan") => format!(
                "[backlot] {from} → {to}: a plan and an acceptance list are wanted — agree them and publish"
            ),
            _ => format!("[backlot] {from} → {to}: your attention is wanted"),
        },
    }
}

/// The line one item sends on its own — a message by its file or
/// purpose, a report by its file.
fn item_line(it: &OutItem, dir: &Path) -> String {
    match it {
        OutItem::Message(m) => line(&m.from, &m.to, m.file.as_deref(), m.purpose.as_deref(), dir),
        OutItem::Report(r) => line(&r.from, &r.to, Some(&r.file), None, dir),
    }
}

/// Every role's pending items, one `Outbound` each. Messages and
/// reports share the per-role seq order; a `to: user` report is
/// never sent. One pending item keeps its own line; two or more
/// merge into `messages/digest-<top seq>.md` behind a single
/// `stage → <role>` line naming the file and the count.
pub fn outbox(state: &StudioState, dir: &Path) -> Vec<Outbound> {
    let mut by_role: BTreeMap<String, Vec<OutItem>> = BTreeMap::new();
    for m in &state.messages {
        if !m.delivered && !m.dropped {
            by_role
                .entry(m.to.clone())
                .or_default()
                .push(OutItem::Message(m.clone()));
        }
    }
    for r in &state.reports {
        if !r.delivered && r.to != "user" {
            by_role
                .entry(r.to.clone())
                .or_default()
                .push(OutItem::Report(r.clone()));
        }
    }
    by_role
        .into_iter()
        .map(|(role, mut items)| {
            items.sort_by_key(OutItem::seq);
            let (text, digest) = match items.as_slice() {
                [one] => (item_line(one, dir), None),
                many => {
                    let file = digest::file_name(many);
                    (
                        format!(
                            "[backlot] stage → {role}: read {} ({} items)",
                            dir.join(&file).display(),
                            many.len()
                        ),
                        Some(file),
                    )
                }
            };
            Outbound {
                role,
                items,
                text,
                digest,
            }
        })
        .collect()
}

/// A role's screen-stability track: the latest sample and when the
/// visible text last changed. Two identical samples spanning
/// [`SCREEN_STABLE_MS`] are "the same on two samples 2 s apart".
#[derive(Debug, Clone, PartialEq)]
pub struct Screen {
    pub text: String,
    /// When the current text first appeared (ms).
    pub same_since_ms: u64,
    /// When the latest sample was taken (ms).
    pub at_ms: u64,
}

impl Screen {
    /// Fold one work sample in.
    pub fn note(prev: Option<&Screen>, text: String, at_ms: u64) -> Screen {
        let same_since_ms = match prev {
            Some(p) if p.text == text => p.same_since_ms,
            _ => at_ms,
        };
        Screen {
            text,
            same_since_ms,
            at_ms,
        }
    }

    /// The visible text held still across the stability window.
    pub fn stable(&self) -> bool {
        self.at_ms.saturating_sub(self.same_since_ms) >= SCREEN_STABLE_MS
    }
}

/// The per-role send gate — each condition independent.
#[derive(Debug, Clone, PartialEq)]
pub struct Gate {
    /// The ledger says the role is live.
    pub live: bool,
    /// Its latest work sample — sends only when `waiting`.
    pub work: Option<Work>,
    /// Its screen-stability track.
    pub screen: Option<Screen>,
    /// When the role's current lifecycle began (ms).
    pub role_since_ms: u64,
    /// A send to it is in flight right now.
    pub in_flight: bool,
    /// When the last send to it finished, ok or refused (ms).
    pub sent_ms: Option<u64>,
}

/// Every gate a send must pass: live target, latest sample
/// `waiting`, screen stable for the window, up long enough, and no
/// send in flight or too recently finished.
pub fn gate_open(g: &Gate, now_ms: u64) -> bool {
    g.live
        && g.work == Some(Work::Waiting)
        && g.screen.as_ref().is_some_and(Screen::stable)
        && now_ms.saturating_sub(g.role_since_ms) >= MIN_UP_MS
        && !g.in_flight
        && g.sent_ms
            .is_none_or(|t| now_ms.saturating_sub(t) >= SEND_GAP_MS)
}

/// What a finished `terminal.send` answer means: an explicit
/// `sent: false` is a refusal that leaves the message queued, the
/// answer's `reason` (when given) for the stage's log. Anything
/// else a successful call returns — this core's `SendResult`
/// carries `status`/`turn`/`started` and no `sent` — means the
/// text went in.
pub fn send_outcome(answer: &serde_json::Value) -> Result<(), Option<String>> {
    match answer.get("sent").and_then(|s| s.as_bool()) {
        Some(false) => Err(answer
            .get("reason")
            .and_then(|r| r.as_str())
            .map(String::from)),
        _ => Ok(()),
    }
}

/// An in-flight `terminal.send`: the seqs it carries (one item, or
/// the digest's whole set) and the handle the tick collects on a
/// later pass.
pub struct InFlight {
    pub seqs: Vec<u64>,
    pub handle: JoinHandle<Result<serde_json::Value, CallError>>,
}

/// This studio's finished in-flight sends, drained for collection —
/// another studio's stays put for its own tick, whose records land
/// in its own ledger.
pub fn take_finished(
    sending: &mut BTreeMap<(String, String), InFlight>,
    studio: &str,
) -> Vec<((String, String), InFlight)> {
    let keys: Vec<(String, String)> = sending
        .iter()
        .filter(|(k, f)| k.0 == studio && f.handle.is_finished())
        .map(|(k, _)| k.clone())
        .collect();
    keys.into_iter()
        .filter_map(|k| sending.remove(&k).map(|f| (k, f)))
        .collect()
}

impl Stage {
    /// After the watch: land owed notices, collect finished sends,
    /// then offer each role's oldest queued item to `terminal.send`
    /// when its gate is open.
    pub(crate) async fn deliver(
        &self,
        studio: &str,
        dir: &Path,
        state: &StudioState,
        live: &[LiveTerminal],
        now_ms: u64,
    ) -> Result<(), Fail> {
        // Notices owed by this state, as ordinary queued messages —
        // the body file lands first so a crash mid-append leaves a
        // rewritable file, and `source_seq` dedups across restarts.
        let mut grew = false;
        for n in notices::owed(state, dir) {
            let dest = format!("messages/{}-{}.md", n.source_seq, n.label);
            let path = dir.join(&dest);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| Fail::refused(format!("notice file: {e}")))?;
            }
            std::fs::write(&path, &n.body)
                .map_err(|e| Fail::refused(format!("notice file: {e}")))?;
            eprintln!(
                "stage: notice {} ({}) queued for {}",
                n.source_seq, n.label, n.to
            );
            record(
                dir,
                EventKind::MessageQueued {
                    to: n.to,
                    file: Some(dest),
                    purpose: None,
                    source_seq: Some(n.source_seq),
                },
            )?;
            grew = true;
        }

        // A queued notice whose source no longer stands is recorded
        // dropped — the ledger keeps the skip, nobody is told. A send
        // already in flight is exempt: it cannot be un-sent.
        let in_flight: BTreeSet<u64> = self
            .sending
            .borrow()
            .iter()
            .filter(|(k, _)| k.0 == studio)
            .flat_map(|(_, f)| f.seqs.iter().copied())
            .collect();
        let dropped: Vec<(u64, &'static str)> = state
            .messages
            .iter()
            .filter(|m| !m.delivered && !m.dropped && !in_flight.contains(&m.seq))
            .filter_map(|m| stale::stale_reason(state, m).map(|r| (m.seq, r)))
            .collect();
        for (seq, reason) in &dropped {
            record(
                dir,
                EventKind::MessageDropped {
                    target: *seq,
                    reason: (*reason).into(),
                },
            )?;
            eprintln!("stage: message {seq} dropped: {reason}");
        }

        // Owed steps the idle owner has not run: nudge once, then
        // escalate — the messages join the same outbox below. When
        // this pass queued mail or dropped any, reload so the drops
        // count as gone and the queues as undelivered — both keep
        // their owner off the idle list.
        let fresh;
        let state = if grew || !dropped.is_empty() {
            fresh = crate::daemon::load_studio(dir)?;
            &fresh
        } else {
            state
        };
        self.nudge(studio, dir, state, live, now_ms)?;

        // Collect finished sends — a finished handle answers without
        // blocking the tick. Only this studio's: another's record
        // and seq belong to its own ledger.
        let done = take_finished(&mut self.sending.borrow_mut(), studio);
        for (pair, f) in done {
            self.sent_ms.borrow_mut().insert(pair.clone(), now_ms);
            match f.handle.await {
                Ok(Ok(v)) => match send_outcome(&v) {
                    Ok(()) => {
                        // Every item the send covered is recorded —
                        // already-recorded duplicates are tolerated,
                        // a crash after the send can send twice.
                        for seq in &f.seqs {
                            let _ = record(dir, EventKind::MessageDelivered { target: *seq });
                        }
                        eprintln!("stage: delivered {:?} to {}", f.seqs, pair.1);
                    }
                    // An explicit `sent: false` is a refusal —
                    // queued for a later tick, send gap applies.
                    Err(reason) => {
                        eprintln!(
                            "stage: send to {} not sent{}, still queued",
                            pair.1,
                            reason.map(|r| format!(" ({r})")).unwrap_or_default()
                        );
                    }
                },
                // A refusal leaves it queued for a later tick — the
                // send gap still applies. A timeout is ambiguous:
                // the text may have gone in, so it stays queued and
                // a duplicate is possible but harmless.
                Ok(Err(CallError::Rpc(_))) | Ok(Err(CallError::Timeout)) => {
                    eprintln!("stage: send to {} refused, still queued", pair.1);
                }
                Ok(Err(e)) => {
                    return Err(Fail::call(e));
                }
                Err(e) => {
                    eprintln!("stage: send to {} lost: {e}", pair.1);
                }
            }
        }

        for out in outbox(state, dir) {
            let pair = (studio.to_string(), out.role.clone());
            let Some(name) = reconcile::terminal_name(studio, &out.role) else {
                continue;
            };
            if !live.iter().any(|t| t.name == name) {
                continue;
            }
            let role = state.roles.get(&out.role);
            let gate = Gate {
                live: role.is_some_and(|r| r.live),
                work: self.work.borrow().get(&pair).copied(),
                screen: self.screens.borrow().get(&pair).cloned(),
                role_since_ms: role.map(|r| r.since_ms).unwrap_or(0),
                in_flight: self.sending.borrow().contains_key(&pair),
                sent_ms: self.sent_ms.borrow().get(&pair).copied(),
            };
            if !gate_open(&gate, now_ms) {
                continue;
            }
            // The digest file lands before the send names it — the
            // stale sweep has already run, so its items are all live.
            if let Some(file) = &out.digest {
                let path = dir.join(file);
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| Fail::refused(format!("digest file: {e}")))?;
                }
                std::fs::write(&path, digest::body(&out.items, dir))
                    .map_err(|e| Fail::refused(format!("digest file: {e}")))?;
            }
            let address = live
                .iter()
                .find(|t| t.name == name)
                .map(|t| t.address.clone())
                .unwrap_or_default();
            let client = self.client.clone();
            let text = out.text.clone();
            let handle = tokio::spawn(async move {
                client
                    .call_timeout(
                        "terminal.send",
                        Some(json!({ "terminal": address, "text": text })),
                        SEND_DEADLINE,
                    )
                    .await
            });
            self.sending.borrow_mut().insert(
                pair,
                InFlight {
                    seqs: out.items.iter().map(OutItem::seq).collect(),
                    handle,
                },
            );
            match &out.digest {
                Some(f) => eprintln!(
                    "stage: sending {f} ({} items) to {}",
                    out.items.len(),
                    out.role
                ),
                None => eprintln!(
                    "stage: sending message {} to {}",
                    out.items[0].seq(),
                    out.role
                ),
            }
        }
        Ok(())
    }
}
