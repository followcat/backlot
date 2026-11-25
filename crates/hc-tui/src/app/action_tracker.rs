//! The ActionTracker's app side (S3 §3, A7.1): `run_action` is the
//! single door every `backlot` invocation goes through — dock keys,
//! Call Sheet rows, context menus, the native dialogs, `Ctrl+B n`,
//! the sheet's own runs. The command spawns through the `NsRun`
//! runner (local `current_exe`, remote `RemoteRunner` — stdin rides
//! on both), the reply lands on `action_rx`, and the result becomes
//! the 3.5 s success toast or the 8 s failure card whose `copy cmd`
//! writes the exact command through OSC 52.

use std::time::Instant;

use crate::action::{
    Action, DONE_HOLD, FAIL_HOLD, copy_line, done_title, fail_body, fail_title, mark,
    mutates_ledger,
};

use super::App;

/// The action toast's state — a success card or the failure card
/// carrying the stderr line and the copyable command.
pub(crate) struct ActionToast {
    /// `paused task t1` / `host-b: paused task t1`, or `failed: …`
    /// / `host-b failed: …` for a failure.
    pub(crate) title: String,
    /// The failure's first stderr line — empty on success.
    pub(crate) body: String,
    /// A failure holds 8 s and offers `copy cmd`.
    pub(crate) fail: bool,
    /// The exact command line `C` / `copy cmd` writes — failures only.
    pub(crate) command: Option<String>,
    /// `Some(machine)` when the success toast offers `[s] start
    /// stage` — a mutation that completed while the machine's stage
    /// is stopped (`None` inside names this core).
    pub(crate) start_stage: Option<Option<String>>,
    pub(crate) deadline: Instant,
}

/// What the key dispatcher asks while the failure card is up.
pub(crate) fn command_of(toast: &Option<ActionToast>) -> Option<&str> {
    toast
        .as_ref()
        .filter(|t| t.fail)
        .and_then(|t| t.command.as_deref())
}

impl App {
    /// The Call Sheet's `…` row marks read one joined string — the
    /// tracker's marks, re-synced on every start and reply.
    fn sync_pending(&mut self) {
        self.hollywood.pending = (!self.actions.pending.is_empty()).then(|| self.actions.marks());
        if !self.plain && self.has_hollywood_cfg {
            let machines = self.panel_summaries();
            self.studio.lot.counts.activity =
                crate::hollywood::hollywood_activity(&machines, self.hollywood.pending.as_deref());
        }
    }

    /// The single door (§3): a duplicate press on the same target
    /// while its action runs is ignored — the row's own mark already
    /// spins, so the press does nothing. A hostile argv — a control
    /// byte in any word or the machine name — never spawns (D17a);
    /// the refusal lands through the same reply path a failed run
    /// would take.
    pub(crate) fn run_action(&mut self, action: Action) {
        // S1: remember where the keys were before the stage's terminals
        // publish — a stage start keeps the focus, and an opening-page
        // start lands on a studio or the lot, never the stage log.
        if action.name == "start stage" {
            self.note_stage_start();
        }
        let hostile = action
            .argv
            .iter()
            .chain(action.machine.iter())
            .find(|w| crate::action::unsafe_word(w))
            .cloned();
        let Some(id) = self.actions.start(action.clone()) else {
            return;
        };
        self.last_dispatched_action_id = Some(id);
        if let Some(bad) = hostile {
            self.sync_pending();
            self.sched.mark_dirty(Instant::now());
            self.on_action_reply((id, Err(format!("refused unsafe argument: {bad:?}"))));
            return;
        }
        self.sync_pending();
        // The runner is invoked eagerly — the tests' argv recorders
        // count the call itself, and the spawn only rides the reply.
        let fut = self.ns_runner()(
            action.machine.clone(),
            action.argv.clone(),
            action.stdin.clone(),
        );
        let tx = self.action_tx.clone();
        tokio::spawn(async move {
            let result = fut.await;
            let _ = tx.send((id, result)).await;
        });
        self.sched.mark_dirty(Instant::now());
    }

    /// The reply's toast: `view` actions open the card read instead,
    /// and a card apply/cancel answered from inside its view retires
    /// it — the Call Sheet's own rule, kept (S1 §5).
    pub(crate) fn on_action_reply(&mut self, (id, result): (u64, Result<String, String>)) {
        let now = Instant::now();
        let Some(pending) = self.actions.finish(id) else {
            return; // A reply for a retired action mutates nothing.
        };
        self.sync_pending();
        self.sched.mark_dirty(now);
        // U6c: a restart's `stage stop` chains `stage start` on `Ok`
        // before the toast draws.
        self.machine_stage_restart_chain(&pending.action, result.is_ok());
        if self.delete_studio_reply(&pending, &result) {
            return;
        }
        // The setup dialog's write: `Ok` flips the bar to its agent
        // form; either way the remembered cfg retires.
        if pending.action.mark == mark::hollywood_setup() {
            self.hw_setup_reply(result.is_ok());
        }
        if !pending.action.view
            && pending.action.mark != mark::hollywood_setup()
            && self.reader_compose_on_reply(id, &result)
        {
            return;
        }
        match result {
            Ok(stdout) if pending.action.view => {
                self.on_view_action_reply(&pending.action, &stdout);
            }
            Ok(_) => {
                let resolved = self
                    .hollywood
                    .view
                    .as_ref()
                    .and_then(|v| v.card.as_deref())
                    .is_some_and(|card| pending.action.mark == mark::card(card));
                if resolved {
                    self.hollywood.view = None;
                }
                // A mutation that landed while its machine's stage is
                // down is on disk but unseen — the toast stays up for
                // the actionable hold with `[s] start stage` (R3 §1b).
                let stopped = self.can_start_stage(pending.action.machine.as_deref())
                    && mutates_ledger(&pending.action)
                    && !self.machine_stage_running(pending.action.machine.as_deref());
                let (body, hold) = if stopped {
                    (
                        match &pending.action.machine {
                            Some(m) => format!("recorded on disk · stage stopped on {m}"),
                            None => "recorded on disk · stage stopped".to_owned(),
                        },
                        FAIL_HOLD,
                    )
                } else {
                    (String::new(), DONE_HOLD)
                };
                self.action_toast = Some(ActionToast {
                    title: done_title(&pending),
                    body,
                    fail: false,
                    command: None,
                    start_stage: stopped.then(|| pending.action.machine.clone()),
                    deadline: now + hold,
                });
            }
            Err(err) => {
                if pending.action.view {
                    self.reader_clear_loading();
                }
                self.action_toast = Some(ActionToast {
                    title: fail_title(&pending),
                    body: fail_body(&err),
                    fail: true,
                    command: Some(copy_line(&pending)),
                    start_stage: None,
                    deadline: now + FAIL_HOLD,
                });
            }
        }
    }

    /// `true` when the toast's deadline passed — the card un-paints.
    pub(crate) fn expire_action_toast(&mut self, now: Instant) -> bool {
        if self
            .action_toast
            .as_ref()
            .is_some_and(|t| now >= t.deadline)
        {
            self.action_toast = None;
            self.action_chip_hit = Default::default();
            self.action_toast_hit = Default::default();
            return true;
        }
        false
    }

    /// `C`/`Esc` while the card is up (§3.3): `C` copies the exact
    /// failed command on a failure toast, `Esc` dismisses either
    /// kind. Anything else reaches the pane as usual — the toast is
    /// a card, not a modal.
    pub(crate) fn action_toast_key(&mut self, key: &super::input::TerminalKey) -> bool {
        let Some(toast) = &self.action_toast else {
            return false;
        };
        let combo = crate::input::terminal_key_of(key);
        use crossterm::event::KeyCode;
        match combo.code {
            KeyCode::Esc => {
                self.action_toast = None;
                true
            }
            KeyCode::Char('C') if toast.fail => self.action_copy(),
            _ => false,
        }
    }

    /// The stopped-stage toast's `s` ahead of the surfaces (R3 §1b):
    /// a mutation that completed under an open Dock or Call Sheet
    /// still answers the offer, while a surface that would read `s`
    /// as text or an answer — the tell line, an armed `y/n`, the
    /// card view — keeps the key. `Esc`/`C` keep their usual place.
    pub(crate) fn action_toast_start_key(&mut self, key: &super::input::TerminalKey) -> bool {
        if self
            .action_toast
            .as_ref()
            .is_none_or(|t| t.start_stage.is_none())
        {
            return false;
        }
        if self.hollywood.editing
            || self.hollywood.confirm.is_some()
            || self.hollywood.view.is_some()
            || self.panel_editing()
            || self.panel.finish_confirm.is_some()
        {
            return false;
        }
        let combo = crate::input::terminal_key_of(key);
        let (code, modifiers) = crate::config::normalize_key_combo((combo.code, combo.modifiers));
        code == crossterm::event::KeyCode::Char('s')
            && modifiers.is_empty()
            && key.input.action == hc_vt::Action::Press
            && self.action_toast_start()
    }

    /// `[s] start stage` on the stopped-stage toast — the chip click
    /// and both key paths share this: `backlot stage start` on the
    /// recorded machine, then the card dismisses.
    pub(crate) fn action_toast_start(&mut self) -> bool {
        let Some(machine) = self
            .action_toast
            .as_ref()
            .and_then(|t| t.start_stage.clone())
        else {
            return false;
        };
        if !self.can_start_stage(machine.as_deref()) {
            self.action_toast = None;
            return false;
        }
        self.run_action(crate::studio_cmds::stage_start(machine));
        self.action_toast = None;
        true
    }

    /// `C` while the failure card is up — the exact command lands on
    /// the host clipboard through OSC 52 and the `copied command`
    /// flash answers. `no modal owns input` is the caller's gate
    /// (§3.3): it runs only when this press would otherwise go to a
    /// pane.
    pub(crate) fn action_copy(&mut self) -> bool {
        let Some(command) = command_of(&self.action_toast).map(str::to_owned) else {
            return false;
        };
        let sequence = crate::selection::osc52_sequence(command.as_bytes());
        let _ = (self.host_control)(sequence.as_bytes());
        self.action_toast = None;
        self.copy_feedback = Some(super::notices::CopyFeedback {
            message: "copied command to clipboard".to_owned(),
        });
        self.copy_feedback_deadline = Some(Instant::now() + std::time::Duration::from_secs(2));
        true
    }

    /// A press inside the toast rect: the chip runs its action —
    /// `copy cmd` on a failure, `start stage` on the stopped-stage
    /// offer — anywhere else on the card dismisses.
    pub(crate) fn action_toast_press(&mut self, point: (u16, u16)) -> bool {
        use ratatui::layout::Position;
        let contains = |rect: ratatui::layout::Rect| {
            !rect.is_empty()
                && rect.contains(Position {
                    x: point.0,
                    y: point.1,
                })
        };
        if contains(self.action_chip_hit) {
            return match self.action_toast.as_ref() {
                Some(toast) if toast.fail => self.action_copy(),
                Some(_) => self.action_toast_start(),
                None => false,
            };
        }
        if contains(self.action_toast_hit) {
            self.action_toast = None;
            return true;
        }
        false
    }

    /// The pre-S3 tests' runner — an argv recorder answering the
    /// scripted output. Both old setters now feed the tracker's
    /// `NsRun`, since one runner serves every surface.
    #[cfg(test)]
    pub(crate) fn set_panel_runner(&mut self, run: TestPanelRunner) {
        self.set_ns_run(std::sync::Arc::new(move |_m, argv, _stdin| {
            let fut = run(argv);
            Box::pin(async move { fut.await.map(|_| String::new()) })
        }));
    }

    /// Same adaptor for the sheet's `Ok(String)` runner shape.
    #[cfg(test)]
    pub(crate) fn set_hollywood_runner(&mut self, run: TestRunner) {
        self.set_ns_run(std::sync::Arc::new(move |_m, argv, _stdin| run(argv)));
    }

    /// A synthetic dock reply — `run_action`'s spawn never ran in
    /// the test, so a pending action is started and finished at
    /// once to land the reply through the real path.
    #[cfg(test)]
    pub(crate) fn on_panel_action(&mut self, (label, result): (String, Result<(), String>)) {
        self.inject_reply(label, result.map(|_| String::new()), false);
    }

    /// The sheet's synthetic reply — the `view` flag carried the
    /// card read, so it rides along here too.
    #[cfg(test)]
    pub(crate) fn on_hollywood_action(
        &mut self,
        (label, result, view): (String, Result<String, String>, bool),
    ) {
        self.inject_reply(label, result, view);
    }

    /// Start + immediately finish a named action with `result` —
    /// the toast/view branch decides on the name, same as a real
    /// reply would.
    #[cfg(test)]
    fn inject_reply(&mut self, name: String, result: Result<String, String>, view: bool) {
        let action = Action {
            label: name.clone(),
            done: name.clone(),
            name,
            mark: String::new(),
            argv: vec![],
            machine: None,
            stdin: None,
            view,
        };
        if let Some(id) = self.actions.start(action) {
            self.on_action_reply((id, result));
        }
    }
}

/// The hollywood tests' argv recorder shape.
#[cfg(test)]
pub(crate) type TestRunner =
    std::sync::Arc<dyn Fn(Vec<String>) -> crate::app::BoxFut<Result<String, String>> + Send + Sync>;

/// The panel tests' argv recorder — the same shape with a `()`
/// reply the adaptor wraps into empty output.
#[cfg(test)]
pub(crate) type TestPanelRunner =
    std::sync::Arc<dyn Fn(Vec<String>) -> crate::app::BoxFut<Result<(), String>> + Send + Sync>;
