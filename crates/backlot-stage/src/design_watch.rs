//! Stage watch & taint state for producer terminal input capture during
//! `DesignPhase::WaitingUser`.

use std::path::{Path, PathBuf};

use backlot_model::design::DesignState;
use backlot_model::state::StudioState;
use backlot_model::summary::Work;

use crate::cli::Fail;
use crate::config::{self, DesignCfg};
use crate::daemon::Stage;
use crate::design_capture::{
    active_waiting_state, advance_cursor_past_unread, ingest_capture_lines, mark_pending_user_note,
};
use crate::signals;

/// Per-studio watch state while the producer is in `DesignPhase::WaitingUser`.
#[derive(Debug, Clone, Default)]
pub(crate) struct DesignWatch {
    pub version: u32,
    pub round: u32,
    pub summary_seq: u64,
    pub prompt_ts: Option<u64>,
    pub waiting_seen: bool,
    pub prev_work: Option<Work>,
    pub seen_touch_seq: u64,
    pub tainted: bool,
    pub tainted_saw_working: bool,
}

fn current_round_summary_seq(d: &DesignState) -> u64 {
    d.rounds
        .iter()
        .find(|r| r.round == d.round)
        .and_then(|r| r.summary_seq)
        .unwrap_or(0)
}

impl Stage {
    pub(crate) fn taint_design(&self, studio: &str, state: &StudioState) {
        if active_waiting_state(state).is_none() {
            return;
        }
        self.design_tainted
            .borrow_mut()
            .insert(studio.to_string(), true);
        if let Some(w) = self.design_watch.borrow_mut().get_mut(studio) {
            w.tainted = true;
        }
    }

    pub(crate) fn resolve_design_cfg(&self, dir: &Path) -> DesignCfg {
        if let Some(cfg) = self.design_cfg.get() {
            return cfg;
        }
        let inferred = if !self.state_dir.is_empty() {
            Some(PathBuf::from(&self.state_dir))
        } else if !self.base.as_os_str().is_empty() {
            Some(self.base.clone())
        } else if dir
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            == Some("studios")
        {
            dir.parent().and_then(Path::parent).map(Path::to_path_buf)
        } else {
            None
        };
        config::design_layered(inferred.as_deref(), &self.env_vars).unwrap_or_default()
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_design_capture(
        &self,
        studio: &str,
        dir: &Path,
        state: &StudioState,
        runner: &str,
        session_id: Option<&str>,
        signal_tail: Option<&str>,
        work: Work,
    ) -> Result<(), Fail> {
        let Some(d) = active_waiting_state(state) else {
            self.design_watch.borrow_mut().remove(studio);
            self.design_tainted.borrow_mut().remove(studio);
            return Ok(());
        };

        let summary_seq = current_round_summary_seq(d);
        let touch_seq = state
            .role_stage_touch_seq
            .get("producer")
            .copied()
            .unwrap_or(0);
        let in_mem_tainted = self
            .design_tainted
            .borrow()
            .get(studio)
            .copied()
            .unwrap_or(false);
        let in_flight_send = self
            .sending
            .borrow()
            .contains_key(&(studio.to_string(), "producer".to_string()));
        let prompt_sig = session_id
            .and_then(|sid| signal_tail.and_then(|tail| signals::last_prompt_submit(tail, sid)))
            .map(|s| s.ts_ms);
        let has_signals = session_id
            .zip(signal_tail)
            .is_some_and(|(sid, tail)| signals::has_session_signal(tail, sid));
        let uses_prompt_hook = crate::ask_answer::prompt_hook_active(
            runner,
            state,
            "producer",
            session_id,
            has_signals,
        );
        let hook_capture_on = self.resolve_design_cfg(dir).hook_capture;
        let use_hook_capture = hook_capture_on && uses_prompt_hook;

        let mut should_ingest_hook = false;
        let mut should_skip_hook = false;
        let mut should_mark_pending = false;
        let mut clear_taint_flag = false;

        {
            let mut map = self.design_watch.borrow_mut();
            let same_phase = map.get(studio).is_some_and(|w| {
                w.version == d.version && w.round == d.round && w.summary_seq == summary_seq
            });
            if !same_phase {
                let waiting_since = d.waiting_since_ms.unwrap_or(0);
                let tainted = in_mem_tainted;
                let baseline_prompt = if in_flight_send {
                    prompt_sig.filter(|&ts| ts <= waiting_since)
                } else {
                    prompt_sig
                };
                if tainted || in_flight_send {
                    should_skip_hook = true;
                } else if use_hook_capture {
                    should_ingest_hook = true;
                } else if uses_prompt_hook && prompt_sig.is_some_and(|ts| ts > waiting_since) {
                    should_mark_pending = true;
                }
                map.insert(
                    studio.to_string(),
                    DesignWatch {
                        version: d.version,
                        round: d.round,
                        summary_seq,
                        prompt_ts: baseline_prompt,
                        waiting_seen: work == Work::Waiting,
                        prev_work: Some(work),
                        seen_touch_seq: touch_seq,
                        tainted,
                        tainted_saw_working: tainted && work == Work::Working,
                    },
                );
            } else if let Some(w) = map.get_mut(studio) {
                if touch_seq > w.seen_touch_seq {
                    w.tainted = true;
                    w.seen_touch_seq = touch_seq;
                }
                w.tainted |= in_mem_tainted;
                let new_prompt = match (w.prompt_ts, prompt_sig) {
                    (None, Some(_)) => true,
                    (Some(old), Some(new)) => new > old,
                    _ => false,
                };
                if let Some(ts) = prompt_sig {
                    w.prompt_ts = Some(w.prompt_ts.map_or(ts, |old| old.max(ts)));
                }
                if w.tainted || in_flight_send {
                    w.tainted_saw_working |= work == Work::Working;
                    should_skip_hook = true;
                    if !in_flight_send
                        && work == Work::Waiting
                        && (w.tainted_saw_working
                            || new_prompt
                            || w.prev_work == Some(Work::Blocked))
                    {
                        w.tainted = false;
                        w.tainted_saw_working = false;
                        clear_taint_flag = true;
                    }
                } else if use_hook_capture {
                    should_ingest_hook = true;
                } else {
                    let untainted_turn = (uses_prompt_hook && new_prompt)
                        || (w.waiting_seen && work == Work::Working);
                    if untainted_turn {
                        should_mark_pending = true;
                    }
                }
                w.waiting_seen = work == Work::Waiting;
                w.prev_work = Some(work);
            }
        }

        if clear_taint_flag {
            self.design_tainted.borrow_mut().remove(studio);
        }
        if should_skip_hook {
            advance_cursor_past_unread(dir)?;
        } else if should_ingest_hook {
            ingest_capture_lines(dir, d.version, d.round)?;
        } else if should_mark_pending {
            mark_pending_user_note(dir, d, prompt_sig.unwrap_or(0))?;
        }
        Ok(())
    }
}
