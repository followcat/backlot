//! Hollywood's watch pass: the post-launch session discovery and
//! the work sample the summary and the send gate read. No
//! permission, stall or rate pass runs here — Hollywood's prompts
//! are the user's, never the stage's to answer (D17a), and the
//! layer owes no steps.

use std::path::Path;
use std::path::PathBuf;

use backlot_model::hollywood::{HOLLYWOOD, RoleMotion};
use backlot_model::summary::Work;
use hc_proto::terminal::ReadParams;

use crate::daemon::Stage;
use crate::hstage;
use crate::reconcile::LiveTerminal;
use crate::runners;
use crate::watch::{structured_state, work_due};

impl Stage {
    /// A live session-less role on a post-launch runner: look for
    /// the session id the way the studios' watch does, and record
    /// it on the `role.state` it belongs to (Hollywood's ledger has
    /// no separate `role.session` — a `started` carrying only the
    /// id is the same fact).
    pub(crate) fn hdiscover(&self, hdir: &Path, terminals: &[LiveTerminal]) {
        let role = self.hstate.borrow().role.clone();
        if !role.live || role.session_id.is_some() {
            return;
        }
        let Some(actor) = role.actor.as_ref() else {
            return;
        };
        if !runners::post_launch_session(&actor.runner) {
            return;
        }
        let Some(term) = crate::handoff_term::active_hollywood_terminal(&role, terminals) else {
            return;
        };
        let home = self
            .env_vars
            .get("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let mut taken = crate::watch::taken_ids(&self.cached, &self.found_sessions.borrow());
        if let Some(id) = &role.session_id {
            taken.insert(id.clone());
        }
        let look = crate::discover::Look {
            home: &home,
            role_dir: &hdir.join("roles").join(HOLLYWOOD),
            pid: term.pid,
        };
        let cwd = hdir.to_string_lossy().into_owned();
        let found = crate::discover::find_session(
            &actor.runner,
            &term.argv,
            &cwd,
            role.since_ms,
            &taken,
            &look,
            &*self.exec,
        );
        if let Some(id) = found {
            self.found_sessions.borrow_mut().insert(id.clone());
            if let Err(e) =
                hstage::record_role(hdir, RoleMotion::Started, None, None, Some(id), None)
            {
                self.hlog(&format!("session record: {}", e.message()));
            }
        }
    }

    /// The work sample: `roles/hollywood/signals.jsonl` first (when
    /// the recorded session emitted hook lines), then the core's
    /// structured state, then the runner's screen markers — identical
    /// to studio roles (`sample.rs`). No permission, stall or rate
    /// pass: Hollywood's prompts are the user's (D17a).
    pub(crate) async fn hsample(
        &self,
        hdir: &Path,
        terminals: &[LiveTerminal],
        runner: &str,
        now_ms: u64,
    ) {
        let pair = hstage::hkey();
        if !work_due(self.work_looked.borrow().get(&pair).copied(), now_ms) {
            return;
        }
        self.work_looked.borrow_mut().insert(pair.clone(), now_ms);
        let role = self.hstate.borrow().role.clone();
        let Some(t) = crate::handoff_term::active_hollywood_terminal(&role, terminals) else {
            return;
        };
        let text = match self
            .client
            .terminal_read(ReadParams {
                terminal: t.address.clone(),
                lines: 0,
                caller: None,
            })
            .await
        {
            Ok(r) => r.text,
            Err(_) => return,
        };
        let session = role.session_id.clone();
        let (signal, signal_state) = session
            .as_deref()
            .map(|sid| {
                let tail = crate::signals::tail(&crate::signals::signals_path(hdir, HOLLYWOOD));
                (
                    crate::signals::last_work(&tail, sid),
                    crate::signals::state(&tail, sid, &std::collections::BTreeSet::new()),
                )
            })
            .unwrap_or((None, None));
        let interrupted = crate::signals::note_role_interrupt(
            &self.interrupt_track,
            &pair,
            runner,
            &text,
            signal.as_ref(),
            signal_state,
        );
        let work = crate::signals::combine(&crate::signals::WorkInputs {
            runner,
            signal: signal_state,
            signal_ts_ms: signal.as_ref().map(|s| s.ts_ms),
            interrupted,
            structured: structured_state(t),
            screen: runners::work_state(runner, &text),
            now_ms,
        });
        self.work.borrow_mut().insert(pair.clone(), work);
        let model = role.actor.as_ref().map_or("", |a| a.model.as_str());
        self.record_account_role(HOLLYWOOD, HOLLYWOOD, runner, model);
        let outcome = {
            let mut rp = self.hrate_pace.borrow_mut();
            let mut cp = self.hconn_pace.borrow_mut();
            crate::handoff_term::sample_hollywood_error(t, &text, work, &mut rp, &mut cp, now_ms)
        };
        if let Some(hit) = outcome.quota_hit {
            let detail = match &outcome.blocked {
                Some(
                    crate::handoff::HBlockedKind::RateLimited { detail }
                    | crate::handoff::HBlockedKind::OutOfQuota { detail },
                ) => detail.as_str(),
                _ => "rate limited",
            };
            self.record_account_stall(
                HOLLYWOOD,
                HOLLYWOOD,
                runner,
                model,
                now_ms,
                hit.reset_at_ms,
                hit.window,
                detail,
            );
        } else {
            self.clear_account_stall_for_role(HOLLYWOOD, HOLLYWOOD);
        }
        *self.herror.borrow_mut() = outcome.blocked;
        if let Some(msg) = outcome.retry_send {
            let _ = self.driver.send(&self.client, &t.address, msg).await;
        }
        let is_q = crate::question::in_episode(runner, work, t.status.as_ref(), &text, None);
        self.hprompt_is_question.set(is_q);
        *self.hprompt.borrow_mut() = if is_q {
            Some(crate::question::dialog_summary(runner, &text))
        } else {
            hollywood_prompt(runner, &text, work)
        };
        let prev = self.screens.borrow().get(&pair).cloned();
        self.screens.borrow_mut().insert(
            pair,
            crate::deliver::Screen::note(prev.as_ref(), text, now_ms),
        );
        if crate::handoff::load_handoff(hdir)
            .ok()
            .flatten()
            .is_some_and(|h| h.phase == crate::handoff::HandoffPhase::NeedsApproval)
        {
            return;
        }
        self.hanswer(t, runner).await;
    }

    /// Send the runner's one-time allow key when the user has recorded
    /// a `role.approval` for the prompt on Hollywood's screen. The
    /// decision must post-date the prompt's first sighting: an answer
    /// to an earlier prompt never lands on this one. The key is the
    /// same `permit::approval_keys` a studio prompt uses — `enter` on
    /// cmd's `❯ 1. Yes` (run-once), never option 2's "don't ask
    /// again" (S12a).
    async fn hanswer(&self, term: &LiveTerminal, runner: &str) {
        if self.hprompt_is_question.get() {
            *self.hprompt_base.borrow_mut() = None;
            self.hanswered.borrow_mut().clear();
            return;
        }
        let Some(prompt) = self.hprompt.borrow().clone() else {
            *self.hprompt_base.borrow_mut() = None;
            self.hanswered.borrow_mut().clear();
            return;
        };
        let base = {
            let mut pace = self.hprompt_base.borrow_mut();
            match pace.as_ref() {
                Some((text, seq)) if text == &prompt => *seq,
                _ => {
                    let seq = self.hstate.borrow().last_seq;
                    *pace = Some((prompt.clone(), seq));
                    seq
                }
            }
        };
        let Some(appr) = self.hstate.borrow().approval_after(base).copied() else {
            return;
        };
        if !self.hanswered.borrow_mut().insert(appr.seq) {
            return;
        }
        let Some(key) = crate::permit::approval_keys(runner, appr.decision) else {
            return;
        };
        let sent = self
            .client
            .call(
                "terminal.keys",
                Some(serde_json::json!({
                    "terminal": term.address,
                    "keys": [key],
                })),
            )
            .await;
        match sent {
            Ok(_) => crate::stagelog::stage_log!("answered hollywood's permission prompt ({key})"),
            // A terminal that raced away — or a refused call — keeps
            // the approval answerable on the next sample.
            Err(_) => {
                self.hanswered.borrow_mut().remove(&appr.seq);
            }
        }
    }
}

/// The permission prompt Hollywood's screen shows, as the one line the
/// band asks about: the command a cmd picker names, else the picker's
/// question, unwrapped from the terminal's width. `None` when the
/// combined work is not `blocked` — the structured status, the hook
/// signal and the screen are all already in it, so a narrow terminal
/// whose picker the classifier cannot read still counts. A prompt no
/// channel can name reads `permission prompt`.
pub(crate) fn hollywood_prompt(runner: &str, screen: &str, work: Work) -> Option<String> {
    if work != Work::Blocked {
        return None;
    }
    runners::prompt_detail(runner, screen)
        .or_else(|| {
            crate::permit::prompt_block(runner, screen)
                .and_then(|block| block.lines().next().map(|l| l.trim().to_string()))
                .filter(|l| !l.is_empty())
        })
        .or_else(|| Some("permission prompt".to_string()))
}

/// Combine Hollywood's hook `signals.jsonl` state, the terminal's
/// structured state and the runner's screen classification.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn hsignal_work(
    hdir: &Path,
    session_id: Option<&str>,
    term: &LiveTerminal,
    runner: &str,
    text: &str,
    now_ms: u64,
) -> backlot_model::summary::Work {
    let (signal, signal_state) = session_id
        .map(|sid| {
            let tail = crate::signals::tail(&crate::signals::signals_path(hdir, HOLLYWOOD));
            (
                crate::signals::last_work(&tail, sid),
                crate::signals::state(&tail, sid, &std::collections::BTreeSet::new()),
            )
        })
        .unwrap_or((None, None));
    crate::signals::combine(&crate::signals::WorkInputs {
        runner,
        signal: signal_state,
        signal_ts_ms: signal.as_ref().map(|s| s.ts_ms),
        interrupted: runners::work::interrupted(runner, text),
        structured: structured_state(term),
        screen: runners::work_state(runner, text),
        now_ms,
    })
}
