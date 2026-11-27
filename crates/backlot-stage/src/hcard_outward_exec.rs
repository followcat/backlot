//! Non-blocking outward command execution (`CardAction::Outward`),
//! timeout escalation, cancellation reconciliation, and completion recording
//! (push-perms spec §2.3).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use backlot_model::event::OutwardOutcome;
use backlot_model::hollywood::{Card, CardAction, CardState, HollywoodKind};
use backlot_model::state::StudioState;

use crate::checkrun::{CheckChild, PollOut};
use crate::cli::Fail;
use crate::daemon::Stage;
use crate::guard_git::GitContext;
use crate::guard_git_env::{normalize_resolve, path_inside};
use crate::guard_prep::{find_real_binary, studio_role_dir};
use crate::guard_rules::{GuardVerdict, classify_with_context};
use crate::hcard::Step;
use crate::hcard_outward::outward_stale;
use crate::hcard_outward_env::{
    cap_tail, finish_reason, person_exec_env, record_outward_completed, stage_probe_env,
    write_outward_log,
};
use crate::hcard_outward_git::verify_local_git_clean;
use crate::store;

pub(crate) const OUTWARD_TIMEOUT_MS: u64 = 120_000;
pub(crate) const OUTWARD_KILL_AFTER_MS: u64 = 5_000;

/// One in-flight outward command spawned by `Stage`.
pub(crate) struct OutwardRunning {
    pub card: u64,
    pub studio: String,
    pub source_seq: u64,
    pub argv: Vec<String>,
    pub sdir: PathBuf,
    pub child: Box<dyn CheckChild>,
    pub timeout_ms: u64,
    pub term_ms: Option<u64>,
    pub killed: bool,
}

enum PrepErr {
    Stale(String),
    Fail(String),
}

struct PreparedExec {
    bin: PathBuf,
    args: Vec<String>,
    cwd: PathBuf,
    env: Vec<(String, String)>,
    source_seq: u64,
    argv: Vec<String>,
    studio: String,
}

fn prepare_outward_exec(
    base: &Path,
    stage_env: &BTreeMap<String, String>,
    sdir: &Path,
    card: &Card,
    s: &StudioState,
) -> Result<PreparedExec, PrepErr> {
    let CardAction::Outward {
        studio,
        role,
        source_seq,
        argv,
        cwd: cwd_str,
        ..
    } = &card.action
    else {
        return Err(PrepErr::Fail("not an outward card".into()));
    };
    if argv.is_empty() {
        return Err(PrepErr::Fail("empty outward command".into()));
    }
    let bin_name = match Path::new(&argv[0]).file_name().and_then(|n| n.to_str()) {
        Some("git") => "git",
        Some("gh") => "gh",
        _ => {
            return Err(PrepErr::Fail(format!(
                "disallowed outward command '{}'",
                argv[0]
            )));
        }
    };
    let cfg = s
        .studio
        .as_ref()
        .ok_or_else(|| PrepErr::Fail("no studio config".into()))?;
    let work_area = crate::workareas::role_dir(sdir, &cfg.repo, role, cfg.no_git);
    let cwd = Path::new(cwd_str);
    if !cwd.is_dir() {
        return Err(PrepErr::Fail(format!(
            "outward cwd does not exist: {}",
            cwd.display()
        )));
    }
    let norm_work = normalize_resolve(cwd, &work_area);
    let norm_cwd = normalize_resolve(cwd, cwd);
    if !path_inside(&norm_cwd, std::slice::from_ref(&norm_work)) {
        return Err(PrepErr::Fail("outward cwd outside role work area".into()));
    }
    let clean_probe = stage_probe_env();
    verify_local_git_clean(cwd, &clean_probe).map_err(PrepErr::Fail)?;

    let ctx = GitContext {
        work_area: Some(&work_area),
        repo_root: Some(Path::new(&cfg.repo)),
        members: &cfg.members,
        ..GitContext::bare(&cfg.push, cwd, &clean_probe)
    };
    let matched = classify_with_context(bin_name, &argv[1..], "", &ctx);
    match &matched.verdict {
        GuardVerdict::Card { .. } => {}
        GuardVerdict::Allow => {
            return Err(PrepErr::Fail(
                "outward command no longer needs a card".into(),
            ));
        }
        GuardVerdict::Refuse { rule, .. } => {
            return Err(PrepErr::Fail(format!(
                "outward command refused by policy ({rule})"
            )));
        }
    }
    let norm_eff = normalize_resolve(cwd, &matched.cwd);
    if !path_inside(&norm_eff, std::slice::from_ref(&norm_work)) {
        return Err(PrepErr::Fail(
            "outward target directory outside role work area".into(),
        ));
    }
    if matched.cwd != cwd {
        verify_local_git_clean(&matched.cwd, &clean_probe).map_err(PrepErr::Fail)?;
    }
    if let Some(reason) = backlot_model::hollywood::stale(&card.action, &card.base, Some(s)) {
        return Err(PrepErr::Stale(reason));
    }
    if let Some(reason) = outward_stale(&card.action, &card.base, Some(s)) {
        return Err(PrepErr::Stale(reason));
    }

    let role_bin = studio_role_dir(&base.to_string_lossy(), studio, role).join("bin");
    let clean_env = person_exec_env(stage_env, Some(&role_bin));
    let clean_path = clean_env
        .iter()
        .find(|(k, _)| k == "PATH")
        .map(|(_, v)| v.as_str());
    let real_bin = find_real_binary(bin_name, clean_path, Some(&role_bin))
        .ok_or_else(|| PrepErr::Fail(format!("real '{bin_name}' binary not found on PATH")))?;

    Ok(PreparedExec {
        bin: real_bin,
        args: argv[1..].to_vec(),
        cwd: cwd.to_path_buf(),
        env: clean_env,
        source_seq: *source_seq,
        argv: argv.clone(),
        studio: studio.clone(),
    })
}

impl Stage {
    /// Validate and spawn one applied `CardAction::Outward` asynchronously.
    pub(crate) fn start_outward_card(
        &self,
        sdir: &Path,
        card: &Card,
        s: &StudioState,
        now_ms: u64,
    ) -> Step {
        if self.outward_running.borrow().contains_key(&card.seq) {
            return Step::InFlight;
        }
        if let Some(prev) = s
            .outward_completions
            .iter()
            .rev()
            .find(|c| c.card == card.seq)
        {
            return match prev.outcome {
                OutwardOutcome::Succeeded => Step::Done,
                OutwardOutcome::Stale => Step::Stale(prev.output_tail.clone()),
                OutwardOutcome::Failed | OutwardOutcome::TimedOut | OutwardOutcome::Denied => {
                    Step::Fail(prev.output_tail.clone())
                }
            };
        }
        let source_seq = match &card.action {
            CardAction::Outward { source_seq, .. } => *source_seq,
            _ => return Step::Fail("not an outward card".into()),
        };
        let prep = match prepare_outward_exec(&self.base, &self.env_vars, sdir, card, s) {
            Ok(p) => p,
            Err(PrepErr::Stale(reason)) => {
                record_outward_completed(
                    sdir,
                    card.seq,
                    source_seq,
                    OutwardOutcome::Stale,
                    None,
                    reason.clone(),
                    None,
                );
                return Step::Stale(reason);
            }
            Err(PrepErr::Fail(reason)) => {
                record_outward_completed(
                    sdir,
                    card.seq,
                    source_seq,
                    OutwardOutcome::Failed,
                    None,
                    reason.clone(),
                    None,
                );
                return Step::Fail(reason);
            }
        };
        match (self.spawn_outward)(&prep.bin, &prep.args, &prep.cwd, &prep.env) {
            Ok(child) => {
                self.outward_running.borrow_mut().insert(
                    card.seq,
                    OutwardRunning {
                        card: card.seq,
                        studio: prep.studio,
                        source_seq: prep.source_seq,
                        argv: prep.argv,
                        sdir: sdir.to_path_buf(),
                        child,
                        timeout_ms: now_ms.saturating_add(OUTWARD_TIMEOUT_MS),
                        term_ms: None,
                        killed: false,
                    },
                );
                Step::InFlight
            }
            Err(reason) => {
                record_outward_completed(
                    sdir,
                    card.seq,
                    source_seq,
                    OutwardOutcome::Failed,
                    None,
                    reason.clone(),
                    None,
                );
                Step::Fail(reason)
            }
        }
    }

    /// Non-blocking poll across all in-flight outward child processes;
    /// records `outward.completed`, writes `outward/c<n>.log`, and updates
    /// Hollywood's card state once each finishes.
    pub(crate) fn poll_outward_running(&self, hdir: &Path, now_ms: u64) -> Result<(), Fail> {
        let current = std::mem::take(&mut *self.outward_running.borrow_mut());
        let mut still = BTreeMap::new();
        let mut finished: Vec<(OutwardRunning, PollOut)> = Vec::new();
        for (card_id, mut run) in current {
            if let Some(out) = run.child.poll() {
                finished.push((run, out));
                continue;
            }
            if now_ms >= run.timeout_ms {
                match run.term_ms {
                    None => {
                        run.child.term();
                        run.term_ms = Some(now_ms);
                    }
                    Some(t) if now_ms >= t.saturating_add(OUTWARD_KILL_AFTER_MS) && !run.killed => {
                        run.child.kill();
                        run.killed = true;
                    }
                    _ => {}
                }
            }
            still.insert(card_id, run);
        }
        *self.outward_running.borrow_mut() = still;

        for (run, out) in finished {
            let outcome = if run.term_ms.is_some() {
                OutwardOutcome::TimedOut
            } else if out.code == Some(0) {
                OutwardOutcome::Succeeded
            } else {
                OutwardOutcome::Failed
            };
            let log_file = write_outward_log(&run.sdir, run.card, &out.output);
            let tail = cap_tail(&out.output);
            record_outward_completed(
                &run.sdir,
                run.card,
                run.source_seq,
                outcome,
                out.code,
                tail.clone(),
                log_file,
            );
            let kind = match outcome {
                OutwardOutcome::Succeeded => HollywoodKind::CardDone {
                    card: run.card,
                    note: format!("{} (exit 0)", run.argv.join(" ")),
                },
                _ => HollywoodKind::CardFailed {
                    card: run.card,
                    reason: finish_reason(outcome, out.code, &tail),
                },
            };
            self.happend(hdir, kind)?;
            self.hrefresh(hdir);
        }
        Ok(())
    }

    /// Ensure every cancelled `CardAction::Outward` card records one
    /// `OutwardCompleted { outcome: Denied }` event in its studio ledger.
    pub(crate) fn reconcile_cancelled_outward(&self) {
        let cancelled: Vec<(u64, String, u64, String)> = self
            .hstate
            .borrow()
            .cards
            .values()
            .filter(|c| c.state == CardState::Cancelled)
            .filter_map(|c| match &c.action {
                CardAction::Outward {
                    studio, source_seq, ..
                } => Some((c.seq, studio.clone(), *source_seq, c.detail.clone())),
                _ => None,
            })
            .collect();
        for (card_id, studio, source_seq, detail) in cancelled {
            if !crate::cli::valid_studio_name(&studio) {
                continue;
            }
            let sdir = store::studio_dir(&self.base, &studio);
            if !sdir.join("events.jsonl").is_file() {
                continue;
            }
            let Ok(sstate) = store::load_state(&sdir) else {
                continue;
            };
            if sstate.finished.is_some() || sstate.has_outward_completion(card_id) {
                continue;
            }
            if let Some(mut run) = self.outward_running.borrow_mut().remove(&card_id) {
                run.child.kill();
            }
            let tail = if detail.trim().is_empty() {
                "cancelled by user".into()
            } else {
                detail
            };
            record_outward_completed(
                &sdir,
                card_id,
                source_seq,
                OutwardOutcome::Denied,
                None,
                tail,
                None,
            );
        }
    }
}
