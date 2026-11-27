//! Stage state machine orchestration for make-before-break Hollywood agent
//! switches (`Preparing` -> `Starting` -> `Verifying` <-> `NeedsApproval`) (§2.3.4).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use backlot_model::event::ActorIdentity;
use backlot_model::hollywood::HOLLYWOOD;
use hc_proto::client::CallError;
use hc_proto::terminal::ReadParams;

use crate::act::fatal;
use crate::cli::Fail;
use crate::config::HollywoodCfg;
use crate::daemon::Stage;
use crate::handoff::{HandoffPhase, HandoffRecord, load_handoff, save_handoff};
use crate::handoff_commit::{hstage_run_params, prep_candidate, recent_ledger_excerpts};
use crate::handoff_pkg::{
    HandoffBriefInput, build_handoff_brief, candidate_first_prompt, write_handoff_brief,
};
use crate::handoff_term::{
    active_hollywood_terminal, apply_handoff_isolation, labels_for_candidate, next_candidate_name,
};
use crate::hstage;
use crate::live::LiveView;
use crate::reconcile::LiveTerminal;
use crate::runners::{self, StartDecision};
use crate::transcript::{TranscriptTarget, extract_runner_tail, format_fallback_tail};

impl Stage {
    /// Advance any active or newly staged Hollywood switch handoff.
    pub(crate) async fn drive_handoff(
        &self,
        hdir: &Path,
        cfg: Option<&HollywoodCfg>,
        terminals: &[LiveTerminal],
        finished: &[LiveTerminal],
        real_slot_free: &mut bool,
        now_ms: u64,
    ) -> Result<(), Fail> {
        let mut rec = match load_handoff(hdir) {
            Ok(Some(r)) => r,
            Ok(None) => {
                *self.hlast_handoff.borrow_mut() = None;
                return Ok(());
            }
            Err(e) => {
                self.hlog(&format!("handoff state error: {e}"));
                return Ok(());
            }
        };
        *self.hlast_handoff.borrow_mut() = rec.visible_in_summary(now_ms).then(|| rec.to_summary());
        if !rec.phase.is_active() {
            return Ok(());
        }
        if rec.cancel_requested && rec.phase != HandoffPhase::Committing {
            self.rollback_handoff(hdir, &mut rec, "cancelled by user", now_ms)
                .await?;
            *self.hlast_handoff.borrow_mut() =
                rec.visible_in_summary(now_ms).then(|| rec.to_summary());
            return Ok(());
        }
        match rec.phase {
            HandoffPhase::Preparing => {
                self.prepare_handoff(hdir, &mut rec, cfg, terminals, now_ms)
                    .await?;
                if rec.phase == HandoffPhase::Starting {
                    self.start_handoff_candidate(
                        hdir,
                        &mut rec,
                        terminals,
                        finished,
                        real_slot_free,
                        now_ms,
                    )
                    .await?;
                }
            }
            HandoffPhase::Starting => {
                if rec.candidate_terminal.is_some() {
                    self.rollback_handoff(
                        hdir,
                        &mut rec,
                        "stage restarted during candidate launch",
                        now_ms,
                    )
                    .await?;
                } else {
                    self.start_handoff_candidate(
                        hdir,
                        &mut rec,
                        terminals,
                        finished,
                        real_slot_free,
                        now_ms,
                    )
                    .await?;
                }
            }
            HandoffPhase::Verifying | HandoffPhase::NeedsApproval => {
                self.verify_handoff_candidate(hdir, &mut rec, terminals, finished, now_ms)
                    .await?;
            }
            HandoffPhase::Committing => {
                self.commit_handoff(hdir, &mut rec, terminals, finished, now_ms)
                    .await?;
            }
            HandoffPhase::Committed | HandoffPhase::RolledBack => {}
        }
        *self.hlast_handoff.borrow_mut() = rec.visible_in_summary(now_ms).then(|| rec.to_summary());
        Ok(())
    }

    async fn prepare_handoff(
        &self,
        hdir: &Path,
        rec: &mut HandoffRecord,
        cfg: Option<&HollywoodCfg>,
        terminals: &[LiveTerminal],
        now_ms: u64,
    ) -> Result<(), Fail> {
        let (old_term_addr, old_term_ref, old_actor, old_sid) = {
            let hs = self.hstate.borrow();
            let old_term = active_hollywood_terminal(&hs.role, terminals);
            let old_actor = hs.role.actor.clone().or_else(|| cfg.map(hstage::identity));
            (
                old_term.map(|t| t.address.clone()),
                old_term,
                old_actor,
                hs.role.session_id.clone(),
            )
        };
        rec.old_terminal = old_term_addr;
        rec.from_actor = old_actor.clone();
        let from_str = old_actor
            .as_ref()
            .map(backlot_model::recast::identity_text)
            .unwrap_or_else(|| "none".to_string());
        let to_actor = ActorIdentity {
            runner: rec.target_runner.clone(),
            runner_version: None,
            model: rec.target_model.clone(),
            effort: rec.target_effort.clone(),
            permission_profile: "standard".into(),
        };
        let to_str = backlot_model::recast::identity_text(&to_actor);
        let tail = self
            .extract_old_tail(hdir, old_term_ref, old_actor.as_ref(), old_sid.as_deref())
            .await;
        let live_view = LiveView {
            work: self.work.borrow().clone(),
            running_cmds: BTreeMap::new(),
            idle: self.idle_since.borrow().clone(),
            accounts: Vec::new(),
            running: now_ms > 0,
        };
        let brief = {
            let hs = self.hstate.borrow();
            build_handoff_brief(&HandoffBriefInput {
                id: &rec.id,
                hdir,
                now_ms,
                from_actor: &from_str,
                to_actor: &to_str,
                trigger: &rec.trigger,
                studios: &self.cached,
                live: &live_view,
                hstate: &hs,
                tail: &tail,
            })
        };
        let path = match write_handoff_brief(hdir, &rec.id, &brief) {
            Ok(p) => p,
            Err(e) => {
                self.rollback_handoff(hdir, rec, &format!("write brief: {e}"), now_ms)
                    .await?;
                return Ok(());
            }
        };
        rec.brief_path = Some(path.to_string_lossy().into_owned());
        rec.phase = HandoffPhase::Starting;
        let _ = save_handoff(hdir, rec);
        Ok(())
    }

    async fn extract_old_tail(
        &self,
        hdir: &Path,
        old_term: Option<&LiveTerminal>,
        old_actor: Option<&ActorIdentity>,
        session_id: Option<&str>,
    ) -> crate::transcript::ExtractedTail {
        let cwd = hdir.to_string_lossy().into_owned();
        let home = self.env_vars.get("HOME").map(std::path::PathBuf::from);
        let role_dir = hdir.join("roles").join(HOLLYWOOD);
        if let Some(actor) = old_actor {
            let target = TranscriptTarget {
                cwd: &cwd,
                runner: &actor.runner,
                session_id,
                role_dir: &role_dir,
                home: home.as_deref(),
            };
            if let Some(extracted) = extract_runner_tail(&target, &*self.exec) {
                return extracted;
            }
        }
        let scroll = match old_term {
            Some(t) => self
                .client
                .terminal_read(ReadParams {
                    terminal: t.address.clone(),
                    lines: 200,
                    caller: None,
                })
                .await
                .ok()
                .map(|r| r.text),
            None => None,
        };
        let ledger_lines = recent_ledger_excerpts(hdir, 20);
        format_fallback_tail(scroll.as_deref(), &ledger_lines)
    }

    async fn start_handoff_candidate(
        &self,
        hdir: &Path,
        rec: &mut HandoffRecord,
        terminals: &[LiveTerminal],
        finished: &[LiveTerminal],
        real_slot_free: &mut bool,
        now_ms: u64,
    ) -> Result<(), Fail> {
        let mut used: BTreeSet<String> = terminals
            .iter()
            .chain(finished.iter())
            .map(|t| t.name.clone())
            .collect();
        if let Some(old) = self.hstate.borrow().role.terminal.as_deref() {
            used.insert(old.to_string());
        }
        let cand_name = next_candidate_name(&used, rec.generation);
        let (old_r, old_m) = rec
            .from_actor
            .as_ref()
            .map(|a| (a.runner.as_str(), a.model.as_str()))
            .unwrap_or(("none", "none"));
        let prompt_body = candidate_first_prompt(
            hdir,
            &rec.id,
            old_r,
            old_m,
            &rec.target_runner,
            &rec.target_model,
        );
        let cwd = hdir.to_string_lossy().into_owned();
        let prompt_arg = if rec.target_runner == "devin" {
            let pdir = hdir.join("prompts");
            let _ = fs::create_dir_all(&pdir);
            let pfile = pdir.join(format!("handoff-{}.first.txt", rec.id));
            if let Err(e) = fs::write(&pfile, &prompt_body) {
                self.rollback_handoff(hdir, rec, &format!("prompt file: {e}"), now_ms)
                    .await?;
                return Ok(());
            }
            pfile.to_string_lossy().into_owned()
        } else {
            prompt_body
        };
        let mut cand_actor = ActorIdentity {
            runner: rec.target_runner.clone(),
            runner_version: None,
            model: rec.target_model.clone(),
            effort: rec.target_effort.clone(),
            permission_profile: "standard".into(),
        };
        let planned = runners::plan(
            &cand_actor,
            &runners::LaunchCtx {
                terminal: &cand_name,
                cwd: &cwd,
                prompt: Some(&prompt_arg),
                resume: None,
                studio_dir: &cwd,
                stage_env: &self.env_vars,
                role: HOLLYWOOD,
                checks: &[],
                answer_trust: self.stage_cfg.get().answer_trust,
            },
        );
        match runners::decide_start(
            planned,
            &cand_actor.runner,
            self.runner_env(&cand_actor.runner),
            real_slot_free,
            &BTreeSet::new(),
            &cwd,
        ) {
            StartDecision::Attention(e) => {
                self.rollback_handoff(hdir, rec, &format!("{}: {}", e.reason, e.detail), now_ms)
                    .await?;
                return Ok(());
            }
            StartDecision::Defer => return Ok(()),
            StartDecision::Launch {
                mut launch,
                runner_env,
            } => {
                cand_actor.runner_version =
                    launch.argv.first().and_then(|p| self.runner_version(p));
                let (sig_path, cursor_dir) = apply_handoff_isolation(&mut launch, hdir, &rec.id);
                rec.candidate_signals = sig_path.map(|p| p.to_string_lossy().into_owned());
                rec.candidate_cursor_dir = cursor_dir.map(|p| p.to_string_lossy().into_owned());
                if let Err(msg) = prep_candidate(hdir, &self.exe_dir, &launch) {
                    self.rollback_handoff(hdir, rec, &msg, now_ms).await?;
                    return Ok(());
                }
                let env = crate::hlife::hrole_env_named(
                    launch.env.clone(),
                    runner_env,
                    &self.state_dir,
                    &self.exe_dir,
                    &self.path_env,
                    &cand_name,
                );
                let mut env = env;
                env.insert("BACKLOT_HANDOFF_ID".into(), rec.id.clone());
                let labels = labels_for_candidate(&rec.id, &cand_name);
                let params = hstage_run_params(&cand_name, launch.argv.clone(), cwd, env, labels);
                match self.driver.run(&self.client, params).await {
                    Ok(res) => {
                        rec.phase = HandoffPhase::Verifying;
                        rec.candidate_name = Some(cand_name);
                        rec.candidate_terminal = Some(res.terminal.address);
                        rec.candidate_session = launch.session_id();
                        rec.started_ms = now_ms;
                        rec.started_seq = crate::store::load_hevents(hdir)
                            .ok()
                            .and_then(|(evs, _)| evs.last().map(|e| e.seq))
                            .unwrap_or(0);
                        rec.last_tick_ms = now_ms;
                        let _ = save_handoff(hdir, rec);
                    }
                    Err(CallError::Rpc(e)) => {
                        *real_slot_free = true;
                        self.rollback_handoff(hdir, rec, &format!("terminal.run: {e}"), now_ms)
                            .await?;
                    }
                    Err(e) => return Err(fatal(e)),
                }
            }
        }
        Ok(())
    }
}
