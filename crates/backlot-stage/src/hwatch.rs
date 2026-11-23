//! Hollywood's watch pass: the post-launch session discovery and
//! the work sample the summary and the send gate read. No
//! permission, stall or rate pass runs here — Hollywood's prompts
//! are the user's, never the stage's to answer (D17a), and the
//! layer owes no steps.

use std::path::Path;
use std::path::PathBuf;

use backlot_model::hollywood::{HOLLYWOOD, HOLLYWOOD_TERMINAL, RoleMotion};
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
        let Some(term) = terminals.iter().find(|t| t.name == HOLLYWOOD_TERMINAL) else {
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
        let Some(t) = terminals.iter().find(|t| t.name == HOLLYWOOD_TERMINAL) else {
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
        let session = self.hstate.borrow().role.session_id.clone();
        let work = hsignal_work(hdir, session.as_deref(), t, runner, &text);
        self.work.borrow_mut().insert(pair.clone(), work);
        *self.hprompt.borrow_mut() = hollywood_prompt(runner, &text);
        let prev = self.screens.borrow().get(&pair).cloned();
        self.screens.borrow_mut().insert(
            pair,
            crate::deliver::Screen::note(prev.as_ref(), text, now_ms),
        );
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
/// question. `None` when the screen is not at a prompt.
pub(crate) fn hollywood_prompt(runner: &str, screen: &str) -> Option<String> {
    use backlot_model::summary::Work;
    if crate::runners::work_state(runner, screen) != Work::Blocked {
        return None;
    }
    let lines: Vec<&str> = screen.lines().collect();
    // cmd's picker (S12a): the `needs to …` sentence, then the command
    // or path on the line under it.
    if let Some(i) = lines.iter().position(|l| {
        l.contains("needs to execute")
            || l.contains("needs to run")
            || l.contains("make this edit to")
    }) {
        let next = lines[i + 1..].iter().map(|l| l.trim()).find(|l| {
            !l.is_empty() && !l.starts_with('❯') && !l.starts_with("2.") && !l.starts_with("3.")
        });
        return Some(next.unwrap_or_else(|| lines[i].trim()).to_string());
    }
    // The other runners' question line, when the picker has one.
    if let Some(q) = lines.iter().find(|l| {
        l.contains("Do you want to")
            || l.contains("Would you like to")
            || l.contains("Approve tool call")
    }) {
        return Some(q.trim().to_string());
    }
    // No question the stage can name: its saved prompt block's first
    // line, else the bare word.
    crate::permit::prompt_block(runner, screen)
        .and_then(|block| block.lines().next().map(|l| l.trim().to_string()))
        .filter(|l| !l.is_empty())
        .or_else(|| Some("permission prompt".to_string()))
}

/// Combine Hollywood's hook `signals.jsonl` state, the terminal's
/// structured state and the runner's screen classification.
pub(crate) fn hsignal_work(
    hdir: &Path,
    session_id: Option<&str>,
    term: &LiveTerminal,
    runner: &str,
    text: &str,
) -> backlot_model::summary::Work {
    let signal_state = session_id.and_then(|sid| {
        let tail = crate::signals::tail(&crate::signals::signals_path(hdir, HOLLYWOOD));
        crate::signals::state(&tail, sid, &std::collections::BTreeSet::new())
    });
    crate::signals::combine(
        signal_state,
        structured_state(term),
        runners::work_state(runner, text),
    )
}
