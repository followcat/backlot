//! Probation verification loop for a launched candidate Hollywood terminal
//! (`Verifying` <-> `NeedsApproval`, strong ledger proof, clean `Working -> Waiting`
//! fallback, quota/auth/error rollback, and approval forwarding) (§2.3.4).

use std::collections::BTreeSet;
use std::path::Path;

use backlot_model::summary::Work;
use hc_proto::client::CallError;
use hc_proto::terminal::ReadParams;

use crate::act::fatal;
use crate::cli::Fail;
use crate::daemon::Stage;
use crate::handoff::{HandoffPhase, HandoffRecord, save_handoff};
use crate::handoff_commit::{candidate_signal_tail, discover_cand_session};
use crate::handoff_term::{auth_error_detail, matches_candidate_proof, tail_has_hook_activity};
use crate::reconcile::LiveTerminal;
use crate::watch::trust_key;
use crate::{runners, store};

impl Stage {
    pub(crate) async fn verify_handoff_candidate(
        &self,
        hdir: &Path,
        rec: &mut HandoffRecord,
        terminals: &[LiveTerminal],
        finished: &[LiveTerminal],
        now_ms: u64,
    ) -> Result<(), Fail> {
        let cand = terminals.iter().find(|t| {
            rec.candidate_terminal.as_deref() == Some(t.address.as_str())
                || t.labels.get("backlot.handoff_id").map(String::as_str) == Some(rec.id.as_str())
        });
        let Some(cand) = cand else {
            self.rollback_handoff(hdir, rec, "candidate terminal exited", now_ms)
                .await?;
            return Ok(());
        };
        if rec.candidate_session.is_none() {
            rec.candidate_session =
                discover_cand_session(hdir, rec, cand, &self.env_vars, &*self.exec);
        }
        let text = match self
            .client
            .terminal_read(ReadParams {
                terminal: cand.address.clone(),
                lines: 0,
                caller: None,
            })
            .await
        {
            Ok(r) => r.text,
            Err(e @ (CallError::Closed | CallError::Protocol(_))) => return Err(fatal(e)),
            Err(_) => String::new(),
        };
        let sig_tail = candidate_signal_tail(hdir, rec);
        let sig_state = rec
            .candidate_session
            .as_deref()
            .and_then(|sid| crate::signals::state(&sig_tail, sid, &BTreeSet::new()));
        let work = crate::signals::combine(
            sig_state,
            crate::watch::structured_state(cand),
            runners::work_state(&rec.target_runner, &text),
        );
        if tail_has_hook_activity(&sig_tail) {
            rec.saw_hook_activity = true;
        }
        if self.stage_cfg.get().answer_trust
            && let Some(key) = trust_key(&text)
        {
            let _ = self.driver.keys(&self.client, &cand.address, &[key]).await;
            rec.last_tick_ms = now_ms;
            let _ = save_handoff(hdir, rec);
            return Ok(());
        }
        if let Some(reason) = candidate_failure_reason(cand, &text, work, rec, now_ms) {
            self.rollback_handoff(hdir, rec, &reason, now_ms).await?;
            return Ok(());
        }
        if work == Work::Blocked {
            self.handle_candidate_approval(hdir, rec, cand, &text, now_ms)
                .await?;
            return Ok(());
        }
        if rec.phase == HandoffPhase::NeedsApproval {
            rec.phase = HandoffPhase::Verifying;
            rec.approval_detail = None;
        }
        rec.active_elapsed_ms = rec
            .active_elapsed_ms
            .saturating_add(now_ms.saturating_sub(rec.last_tick_ms));
        rec.last_tick_ms = now_ms;
        if work == Work::Working {
            let start = *rec.saw_working_start_ms.get_or_insert(now_ms);
            rec.saw_working_ms = now_ms.saturating_sub(start);
        }
        self.hrefresh(hdir);
        let old_live = rec
            .old_terminal
            .as_deref()
            .is_some_and(|old| terminals.iter().any(|t| t.address == old));
        let strong_proof = store::load_hevents(hdir).ok().is_some_and(|(evs, _)| {
            evs.iter()
                .any(|e| matches_candidate_proof(e, rec, old_live))
        });
        let fallback_proof =
            work == Work::Waiting && (rec.saw_hook_activity || rec.saw_working_ms >= 2_000);
        if strong_proof || fallback_proof {
            rec.phase = HandoffPhase::Committing;
            let _ = save_handoff(hdir, rec);
            self.commit_handoff(hdir, rec, terminals, finished, now_ms)
                .await?;
            return Ok(());
        }
        if rec.active_elapsed_ms >= rec.timeout_ms {
            let secs = rec.timeout_ms / 1000;
            self.rollback_handoff(
                hdir,
                rec,
                &format!(
                    "switch to {}/{} timed out after {secs}s",
                    rec.target_runner, rec.target_model
                ),
                now_ms,
            )
            .await?;
            return Ok(());
        }
        let _ = save_handoff(hdir, rec);
        Ok(())
    }

    async fn handle_candidate_approval(
        &self,
        hdir: &Path,
        rec: &mut HandoffRecord,
        cand: &LiveTerminal,
        text: &str,
        now_ms: u64,
    ) -> Result<(), Fail> {
        rec.phase = HandoffPhase::NeedsApproval;
        rec.approval_detail =
            crate::hwatch::hollywood_prompt(&rec.target_runner, text, Work::Blocked)
                .or_else(|| Some("permission prompt".into()));
        rec.last_tick_ms = now_ms;
        self.hrefresh(hdir);
        let base = *rec
            .approval_base_seq
            .get_or_insert_with(|| self.hstate.borrow().approvals.last().map_or(0, |a| a.seq));
        let next_appr = {
            let appr = self.hstate.borrow().approval_after(base).copied();
            appr.and_then(|appr| {
                let key = crate::permit::approval_keys(&rec.target_runner, appr.decision)?;
                self.hanswered
                    .borrow_mut()
                    .insert(appr.seq)
                    .then_some((appr.seq, key))
            })
        };
        if let Some((seq, key)) = next_appr
            && self
                .driver
                .keys(&self.client, &cand.address, &[key])
                .await
                .is_err()
        {
            self.hanswered.borrow_mut().remove(&seq);
        }
        let _ = save_handoff(hdir, rec);
        Ok(())
    }
}

fn candidate_failure_reason(
    cand: &LiveTerminal,
    text: &str,
    work: Work,
    rec: &HandoffRecord,
    now_ms: u64,
) -> Option<String> {
    if crate::ratelimit::limited(cand, text, work, now_ms).is_some()
        || crate::ratelimit::weekly_choice_dialog(text)
    {
        return Some(format!(
            "candidate {}/{} hit quota or rate limit",
            rec.target_runner, rec.target_model
        ));
    }
    if let Some(auth) = auth_error_detail(cand, text, work) {
        return Some(format!(
            "candidate {}/{} authentication failed: {auth}",
            rec.target_runner, rec.target_model
        ));
    }
    if crate::connerr::matches(cand, text, work) || crate::ratelimit::errored(cand) {
        return Some(format!(
            "candidate {}/{} errored at startup",
            rec.target_runner, rec.target_model
        ));
    }
    None
}
