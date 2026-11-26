//! Atomic commit, clean rollback, cursor directory promotion, and shared
//! launch helpers for Hollywood's make-before-break handoff (§2.3.4–§2.3.5).

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use backlot_model::EventBy;
use backlot_model::hollywood::{HOLLYWOOD, HOLLYWOOD_TERMINAL, HollywoodKind, RoleMotion};
use hc_proto::terminal::RunParams;

use crate::cli::{Env, Fail};
use crate::config::HollywoodCfg;
use crate::daemon::Stage;
use crate::discover;
use crate::handoff::{HandoffPhase, HandoffRecord, previous_retention_ms, save_handoff};
use crate::handoff_term::cleanup_candidate_artifacts;
use crate::hstage;
use crate::reconcile::LiveTerminal;
use crate::runners;
use crate::store::{self, HPending};

impl Stage {
    pub(crate) async fn commit_handoff(
        &self,
        hdir: &Path,
        rec: &mut HandoffRecord,
        terminals: &[LiveTerminal],
        finished: &[LiveTerminal],
        now_ms: u64,
    ) -> Result<(), Fail> {
        let existing_cfg = crate::config::hollywood(&self.env_vars).ok().flatten();
        let next_cfg = HollywoodCfg {
            runner: rec.target_runner.clone(),
            model: rec.target_model.clone(),
            effort: rec.target_effort.clone(),
            key: existing_cfg.as_ref().and_then(|c| c.key.clone()),
            default_cast: existing_cfg.and_then(|c| c.default_cast),
        };
        let cli_env = Env {
            vars: self.env_vars.clone(),
        };
        if let Err(e) = crate::hsetup::write_config(&cli_env, &next_cfg) {
            self.rollback_handoff(hdir, rec, &format!("config write: {e:?}"), now_ms)
                .await?;
            return Ok(());
        }
        *self.hcfg.borrow_mut() = Some(next_cfg.clone());

        let cand_addr = rec.candidate_terminal.clone().unwrap_or_default();
        if self.labels_ok.get() && !cand_addr.is_empty() {
            let promote = BTreeMap::from([
                ("backlot.role".to_string(), HOLLYWOOD.to_string()),
                ("backlot.handoff_id".to_string(), String::new()),
            ]);
            if self
                .driver
                .label(&self.client, &cand_addr, &promote)
                .await
                .is_err()
            {
                self.labels_ok.set(false);
            }
        }

        let old_addr = rec.old_terminal.clone().filter(|a| {
            terminals.iter().any(|t| &t.address == a) || finished.iter().any(|t| &t.address == a)
        });
        let mut sweep_old_immediately = !self.labels_ok.get();
        if let Some(old) = old_addr.as_deref() {
            if self.labels_ok.get() {
                let until = now_ms.saturating_add(previous_retention_ms(&self.env_vars));
                let prev_labels = BTreeMap::from([
                    ("backlot.role".to_string(), "hollywood-previous".to_string()),
                    ("backlot.previous_until_ms".to_string(), until.to_string()),
                ]);
                if self
                    .driver
                    .label(&self.client, old, &prev_labels)
                    .await
                    .is_err()
                {
                    self.labels_ok.set(false);
                    sweep_old_immediately = true;
                } else {
                    self.hprevious_until_ms.set(Some(until));
                }
            }
            let _ = self.driver.kill(&self.client, old).await;
            if sweep_old_immediately {
                let _ = self.driver.rm(&self.client, old).await;
            }
        }

        for t in finished {
            if t.labels.get("backlot.role").map(String::as_str) == Some("hollywood-previous")
                && old_addr.as_deref() != Some(t.address.as_str())
            {
                let _ = self.driver.rm(&self.client, &t.address).await;
            }
        }
        self.sweep_hollywood_ticks.set(3);

        // Promote isolated cursor directory when committing a cursor candidate so future resumes find it.
        if let Some(iso_cursor) = rec.candidate_cursor_dir.as_deref() {
            let canonical = hdir.join("roles").join(HOLLYWOOD).join("cursor");
            let iso = Path::new(iso_cursor);
            if iso.is_dir() && iso != canonical {
                let _ = fs::remove_dir_all(&canonical);
                let _ = fs::rename(iso, &canonical);
            }
        }

        let new_actor = hstage::identity(&next_cfg);
        let eff_old = rec
            .from_actor
            .as_ref()
            .map(backlot_model::recast::identity_text)
            .unwrap_or_else(|| "none".into());
        let eff_new = backlot_model::recast::identity_text(&new_actor);
        if old_addr.is_some() || self.hstate.borrow().role.live {
            let _ = hstage::record_role(
                hdir,
                RoleMotion::Closed,
                old_addr,
                rec.from_actor.clone(),
                None,
                Some(&format!("handoff -> {eff_new}")),
            );
        }
        hstage::record_role(
            hdir,
            RoleMotion::Started,
            Some(cand_addr),
            Some(new_actor),
            rec.candidate_session.clone(),
            Some(&format!("handoff from {eff_old}")),
        )?;

        if let Some(card_seq) = rec.card_seq {
            let _ = store::append_hollywood(hdir, |_, _| {
                Ok::<_, ()>(HPending {
                    event: backlot_model::hollywood::NewHollywoodEvent::new(
                        EventBy::role("stage"),
                        HollywoodKind::CardDone {
                            card: card_seq,
                            note: format!("switched to {eff_new}"),
                        },
                    ),
                    files: Vec::new(),
                })
            });
        }
        self.reset_hollywood_delivery_state();
        self.hrefresh(hdir);
        rec.phase = HandoffPhase::Committed;
        rec.finished_ms = Some(now_ms);
        rec.error = None;
        let _ = save_handoff(hdir, rec);
        Ok(())
    }

    pub(crate) async fn rollback_handoff(
        &self,
        hdir: &Path,
        rec: &mut HandoffRecord,
        reason: &str,
        now_ms: u64,
    ) -> Result<(), Fail> {
        if let Some(addr) = rec.candidate_terminal.as_deref() {
            let _ = self.driver.kill(&self.client, addr).await;
            let _ = self.driver.rm(&self.client, addr).await;
        }
        cleanup_candidate_artifacts(rec);
        if let Some(old_actor) = rec.from_actor.as_ref()
            && old_actor.runner == rec.target_runner
            && matches!(old_actor.runner.as_str(), "agy" | "cmd" | "devin" | "cline")
        {
            let cwd = hdir.to_string_lossy().into_owned();
            if let Ok(launch) = runners::plan(
                old_actor,
                &runners::LaunchCtx {
                    terminal: HOLLYWOOD_TERMINAL,
                    cwd: &cwd,
                    prompt: None,
                    resume: None,
                    studio_dir: &cwd,
                    stage_env: &self.env_vars,
                    role: HOLLYWOOD,
                    checks: &[],
                },
            ) && let Some(prep) = launch.prep.as_ref()
            {
                let _ = runners::prepare(prep);
            }
        }
        if let Some(card_seq) = rec.card_seq {
            let _ = store::append_hollywood(hdir, |_, _| {
                Ok::<_, ()>(HPending {
                    event: backlot_model::hollywood::NewHollywoodEvent::new(
                        EventBy::role("stage"),
                        HollywoodKind::CardFailed {
                            card: card_seq,
                            reason: reason.to_string(),
                        },
                    ),
                    files: Vec::new(),
                })
            });
        }
        rec.phase = HandoffPhase::RolledBack;
        rec.error = Some(reason.to_string());
        rec.finished_ms = Some(now_ms);
        let _ = save_handoff(hdir, rec);
        Ok(())
    }

    pub(crate) fn reset_hollywood_delivery_state(&self) {
        let pair = hstage::hkey();
        self.work.borrow_mut().remove(&pair);
        self.work_looked.borrow_mut().remove(&pair);
        self.screens.borrow_mut().remove(&pair);
        self.sent_ms.borrow_mut().remove(&pair);
        self.sending.borrow_mut().remove(&pair);
        *self.hrate_pace.borrow_mut() = None;
        *self.hconn_pace.borrow_mut() = None;
        *self.herror.borrow_mut() = None;
        *self.hprompt.borrow_mut() = None;
        self.hprompt_is_question.set(false);
        *self.hprompt_base.borrow_mut() = None;
        self.hanswered.borrow_mut().clear();
    }
}

pub(crate) fn prep_candidate(
    hdir: &Path,
    exe_dir: &str,
    launch: &runners::Launch,
) -> Result<(), String> {
    if let Some(prep) = &launch.prep {
        runners::prepare(prep).map_err(|e| format!("prep: {e}"))?;
    }
    if let Some(hooks) = &launch.hooks {
        crate::act::prepare_hooks(hooks).map_err(|e| format!("hook files: {e}"))?;
    } else if let Some(sig) = launch.prep.as_ref().and_then(runners::prep_signals_file) {
        let _ = runners::truncate_signals(sig);
    }
    crate::guard::prepare_role(&hdir.join("roles").join(HOLLYWOOD), exe_dir)
        .map_err(|e| format!("guard files: {e}"))?;
    Ok(())
}

pub(crate) fn hstage_run_params(
    name: &str,
    argv: Vec<String>,
    cwd: String,
    env: BTreeMap<String, String>,
    labels: BTreeMap<String, String>,
) -> RunParams {
    crate::hlife::hrun_params_named(argv, cwd, env, labels, name)
}

pub(crate) fn discover_cand_session(
    hdir: &Path,
    rec: &HandoffRecord,
    term: &LiveTerminal,
    env_vars: &BTreeMap<String, String>,
    exec: &discover::Exec,
) -> Option<String> {
    if !runners::post_launch_session(&rec.target_runner) {
        return None;
    }
    let home = env_vars
        .get("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    let role_dir = hdir.join("roles").join(HOLLYWOOD);
    let look = discover::Look {
        home: &home,
        role_dir: &role_dir,
        pid: term.pid,
    };
    let cwd = hdir.to_string_lossy().into_owned();
    discover::find_session(
        &rec.target_runner,
        &term.argv,
        &cwd,
        rec.started_ms,
        &BTreeSet::new(),
        &look,
        exec,
    )
}

pub(crate) fn candidate_signal_tail(hdir: &Path, rec: &HandoffRecord) -> String {
    if let Some(sig) = rec.candidate_signals.as_deref() {
        return crate::signals::tail(Path::new(sig));
    }
    crate::signals::tail(&crate::signals::signals_path(hdir, HOLLYWOOD))
}

pub(crate) fn recent_ledger_excerpts(hdir: &Path, max_lines: usize) -> Vec<String> {
    let Ok((events, _)) = store::load_hevents(hdir) else {
        return Vec::new();
    };
    let start = events.len().saturating_sub(max_lines);
    events[start..]
        .iter()
        .map(|e| {
            format!(
                "seq {} [{}]: {}",
                e.seq,
                e.by.role,
                crate::hrender::hconfirm(e)
            )
        })
        .collect()
}
