//! The connection-error taxonomy: Devin (and any runner) that stops
//! on `Connection error` until it is sent a short message. Its own
//! attention reason — counted apart from `stalled`, `rate limited`
//! and `agent error` — with a bounded short-backoff `continue` and
//! an escalation to a person. Observed on Devin 3000.12.32; never
//! assumed for a runner nobody watched.

use backlot_model::event::EventKind;
use backlot_model::summary::Work;

use crate::act::record;
use crate::cli::Fail;
use crate::daemon::Stage;
use crate::ratelimit;
use crate::reconcile::LiveTerminal;
use crate::sample::{Raise, Watched};

/// The connection-error attention reason.
pub const CONNECTION_ERROR: &str = "connection error";
/// A `continue` goes out this long after the error was first seen —
/// short enough to be a retry, long enough not to race a redraw.
pub const CONN_WAIT_MS: u64 = 15_000;
/// Auto-retries before the episode is a person's.
pub const MAX_CONN_RETRIES: u8 = 3;

/// Whether this sample shows a connection error: in an `error`
/// status's message, or in the fresh tail of a non-working screen.
/// A `working` terminal may be *printing* the words, so its screen
/// never counts; a rate limit elsewhere in the sample takes
/// precedence (the caller checks that).
pub(crate) fn matches(term: &LiveTerminal, screen: &str, work: Work) -> bool {
    let tail = ratelimit::fresh_tail(screen).to_lowercase();
    if let Some(status) = term.status.as_ref() {
        if status.state == "working" {
            return false;
        }
        if status.state == "error" && status.msg.to_lowercase().contains(CONNECTION_ERROR) {
            return true;
        }
    }
    work != Work::Working && tail.contains(CONNECTION_ERROR)
}

/// One role's connection retry exchange: `continue`s already sent
/// and when the next is due — the same shape as the rate-limit pace.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ConnPace {
    pub retries: u8,
    pub due_ms: u64,
}

/// What one sample of a still-erroring role should do.
pub(crate) enum Step {
    Wait,
    Send,
    Escalate,
}

/// The pace decision: under the budget and past `due` it is time to
/// send; at the budget it escalates; otherwise it waits.
pub(crate) fn step(pace: ConnPace, retries_left: bool, now_ms: u64) -> Step {
    if !retries_left {
        return Step::Escalate;
    }
    if now_ms >= pace.due_ms {
        return Step::Send;
    }
    Step::Wait
}

impl Stage {
    /// The connection-error pass on one fresh sample. The first
    /// sighting raises a stage-owned attention and schedules the
    /// first `continue`; three failed attempts escalate it to a
    /// person, and the banner disappearing resolves `resumed`. A
    /// cleared or escalated episode keeps its `conn_pace` entry, so
    /// it never re-raises while the banner stays.
    pub(crate) async fn connection_error(
        &self,
        studio: &str,
        w: &Watched<'_>,
        text: &str,
        work: Work,
        now_ms: u64,
    ) -> Result<(), Fail> {
        let (dir, state, role) = (w.dir, w.state, w.role);
        let pair = (studio.to_string(), role.to_string());
        let matched =
            matches(w.term, text, work) && ratelimit::limited(w.term, text, work, now_ms).is_none();
        let open = state
            .open_attention()
            .find(|a| a.role == role && a.reason == CONNECTION_ERROR);
        match (matched, open) {
            (false, Some(attn)) => {
                record(
                    dir,
                    EventKind::AttentionCleared {
                        target: attn.seq,
                        reason: Some("resumed".into()),
                    },
                )?;
                self.conn_pace.borrow_mut().remove(&pair);
            }
            (false, None) => {
                self.conn_pace.borrow_mut().remove(&pair);
            }
            (true, None) => {
                if self.conn_pace.borrow().contains_key(&pair) {
                    return Ok(());
                }
                self.raise_attention(
                    Raise {
                        dir,
                        role,
                        reason: CONNECTION_ERROR,
                        text,
                        needs_user: false,
                        bound: None,
                    },
                    move |seq| {
                        format!("connection error; auto-retry after 15s; screen saved to attention/{seq}.txt")
                    },
                )
                .await?;
                self.conn_pace.borrow_mut().insert(
                    pair,
                    ConnPace {
                        retries: 0,
                        due_ms: now_ms + CONN_WAIT_MS,
                    },
                );
                crate::stagelog::stage_log!("{role} connection error");
            }
            (true, Some(attn)) => {
                if attn.needs_user {
                    return Ok(());
                }
                let pace = self
                    .conn_pace
                    .borrow()
                    .get(&pair)
                    .copied()
                    .unwrap_or(ConnPace {
                        retries: 0,
                        due_ms: now_ms,
                    });
                match step(pace, pace.retries < MAX_CONN_RETRIES, now_ms) {
                    Step::Wait => {}
                    Step::Send => {
                        let sent = self
                            .driver
                            .send(&self.client, &w.term.address, ratelimit::CONTINUE)
                            .await;
                        match sent {
                            Ok(_) => {
                                self.taint_ask(studio, w.state, role);
                                eprintln!(
                                    "stage: sent '{:?}' to {role} after its connection error",
                                    ratelimit::CONTINUE
                                );
                                self.conn_pace.borrow_mut().insert(
                                    pair,
                                    ConnPace {
                                        retries: pace.retries + 1,
                                        due_ms: now_ms + CONN_WAIT_MS,
                                    },
                                );
                            }
                            Err(e) if crate::sendfail::retryable(&e) => {
                                let refusal = crate::sendfail::refusal(&e);
                                if !refusal.not_typed() {
                                    self.taint_ask(studio, w.state, role);
                                }
                                eprintln!(
                                    "stage: connection retry to {role} not delivered ({}), retrying later",
                                    refusal.text()
                                );
                            }
                            Err(e) => return Err(Fail::call(e)),
                        }
                    }
                    Step::Escalate => {
                        record(
                            dir,
                            EventKind::AttentionCleared {
                                target: attn.seq,
                                reason: Some(format!(
                                    "auto-retries exhausted after {}",
                                    MAX_CONN_RETRIES
                                )),
                            },
                        )?;
                        self.raise_attention(
                            Raise {
                                dir,
                                role,
                                reason: CONNECTION_ERROR,
                                text,
                                needs_user: true,
                                bound: None,
                            },
                            move |seq| {
                                format!(
                                    "connection error after {} auto-retries; needs the user — screen saved to attention/{seq}.txt",
                                    MAX_CONN_RETRIES
                                )
                            },
                        )
                        .await?;
                        eprintln!(
                            "stage: {role} connection error escalated after {} auto-retries",
                            MAX_CONN_RETRIES
                        );
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "connerr_tests.rs"]
mod tests;
