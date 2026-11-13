//! The time-bound attention episodes the per-role sample drives: a
//! screen — or, for a reporting terminal, an activity revision —
//! held unchanged for the stall limit raises `stalled`; an
//! `error` status or a waiting screen naming a limit opens a
//! `rate limited` episode the stage answers itself with bounded
//! auto-retries.

use backlot_model::event::{EventKind, STALLED};
use backlot_model::state::actor_task;
use backlot_model::summary::Work;
use hc_proto::client::CallError;

use crate::act::record;
use crate::cli::Fail;
use crate::daemon::Stage;
use crate::sample::{Raise, Watched};
use crate::stall;

impl Stage {
    /// The S12d rate-limit pass on one fresh sample: a limited
    /// role gets one stage-owned attention while the stage waits
    /// the parsed reset out and sends `continue`; three retries
    /// and the episode escalates `needs_user`. A `continue` that
    /// worked — the limit lifted — resolves `resumed`; a cleared
    /// or escalated episode never re-raises while the limit
    /// holds. Counted by its own reason, never as stall or rework.
    pub(crate) async fn rate_limit(
        &self,
        studio: &str,
        w: &Watched<'_>,
        text: &str,
        now_ms: u64,
    ) -> Result<(), Fail> {
        use crate::ratelimit as rl;
        let (dir, state, role) = (w.dir, w.state, w.role);
        let pair = (studio.to_string(), role.to_string());
        let limited = rl::limited(w.term, text);
        let open = state
            .open_attention()
            .find(|a| a.role == role && a.reason == rl::RATE_LIMITED);
        match (limited, open) {
            (None, Some(attn)) => {
                record(
                    dir,
                    EventKind::AttentionCleared {
                        target: attn.seq,
                        reason: Some("resumed".into()),
                    },
                )?;
                self.rate_pace.borrow_mut().remove(&pair);
            }
            (None, None) => {
                self.rate_pace.borrow_mut().remove(&pair);
            }
            (Some(wait), None) => {
                if self.rate_pace.borrow().contains_key(&pair) {
                    return Ok(());
                }
                self.raise_attention(
                    Raise {
                        dir,
                        role,
                        reason: rl::RATE_LIMITED,
                        text,
                        needs_user: false,
                        bound: None,
                    },
                    move |seq| {
                        format!("rate limited; auto-retry after the reset; screen saved to attention/{seq}.txt")
                    },
                )
                .await?;
                self.rate_pace.borrow_mut().insert(
                    pair,
                    rl::RatePace {
                        retries: 0,
                        due_ms: now_ms + wait + rl::RETRY_MARGIN_MS,
                    },
                );
                eprintln!("stage: {role} rate limited");
            }
            (Some(wait), Some(attn)) => {
                if attn.needs_user {
                    return Ok(());
                }
                let pace = self
                    .rate_pace
                    .borrow()
                    .get(&pair)
                    .copied()
                    .unwrap_or(rl::RatePace {
                        retries: 0,
                        due_ms: now_ms,
                    });
                match rl::step(pace, pace.retries < rl::MAX_RETRIES, now_ms) {
                    rl::Step::Wait => {}
                    rl::Step::Send => {
                        let sent = self
                            .client
                            .call(
                                "terminal.send",
                                Some(serde_json::json!({
                                    "terminal": w.term.address,
                                    "text": rl::CONTINUE,
                                })),
                            )
                            .await;
                        match sent {
                            Ok(_) => {
                                eprintln!(
                                    "stage: sent '{:?}' to {role} after its rate-limit reset",
                                    rl::CONTINUE
                                );
                                self.rate_pace.borrow_mut().insert(
                                    pair,
                                    rl::RatePace {
                                        retries: pace.retries + 1,
                                        due_ms: now_ms + wait + rl::RETRY_MARGIN_MS,
                                    },
                                );
                            }
                            Err(CallError::Rpc(_)) => {}
                            Err(e) => return Err(Fail::call(e)),
                        }
                    }
                    rl::Step::Escalate => {
                        record(
                            dir,
                            EventKind::AttentionCleared {
                                target: attn.seq,
                                reason: Some(format!(
                                    "auto-retries exhausted after {}",
                                    rl::MAX_RETRIES
                                )),
                            },
                        )?;
                        self.raise_attention(
                            Raise {
                                dir,
                                role,
                                reason: rl::RATE_LIMITED,
                                text,
                                needs_user: true,
                                bound: None,
                            },
                            move |seq| {
                                format!(
                                    "rate limited after {} auto-retries; needs the user — screen saved to attention/{seq}.txt",
                                    rl::MAX_RETRIES
                                )
                            },
                        )
                        .await?;
                        eprintln!(
                            "stage: {role} rate limit escalated after {} auto-retries",
                            rl::MAX_RETRIES
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// The S6 stall pass on the sample just stored: a screen held
    /// unchanged for the studio's `stall_limit_s` raises `stalled`
    /// once — one open attention per role and reason — with the
    /// screen saved under `attention/`; a screen that changed since
    /// the raise clears the attention. A stage restart loses the
    /// in-memory track, so the first sample after it reads as a
    /// fresh screen: an open stall clears and can re-raise one
    /// limit later — acceptable until S6b's recovery work.
    ///
    /// S9f adds the signal half: a role whose last hook signal
    /// reports Working and is older than the limit is as stalled as
    /// a frozen screen — the spinner may keep moving while the turn
    /// itself is dead. Either source can clear an open stall: a new
    /// screen, or a newer signal.
    pub(crate) async fn stall(
        &self,
        studio: &str,
        w: &Watched<'_>,
        work: Work,
        now_ms: u64,
    ) -> Result<(), Fail> {
        let (dir, state, role) = (w.dir, w.state, w.role);
        let pair = (studio.to_string(), role.to_string());
        // The structured stall's clock (S12d): while the terminal
        // reports `working`, its activity revision must keep
        // moving — `rev_since` is when the current revision first
        // showed. A terminal not reporting `working` drops its
        // entry and never stalls this way.
        let working = w.term.status.as_ref().is_some_and(|s| s.state == "working");
        let rev_since = match w.term.activity_revision.filter(|_| working) {
            None => {
                self.revision_since.borrow_mut().remove(&pair);
                None
            }
            Some(rev) => {
                let mut map = self.revision_since.borrow_mut();
                let e = map.entry(pair.clone()).or_insert((rev, now_ms));
                if e.0 != rev {
                    *e = (rev, now_ms);
                }
                Some(e.1)
            }
        };
        let Some(screen) = self.screens.borrow().get(&pair).cloned() else {
            return Ok(());
        };
        let signal = self.last_signal.borrow().get(&pair).cloned();
        if let Some(attn) = stall::open_stall(state, role) {
            // A newer signal — the turn demonstrably alive — an
            // activity revision that moved since the raise, or the
            // screen having moved ends the episode.
            let cleared = if signal.as_ref().is_some_and(|s| s.ts_ms > attn.ts_ms) {
                Some("new hook signal")
            } else if rev_since.is_some_and(|s| s > attn.ts_ms) {
                Some("activity resumed")
            } else if screen.same_since_ms > attn.ts_ms {
                Some("screen changed")
            } else {
                None
            };
            if let Some(reason) = cleared {
                record(
                    dir,
                    EventKind::AttentionCleared {
                        target: attn.seq,
                        reason: Some(reason.into()),
                    },
                )?;
            }
            return Ok(());
        }
        let task = actor_task(role)
            .and_then(|id| state.tasks.get(id))
            .map(|t| t.status);
        let limit_ms = u64::from(state.stall_limit_s()) * 1000;
        if let Some(sig) =
            signal.filter(|s| stall::signal_stalled(role, task, work, s, limit_ms, now_ms))
        {
            let silent_s = now_ms.saturating_sub(sig.ts_ms) / 1000;
            self.raise_attention(
                Raise {
                    dir,
                    role,
                    reason: STALLED,
                    text: &screen.text,
                    needs_user: false,
                    bound: None,
                },
                move |seq| {
                    format!("no hook signal for {silent_s}s; screen saved to attention/{seq}.txt")
                },
            )
            .await?;
            eprintln!("stage: {role} stalled");
        } else if rev_since.is_some_and(|s| now_ms.saturating_sub(s) >= limit_ms) {
            // The terminal still says `working` but its activity
            // revision has not moved for the limit — the
            // structured twin of the frozen-screen stall.
            let held_s = now_ms.saturating_sub(rev_since.unwrap_or(now_ms)) / 1000;
            self.raise_attention(
                Raise {
                    dir,
                    role,
                    reason: STALLED,
                    text: &screen.text,
                    needs_user: false,
                    bound: None,
                },
                move |seq| {
                    format!(
                        "status 'working' but activity revision unchanged for {held_s}s; screen saved to attention/{seq}.txt"
                    )
                },
            )
            .await?;
            eprintln!("stage: {role} stalled");
        } else if stall::stalled(role, task, work, &screen, limit_ms) {
            let held_s = screen.at_ms.saturating_sub(screen.same_since_ms) / 1000;
            self.raise_attention(
                Raise {
                    dir,
                    role,
                    reason: STALLED,
                    text: &screen.text,
                    needs_user: false,
                    bound: None,
                },
                move |seq| {
                    format!("screen unchanged for {held_s}s; screen saved to attention/{seq}.txt")
                },
            )
            .await?;
            eprintln!("stage: {role} stalled");
        }
        Ok(())
    }
}
