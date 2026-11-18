//! The Call Sheet's row actions (S1 §5): each key's `backlot`
//! command, on the right machine — a remote studio's prompt, stage
//! and finish actions go through `RemoteRunner`; cards and `tell`
//! stay on the local core — and the pending mark while a command
//! runs. Nothing may fail silently: a command's stderr lands as a
//! notice where the user is looking (A7.1).

use std::time::Instant;

use super::App;
use crate::hollywood::{self, CardView, Confirm, Target};
use crate::studio::StudioKey;

impl App {
    /// The argv tail for a layer command — the subcommand first,
    /// then `--as user` and the endpoint flags, like `panel_argv`.
    /// A remote command drops the endpoint flags: its terminal on
    /// that machine reaches its own core through `HC_*`.
    fn hollywood_argv(&self, command: &[String], machine: Option<&str>) -> Vec<String> {
        let mut argv = command.to_vec();
        argv.extend(["--as".into(), "user".into()]);
        if machine.is_none() {
            argv.extend(self.panel_endpoint_args.iter().cloned());
        }
        argv
    }

    /// `y`/`d` on a prompt row: `backlot approve --person <role_key>
    /// --decision <d>`, on the prompt's own machine (D9).
    fn hollywood_prompt_decision(&mut self, studio: &StudioKey, role: &str, allow: bool) {
        let argv = self.hollywood_argv(
            &[
                "approve".into(),
                "--person".into(),
                role.to_string(),
                "--decision".into(),
                if allow { "allow".into() } else { "deny".into() },
                "--studio".into(),
                studio.id.clone(),
            ],
            studio.machine.as_deref(),
        );
        self.run_hollywood_action(hollywood::HollywoodAction {
            label: format!("approve {role} {}", if allow { "allow" } else { "deny" }),
            argv,
            view: false,
            machine: studio.machine.clone(),
        });
    }

    /// `p` on a prompt row: the role's live screen, read-only —
    /// machine-qualified for a remote studio.
    fn hollywood_peek(&mut self, studio: &StudioKey, role: &str) {
        let machines = self.panel_summaries();
        let terminal = crate::panel::data::studio(&machines, studio).and_then(|s| {
            s.studio
                .roles
                .iter()
                .find(|r| r.role == role)
                .and_then(|r| r.terminal.clone())
        });
        match terminal {
            Some(terminal) => self.open_peek(terminal),
            None => self.show_notice("Hollywood", "the role has no screen to peek"),
        }
    }

    /// `r` on a stage row: `backlot stage start` on the machine —
    /// the same command the panel's `s` runs.
    pub(super) fn hollywood_stage_start(&mut self, machine: Option<String>) {
        let argv = self.hollywood_argv(&["stage".into(), "start".into()], machine.as_deref());
        let machine_label = machine.clone().unwrap_or_else(|| "Local".into());
        self.run_hollywood_action(hollywood::HollywoodAction {
            label: format!("stage start {machine_label}"),
            argv,
            view: false,
            machine,
        });
    }

    /// `f` on a ready row: `backlot studio finish`, on the studio's
    /// own machine, after the inline confirm answers `y`.
    fn hollywood_finish(&mut self, key: &StudioKey) {
        let argv = self.hollywood_argv(
            &[
                "studio".into(),
                "finish".into(),
                "--studio".into(),
                key.id.clone(),
            ],
            key.machine.as_deref(),
        );
        self.run_hollywood_action(hollywood::HollywoodAction {
            label: format!("studio finish {}", key.id),
            argv,
            view: false,
            machine: key.machine.clone(),
        });
    }

    /// `a` / Apply on the selected card: `backlot card apply <id>`.
    fn hollywood_card_apply(&mut self, id: &str) {
        let argv = self.hollywood_argv(&["card".into(), "apply".into(), id.to_string()], None);
        self.run_hollywood_action(hollywood::HollywoodAction {
            label: format!("card apply {id}"),
            argv,
            view: false,
            machine: None,
        });
    }

    /// `x` / Cancel on the selected card: `backlot card cancel <id>`
    /// with the layer's reason.
    fn hollywood_card_cancel(&mut self, id: &str) {
        let argv = self.hollywood_argv(
            &[
                "card".into(),
                "cancel".into(),
                id.to_string(),
                "--reason".into(),
                "cancelled from the call sheet".into(),
            ],
            None,
        );
        self.run_hollywood_action(hollywood::HollywoodAction {
            label: format!("card cancel {id}"),
            argv,
            view: false,
            machine: None,
        });
    }

    /// `s` / Show me on any card: `backlot card show <id>` in the
    /// read-only view.
    pub(super) fn hollywood_card_show(&mut self, id: &str) {
        let argv = self.hollywood_argv(&["card".into(), "show".into(), id.to_string()], None);
        self.run_hollywood_action(hollywood::HollywoodAction {
            label: format!("card show {id}"),
            argv,
            view: true,
            machine: None,
        });
    }

    /// A plain key on the selected row — the Call Sheet's own
    /// dispatch (S1 §5). Returns true when the key was the row's.
    pub(super) fn hollywood_row_key(&mut self, code: crossterm::event::KeyCode) -> bool {
        use crossterm::event::KeyCode;
        let targets = self.hollywood_targets();
        let index = self.hollywood_selected_index(&targets);
        match (targets.get(index).cloned(), code) {
            (Some(Target::Prompt(studio, role)), KeyCode::Char('y')) => {
                self.hollywood_prompt_decision(&studio, &role, true);
            }
            (Some(Target::Prompt(studio, role)), KeyCode::Char('d')) => {
                self.hollywood_prompt_decision(&studio, &role, false);
            }
            (Some(Target::Prompt(studio, role)), KeyCode::Char('p')) => {
                self.hollywood_peek(&studio, &role);
            }
            (Some(Target::Stage(machine, false)), KeyCode::Char('r')) => {
                self.hollywood_stage_start(machine);
            }
            (Some(Target::Card(id)), KeyCode::Char('a')) => self.hollywood_card_apply(&id),
            (Some(Target::Card(id)), KeyCode::Char('x')) => self.hollywood_card_cancel(&id),
            (Some(Target::Card(id)), KeyCode::Char('s')) => self.hollywood_card_show(&id),
            (Some(Target::Ready(key)), KeyCode::Char('f')) => {
                self.hollywood.confirm = Some(Confirm::Finish(key));
            }
            _ => return false,
        }
        true
    }

    /// The armed `y/n` confirm's answer — `y` runs the action, `n`
    /// and `Esc` stand down. True when one was armed.
    pub(super) fn hollywood_confirm_key(&mut self, code: crossterm::event::KeyCode) -> bool {
        use crossterm::event::KeyCode;
        let Some(confirm) = self.hollywood.confirm.clone() else {
            return false;
        };
        match code {
            KeyCode::Char('y') => {
                self.hollywood.confirm = None;
                match confirm {
                    Confirm::Finish(key) => self.hollywood_finish(&key),
                }
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                self.hollywood.confirm = None;
            }
            _ => {}
        }
        true
    }

    /// Enter on the editing input line: the editor closes — a
    /// nonempty line goes out as `backlot tell hollywood "<line>"`
    /// (D20); an empty one sends nothing.
    pub(super) fn hollywood_input_submit(&mut self) {
        self.hollywood.editing = false;
        let line = self.hollywood.input.as_str().trim().to_string();
        if line.is_empty() {
            return;
        }
        self.hollywood.input.clear();
        let argv = self.hollywood_argv(&["tell".into(), "hollywood".into(), line], None);
        self.run_hollywood_action(hollywood::HollywoodAction {
            label: "tell hollywood".into(),
            argv,
            view: false,
            machine: None,
        });
    }

    // ---------- the action runner ----------

    /// Spawn the action off the event loop through the injected
    /// runner — a remote machine's prompt, stage and finish actions
    /// go through `RemoteRunner`, everything else through the local
    /// `backlot` spawn — and mark the row `…` while it runs; the
    /// result comes back on `hwood_rx`.
    fn run_hollywood_action(&mut self, action: hollywood::HollywoodAction) {
        let future = match &action.machine {
            None => (self.hollywood_runner)(action.argv.clone()),
            Some(machine) => match &self.remote_runner {
                Some(runner) => {
                    let future = runner.run(machine.clone(), action.argv.clone());
                    Box::pin(async move { future.await.map(|_| String::new()) })
                }
                None => {
                    self.show_notice(
                        "Hollywood action failed",
                        &format!("{}: cannot reach {machine}", action.label),
                    );
                    return;
                }
            },
        };
        self.hollywood.pending = Some(action.label.clone());
        let tx = self.hwood_tx.clone();
        tokio::spawn(async move {
            let _ = tx.send((action.label, future.await, action.view)).await;
        });
    }

    /// The action's result: `card show` output opens the view; a
    /// failure is a notice carrying the first stderr line.
    pub(crate) fn on_hollywood_action(
        &mut self,
        (label, result, view): (String, Result<String, String>, bool),
    ) {
        self.hollywood.pending = None;
        self.sched.mark_dirty(Instant::now());
        match result {
            Ok(stdout) if view => {
                let title = label.trim_start_matches("card show ").to_string();
                let lines: Vec<String> = stdout.lines().map(str::to_string).collect();
                if lines.is_empty() {
                    self.show_notice("Hollywood", &format!("{label}: nothing to show"));
                } else {
                    self.hollywood.view = Some(CardView {
                        title,
                        lines,
                        scroll: 0,
                    });
                }
            }
            Ok(_) => {}
            Err(error) => {
                self.show_notice("Hollywood action failed", &format!("{label}: {error}"));
            }
        }
    }
}
