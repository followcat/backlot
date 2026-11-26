//! Detecting when a role's open `tell user` was answered directly in
//! its terminal rather than via `backlot tell` back.

use std::path::Path;

use backlot_model::caps::{self, StatusSource};
use backlot_model::event::EventKind;
use backlot_model::state::StudioState;
use backlot_model::summary::{Work, open_ask};
use backlot_model::{EventBy, NewEvent, Refusal};

use crate::cli::Fail;
use crate::daemon::Stage;
use crate::reasons;
use crate::signals;
use crate::store::{self, StoreError};

/// Per-`(studio, role)` state tracked while one `tell user` ask is open.
#[derive(Debug, Clone, Default)]
pub(crate) struct AskWatch {
    pub ask_seq: u64,
    pub prompt_ts: Option<u64>,
    pub waiting_seen: bool,
    pub tainted: bool,
    pub answered: bool,
}

/// Whether `runner` expects a prompt-submitted or pre-invocation hook
/// signal for this role's session instead of the `waiting` -> `working`
/// fallback.
pub(crate) fn prompt_hook_active(
    runner: &str,
    state: &StudioState,
    role: &str,
    session_id: Option<&str>,
    has_signals: bool,
) -> bool {
    let Some(cap) = caps::get(runner) else {
        return false;
    };
    if !caps::has_prompt_hook(runner) {
        return false;
    }
    let hooks_unavailable = state
        .open_attention()
        .any(|a| a.role == role && a.reason == reasons::HOOKS_UNAVAILABLE);
    if hooks_unavailable || session_id.is_none() {
        return false;
    }
    // Emulated runners (`fake`) register hooks on `--settings` but only
    // write `signals.jsonl` when `hook:` inputs are played; a session
    // with no signals at all falls back to screen transitions (`work:`).
    if cap.status_source == StatusSource::ScreenOnly && !has_signals {
        return false;
    }
    true
}

impl Stage {
    /// Mark `(studio, role)`'s open ask or producer's open `WaitingUser`
    /// design phase as touched by a stage write to its terminal (keys,
    /// delivered message/notice, or `continue`), so a turn started by
    /// that write cannot clear the ask or record as user input.
    pub(crate) fn taint_ask(&self, studio: &str, state: &StudioState, role: &str) {
        if role == "producer" {
            self.taint_design(studio, state);
        }
        let Some(ask) = open_ask(state, role) else {
            return;
        };
        let pair = (studio.to_string(), role.to_string());
        {
            let mut tainted = self.ask_tainted.borrow_mut();
            let entry = tainted.entry(pair.clone()).or_insert(0);
            *entry = (*entry).max(ask.seq);
        }
        if let Some(w) = self.ask_watch.borrow_mut().get_mut(&pair)
            && w.ask_seq == ask.seq
        {
            w.tainted = true;
        }
    }

    /// Check `role`'s hook signals and work state for an open `tell user`
    /// answered in its terminal without stage involvement, appending
    /// `ask.answered` once when detected.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_ask_answered(
        &self,
        studio: &str,
        dir: &Path,
        state: &StudioState,
        role: &str,
        runner: &str,
        session_id: Option<&str>,
        signal_tail: Option<&str>,
        work: Work,
    ) -> Result<(), Fail> {
        let pair = (studio.to_string(), role.to_string());
        let Some(ask) = open_ask(state, role) else {
            self.ask_watch.borrow_mut().remove(&pair);
            return Ok(());
        };
        let prompt_sig = session_id
            .zip(signal_tail)
            .and_then(|(sid, tail)| signals::last_prompt_submit(tail, sid))
            .map(|s| s.ts_ms);
        let has_signals = session_id
            .zip(signal_tail)
            .is_some_and(|(sid, tail)| signals::has_session_signal(tail, sid));
        let uses_prompt_hook = prompt_hook_active(runner, state, role, session_id, has_signals);

        let ledger_touched = state.role_stage_touch_seq.get(role).copied().unwrap_or(0) > ask.seq;
        let in_mem_tainted = self.ask_tainted.borrow().get(&pair).copied().unwrap_or(0) >= ask.seq;
        let in_flight_send = self.sending.borrow().contains_key(&pair);

        let mut emit_target: Option<u64> = None;
        {
            let mut map = self.ask_watch.borrow_mut();
            let fresh = map.get(&pair).is_none_or(|w| w.ask_seq != ask.seq);
            if fresh {
                let tainted = ledger_touched || in_mem_tainted;
                let baseline_prompt = if in_flight_send {
                    prompt_sig.filter(|&ts| ts <= ask.ts_ms)
                } else {
                    prompt_sig
                };
                let hook_after_ask = !tainted
                    && !in_flight_send
                    && uses_prompt_hook
                    && prompt_sig.is_some_and(|ts| ts > ask.ts_ms);
                if hook_after_ask {
                    emit_target = Some(ask.seq);
                }
                map.insert(
                    pair,
                    AskWatch {
                        ask_seq: ask.seq,
                        prompt_ts: baseline_prompt,
                        waiting_seen: work == Work::Waiting,
                        tainted,
                        answered: emit_target.is_some(),
                    },
                );
            } else if let Some(w) = map.get_mut(&pair) {
                w.tainted |= ledger_touched || in_mem_tainted;
                if w.answered || w.tainted || in_flight_send {
                    return Ok(());
                }
                if uses_prompt_hook {
                    let new_prompt = match (w.prompt_ts, prompt_sig) {
                        (None, Some(_)) => true,
                        (Some(old), Some(new)) => new > old,
                        _ => false,
                    };
                    if new_prompt {
                        emit_target = Some(ask.seq);
                        w.answered = true;
                    }
                    if let Some(ts) = prompt_sig {
                        w.prompt_ts = Some(w.prompt_ts.map_or(ts, |old| old.max(ts)));
                    }
                } else {
                    if w.waiting_seen && work == Work::Working {
                        emit_target = Some(ask.seq);
                        w.answered = true;
                    }
                    w.waiting_seen = work == Work::Waiting;
                }
            }
        }

        if let Some(target) = emit_target {
            record_ask_answered(dir, role, target)?;
        }
        Ok(())
    }
}

fn record_ask_answered(dir: &Path, role: &str, target: u64) -> Result<(), Fail> {
    if !dir.join("events.jsonl").is_file() {
        return Err(Fail::refused("studio archived"));
    }
    let role_name = role.to_string();
    match store::append(dir, move |_, _| {
        Ok::<_, std::convert::Infallible>(store::Pending {
            event: NewEvent::new(
                EventBy::role("stage"),
                EventKind::AskAnswered {
                    role: role_name.clone(),
                    target,
                },
            ),
            files: Vec::new(),
        })
    }) {
        Ok(ev) => {
            crate::stagelog::stage_log!(
                "ask #{target} for {role} answered in terminal (seq {})",
                ev.seq
            );
            Ok(())
        }
        Err(StoreError::Refused(Refusal::AskAnswered(_))) => Ok(()),
        Err(StoreError::Io(e)) => Err(Fail::refused(format!("append: {e}"))),
        Err(StoreError::Corrupt(c)) => Err(Fail::refused(format!("{c}"))),
        Err(StoreError::Refused(r)) => Err(Fail::refused(format!("{r}"))),
        Err(StoreError::Build(_)) => unreachable!("the build closure cannot fail"),
    }
}
