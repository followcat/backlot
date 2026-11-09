//! Focused-panel keys: the toggle chord check, the navigator-style
//! key ownership, Enter actions (focus, collapse, two-step reopen)
//! and the injectable `priority`/`reopen` runner.

use backlot_model::event::Priority;
use backlot_model::summary::RoleSummary;

use crate::app::input::TerminalKey;
use crate::panel::{PanelAction, Target};

use super::App;

impl App {
    /// Whether this press is the configured panel chord tail.
    pub(crate) fn panel_key_press(&self, key: &TerminalKey) -> bool {
        let matched = crate::input::terminal_key_of(key);
        self.panel_key_combo
            .is_some_and(|combo| crate::config::terminal_key_matches_combo(&matched, combo))
    }

    /// Keys while the panel is focused — every press is consumed here,
    /// like `handle_navigator_key`. Repeats move only; releases are
    /// dropped.
    pub(crate) fn handle_panel_key(&mut self, key: &TerminalKey) {
        let combo = crate::input::terminal_key_of(key);
        // Prefix plumbing while focused: an armed prefix that is not the
        // panel chord (that was intercepted by the caller) disarms and
        // is eaten — the panel owns its keys.
        if self.prefix.armed() {
            self.prefix.reset();
            self.prefix.swallow(key);
            return;
        }
        if self.live.matches_prefix(&combo) {
            // Arm the chord: the next press runs the caller's toggle
            // intercept or the disarm branch above.
            let _ = self.prefix.feed(key, &self.live);
            return;
        }
        let (code, modifiers) = crate::config::normalize_key_combo((combo.code, combo.modifiers));
        let plain = modifiers.is_empty();
        if matches!(
            code,
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j')
        ) && plain
        {
            self.panel_move(1);
            return;
        }
        if matches!(
            code,
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k')
        ) && plain
        {
            self.panel_move(-1);
            return;
        }
        let page = self.panel_hits.viewport_rows.max(1) as isize;
        if code == crossterm::event::KeyCode::PageDown && plain {
            self.panel_move(page);
            return;
        }
        if code == crossterm::event::KeyCode::PageUp && plain {
            self.panel_move(-page);
            return;
        }
        if key.input.action != hc_vt::Action::Press {
            return;
        }
        use crossterm::event::{KeyCode, KeyModifiers};
        match code {
            KeyCode::Esc => self.panel.focused = false,
            KeyCode::Char('c') if plain => self.panel_flip_mode(),
            KeyCode::Char(' ') if plain => self.panel_collapse_selected(),
            KeyCode::Enter => self.panel_activate(),
            KeyCode::Char('+') | KeyCode::Char('=')
                if plain || modifiers == KeyModifiers::SHIFT =>
            {
                self.panel_priority(1)
            }
            KeyCode::Char('-') | KeyCode::Char('_')
                if plain || modifiers == KeyModifiers::SHIFT =>
            {
                self.panel_priority(-1)
            }
            _ => {}
        }
    }

    /// Enter/left-click on the selected row: the title flips the mode,
    /// a collapsed-mode working group expands, a group header
    /// collapses, an open person focuses their terminal, a studio its
    /// producer's, and a closed person runs the two-step reopen.
    pub(super) fn panel_activate(&mut self) {
        let summary = self.panel_summary();
        let targets = self.panel_targets();
        let index = self.panel_selected_index(&targets);
        match targets.get(index).cloned() {
            Some(Target::Title) => self.panel_flip_mode(),
            Some(Target::WorkingGroup(_)) => self.panel.mode = crate::panel::PanelMode::Expanded,
            Some(Target::Group(studio, group)) => {
                let key = (studio, group);
                if !self.panel.collapsed_groups.remove(&key) {
                    self.panel.collapsed_groups.insert(key);
                }
            }
            Some(Target::Studio(studio_id)) => {
                let producer = summary
                    .as_ref()
                    .and_then(|s| s.studios.iter().find(|st| st.id == studio_id))
                    .and_then(|st| {
                        st.roles
                            .iter()
                            .find(|r| r.role == "producer" && r.state == "live")
                    })
                    .and_then(|r| r.terminal.clone());
                if let Some(terminal) = producer {
                    self.focus_pane(terminal);
                    self.panel.focused = false;
                }
            }
            Some(Target::Person(studio, role_name)) => {
                let role = summary.as_ref().and_then(|s| {
                    s.studios
                        .iter()
                        .find(|st| st.id == studio)
                        .and_then(|st| st.roles.iter().find(|r| r.role == role_name))
                });
                if let Some(role) = role.cloned() {
                    self.panel_person_enter(&studio, &role);
                }
            }
            _ => {}
        }
    }

    /// Enter on a person: a live one focuses its terminal; a closed
    /// one arms the row hint first and reopens on the second press —
    /// or explains why it cannot.
    fn panel_person_enter(&mut self, studio: &str, role: &RoleSummary) {
        if role.state == "closed" {
            if !role.resumable {
                self.show_notice(
                    "Cannot reopen",
                    &format!("{} has no saved session to resume", role.name),
                );
                return;
            }
            let key = (studio.to_owned(), role.role.clone());
            if self.panel.reopen_confirm.as_ref() == Some(&key) {
                self.panel.reopen_confirm = None;
                let argv = self.panel_argv(
                    studio,
                    &["reopen".into(), "--person".into(), role.role.clone()],
                );
                self.run_panel_action(PanelAction {
                    label: format!("reopen {}", role.name),
                    argv,
                });
            } else {
                self.panel.reopen_confirm = Some(key);
            }
            return;
        }
        if role.state == "live"
            && let Some(terminal) = role.terminal.clone()
        {
            self.focus_pane(terminal);
            self.panel.focused = false;
        }
    }

    /// `+`/`-` on a selected actor: raise/lower its task priority.
    /// Clamped at the ends — no action is spawned there.
    fn panel_priority(&mut self, delta: i32) {
        let summary = self.panel_summary();
        let targets = self.panel_targets();
        let index = self.panel_selected_index(&targets);
        let Some(Target::Person(studio_id, role_name)) = targets.get(index).cloned() else {
            return;
        };
        let Some((studio, role)) = summary.as_ref().and_then(|s| {
            s.studios
                .iter()
                .find(|st| st.id == studio_id)
                .and_then(|st| {
                    st.roles
                        .iter()
                        .find(|r| r.role == role_name)
                        .map(|r| (st, r))
                })
        }) else {
            return;
        };
        let (Some(task), Some(priority)) = (role.task.clone(), role.priority) else {
            return; // only actors carry a task priority
        };
        let next = match (delta > 0, priority) {
            (true, Priority::Low) => Some(Priority::Normal),
            (true, Priority::Normal) => Some(Priority::High),
            (false, Priority::High) => Some(Priority::Normal),
            (false, Priority::Normal) => Some(Priority::Low),
            _ => None,
        };
        let Some(next) = next else { return };
        let argv = self.panel_argv(
            &studio.id,
            &[
                "priority".into(),
                "--task".into(),
                task,
                "--level".into(),
                next.to_string(),
            ],
        );
        self.run_panel_action(PanelAction {
            label: format!("priority {} {}", role.name, next),
            argv,
        });
    }

    /// The argv tail for a panel command — the command word first
    /// (`backlot` dispatches studio commands on argv[1]), then its
    /// flags, then the common `--studio`/`--as` and endpoint flags.
    fn panel_argv(&self, studio: &str, command: &[String]) -> Vec<String> {
        let mut argv = command.to_vec();
        argv.extend([
            "--studio".into(),
            studio.to_owned(),
            "--as".into(),
            "user".into(),
        ]);
        argv.extend(self.panel_endpoint_args.iter().cloned());
        argv
    }

    /// Spawn the action off the event loop through the injected
    /// runner; the result comes back on `panel_rx`.
    fn run_panel_action(&mut self, action: PanelAction) {
        let future = (self.panel_runner)(action.argv);
        let tx = self.panel_action_tx.clone();
        let label = action.label;
        tokio::spawn(async move {
            let _ = tx.send((label, future.await)).await;
        });
    }

    /// The action's result — a non-zero exit shows its first stderr
    /// line (the runner extracted it).
    pub(crate) fn on_panel_action(&mut self, (label, result): (String, Result<(), String>)) {
        if let Err(error) = result {
            self.show_notice("Studio action failed", &format!("{label}: {error}"));
        }
    }
}
