//! The rate-limit and agent-error episodes the per-role sample
//! drives: an `error` status carrying limit text, or a non-working
//! reporting core's (or a silent core's) fresh tail naming a limit,
//! opens a `rate limited` episode the stage answers itself with
//! bounded auto-retries — a weekly or monthly quota escalates at
//! once; a bare `error` raises `agent error` instead — observed,
//! never assumed. The stall pass lives in `stall.rs` and the
//! connection-error pass in `connerr.rs`.

use backlot_model::event::EventKind;
use backlot_model::summary::Work;

use crate::act::record;
use crate::cli::Fail;
use crate::daemon::Stage;
use crate::permit;
use crate::sample::{Raise, Watched};

fn hhmm_local(ms: u64) -> String {
    let timestamp = (ms / 1000).min(i64::MAX as u64) as libc::time_t;
    let mut local: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&timestamp, &mut local) }.is_null() {
        return "the reset".into();
    }
    format!("{:02}:{:02}", local.tm_hour, local.tm_min)
}

impl Stage {
    /// The S12d error-taxonomy pass on one fresh sample. A limited
    /// role gets one stage-owned attention while the stage waits
    /// the parsed reset out and sends `continue`; three retries
    /// and the episode escalates `needs_user`. A `continue` that
    /// worked — the limit lifted — resolves `resumed`; a cleared
    /// or escalated episode never re-raises while the limit
    /// holds. Counted by its own reason, never as stall or rework.
    /// A bare `error` status — no limit text in `status.msg` or the
    /// screen's fresh tail — is `agent error` instead: never an
    /// auto-`continue`, an actor's goes to the director and
    /// anything else's needs the user, and it clears when the
    /// status recovers or the limit text shows. `work` is the
    /// screen-derived state, so the silent-core fallback cannot
    /// fire on a screen merely *mentioning* a limit while it works.
    pub(crate) async fn rate_limit(
        &self,
        studio: &str,
        w: &Watched<'_>,
        text: &str,
        work: Work,
        now_ms: u64,
    ) -> Result<(), Fail> {
        use crate::ratelimit as rl;
        let (dir, state, role) = (w.dir, w.state, w.role);
        let pair = (studio.to_string(), role.to_string());
        let limited = rl::limited(w.term, text, work, now_ms);
        let conn = crate::connerr::matches(w.term, text, work);
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
            (Some(hit), None) => {
                if self.rate_pace.borrow().contains_key(&pair) {
                    return Ok(());
                }
                let until = hhmm_local(now_ms + hit.wait_ms + rl::RETRY_MARGIN_MS);
                let weekly_choice = rl::weekly_choice_dialog(text);
                self.raise_attention(
                    Raise {
                        dir,
                        role,
                        reason: rl::RATE_LIMITED,
                        text,
                        needs_user: hit.escalate_now,
                        bound: None,
                    },
                    move |seq| {
                        if hit.escalate_now {
                            if weekly_choice {
                                format!("weekly limit reached: options 1–2 wait, 3–4 cost money; needs the user — screen saved to attention/{seq}.txt")
                            } else {
                                format!("weekly or monthly limit exhausted; needs the user — screen saved to attention/{seq}.txt")
                            }
                        } else {
                            format!("waiting until {until}; auto-retry after the reset; screen saved to attention/{seq}.txt")
                        }
                    },
                )
                .await?;
                // The sentinel: a long quota spends the budget at
                // once, so a cleared attention never re-raises while
                // the banner holds.
                let retries = if hit.escalate_now { rl::MAX_RETRIES } else { 0 };
                self.rate_pace.borrow_mut().insert(
                    pair,
                    rl::RatePace {
                        retries,
                        due_ms: now_ms + hit.wait_ms + rl::RETRY_MARGIN_MS,
                    },
                );
                eprintln!(
                    "stage: {role} {}",
                    if hit.escalate_now {
                        "weekly/monthly limit"
                    } else {
                        "rate limited"
                    }
                );
            }
            (Some(hit), Some(attn)) => {
                if attn.needs_user {
                    return Ok(());
                }
                if hit.escalate_now {
                    self.escalate_rate_limit(
                        dir,
                        role,
                        attn.seq,
                        text,
                        "weekly or monthly limit".into(),
                        "weekly or monthly limit exhausted",
                    )
                    .await?;
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
                if hit.auto_continue {
                    if now_ms >= pace.due_ms {
                        self.escalate_rate_limit(
                            dir,
                            role,
                            attn.seq,
                            text,
                            "reset passed without movement".into(),
                            "rate-limit reset passed without movement; needs the user",
                        )
                        .await?;
                    }
                    return Ok(());
                }
                match rl::step(pace, pace.retries < rl::MAX_RETRIES, now_ms) {
                    rl::Step::Wait => {}
                    rl::Step::Send => {
                        let sent = self
                            .driver
                            .send(&self.client, &w.term.address, rl::CONTINUE)
                            .await;
                        match sent {
                            Ok(_) => {
                                self.taint_ask(studio, w.state, role);
                                eprintln!(
                                    "stage: sent '{:?}' to {role} after its rate-limit reset",
                                    rl::CONTINUE
                                );
                                self.rate_pace.borrow_mut().insert(
                                    pair,
                                    rl::RatePace {
                                        retries: pace.retries + 1,
                                        due_ms: now_ms + hit.wait_ms + rl::RETRY_MARGIN_MS,
                                    },
                                );
                            }
                            Err(e) if crate::sendfail::retryable(&e) => {
                                let refusal = crate::sendfail::refusal(&e);
                                if !refusal.not_typed() {
                                    self.taint_ask(studio, w.state, role);
                                }
                                eprintln!(
                                    "stage: rate-limit continue to {role} not delivered ({}), retrying later",
                                    refusal.text()
                                );
                            }
                            Err(e) => return Err(Fail::call(e)),
                        }
                    }
                    rl::Step::Escalate => {
                        self.escalate_rate_limit(
                            dir,
                            role,
                            attn.seq,
                            text,
                            format!("auto-retries exhausted after {}", rl::MAX_RETRIES),
                            &format!(
                                "rate limited after {} auto-retries; needs the user",
                                rl::MAX_RETRIES
                            ),
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
        // A bare `error` — the status word without any limit text
        // — raises `agent error` once and sends nothing; it clears
        // when the status recovers or limit text hands the role to
        // the episode above. A connection error is the connerr
        // pass's own episode, never `agent error`.
        let errored = rl::errored(w.term) && limited.is_none() && !conn;
        let err_open = state
            .open_attention()
            .find(|a| a.role == role && a.reason == rl::AGENT_ERROR);
        match (errored, err_open) {
            (false, Some(attn)) => {
                record(
                    dir,
                    EventKind::AttentionCleared {
                        target: attn.seq,
                        reason: Some(
                            if conn {
                                "connection error observed"
                            } else if rl::errored(w.term) {
                                "rate limit text observed"
                            } else {
                                "status recovered"
                            }
                            .into(),
                        ),
                    },
                )?;
            }
            (true, None) => {
                let user_owned = !permit::is_actor(role);
                let msg: String = w
                    .term
                    .status
                    .as_ref()
                    .and_then(|s| s.msg.lines().next())
                    .unwrap_or("")
                    .chars()
                    .take(120)
                    .collect();
                self.raise_attention(
                    Raise {
                        dir,
                        role,
                        reason: rl::AGENT_ERROR,
                        text,
                        needs_user: user_owned,
                        bound: None,
                    },
                    move |seq| {
                        if user_owned {
                            format!("terminal reports error: {msg}; needs the user — screen saved to attention/{seq}.txt")
                        } else {
                            format!("terminal reports error: {msg}; screen saved to attention/{seq}.txt")
                        }
                    },
                )
                .await?;
                crate::stagelog::stage_log!("{role} reports error");
            }
            _ => {}
        }
        Ok(())
    }

    /// The rate-limit escalation flips the same open attention to
    /// the person: one stall keeps one attention, so Hollywood hears
    /// about the episode once.
    async fn escalate_rate_limit(
        &self,
        dir: &std::path::Path,
        _role: &str,
        seq: u64,
        _text: &str,
        _cleared: String,
        detail: &str,
    ) -> Result<(), Fail> {
        record(
            dir,
            EventKind::AttentionEscalated {
                target: seq,
                detail: format!("{detail} — screen saved to attention/{seq}.txt"),
            },
        )?;
        Ok(())
    }
}
