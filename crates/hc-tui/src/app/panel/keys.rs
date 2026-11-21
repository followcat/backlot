//! Focused-panel keys: the toggle chord check, the navigator-style
//! key ownership, Enter actions (focus, collapse, direct reopen)
//! and the injectable `priority`/`reopen` runner.

use backlot_model::summary::RoleSummary;

use crate::app::input::TerminalKey;
use crate::panel::Target;
use crate::panel::data::StudioRef;
use crate::studio_cmds;

use super::App;

impl App {
    /// Whether this press is the configured panel chord tail.
    pub(crate) fn panel_key_press(&self, key: &TerminalKey) -> bool {
        let matched = crate::input::terminal_key_of(key);
        self.panel_key_combo
            .is_some_and(|combo| crate::config::terminal_key_matches_combo(&matched, combo))
    }

    /// Whether this press is the chord's shifted tail — `prefix+Y`
    /// closes the dock from anywhere while it is open (S2 §4).
    pub(crate) fn panel_close_press(&self, key: &TerminalKey) -> bool {
        if !self.panel.open() {
            return false;
        }
        let matched = crate::input::terminal_key_of(key);
        self.panel_key_combo
            .map(|(code, mods)| (code, mods | crossterm::event::KeyModifiers::SHIFT))
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
            KeyCode::Esc => {
                self.panel.finish_confirm = None;
                if self.panel_fullscreen() || self.panel.esc_close {
                    // Only the strip ever overlays the panes now —
                    // an overlay dock closes outright (S2 §1). A dock
                    // holding a dead pane's hand-off closes too: the
                    // keys it took have nowhere to go back to.
                    self.panel_close();
                } else {
                    // Beside the panes: `esc back` — the dock keeps
                    // its place and hands the keys to the terminal.
                    self.panel.focused = false;
                }
            }
            KeyCode::Char('q') if plain => self.panel_close(),
            KeyCode::Char('c') if plain => self.panel_flip_mode(),
            KeyCode::Char(' ') if plain => self.panel_collapse_selected(),
            KeyCode::Enter => self.panel_activate(),
            KeyCode::Char('v') if plain => self.panel_sheet_open(),
            KeyCode::Char('o') if plain => self.panel_reopen_selected(),
            KeyCode::Char('p') if plain => self.panel_peek_selected(),
            KeyCode::Char('?') if plain => {
                self.help = true;
                self.help_state = crate::chrome::help::HelpState::default();
            }
            KeyCode::Char('r') if plain => self.panel_stage_start(),
            KeyCode::Char('f') if plain => self.panel_finish_selected(),
            KeyCode::Char(c @ ('y' | 'd')) if plain => self.panel_decide(c == 'y'),
            KeyCode::Char('n') if plain => {
                self.panel.finish_confirm = None;
            }
            KeyCode::Char('m') if plain => self.panel_menu_selected(),
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
    /// a block top border or strip line makes that studio current
    /// through the studio-nav path, a stacked block expands, an open
    /// person focuses their terminal, a closed person shows the
    /// a closed person reopens through `run_action` (S5 §2), a task row opens
    /// its sheet and the `✓ <n> accepted` fold toggles.
    pub(super) fn panel_activate(&mut self) {
        let machines = self.panel_summaries();
        let targets = self.panel_targets();
        let index = self.panel_selected_index(&targets);
        match targets.get(index).cloned() {
            Some(Target::Studio(id) | Target::Strip(id)) => {
                self.focus_studio(&id);
                self.panel_leave();
            }
            Some(Target::Task(key, task)) => {
                self.sheet_open(key, Some(task));
            }
            Some(Target::Fold(id)) => {
                if !self.panel.accepted_folds.remove(&id) {
                    self.panel.accepted_folds.insert(id);
                }
            }
            Some(Target::Stage(machine)) => {
                // The `[r] start` the row prints (U6c) — a click or
                // Enter on `stage not running` is the same call.
                if !self.machine_stage_running(machine.as_deref()) {
                    self.run_action(studio_cmds::stage_start(machine));
                }
            }
            // R22: the `hollywood setup` hint is the in-TUI dialog's
            // button — same dialog the Call Sheet's row opens.
            Some(Target::Setup) => self.hw_setup_open(),
            Some(Target::Block(id)) => {
                // A single-line block expands — the same manual
                // override Space writes; the selection follows the
                // block's new identity.
                self.panel.collapsed_blocks.remove(&id);
                self.panel.expanded_blocks.insert(id.clone());
                self.panel.selected = Some(Target::Studio(id.clone()));
                self.panel.selected_row = self
                    .panel_targets()
                    .iter()
                    .position(|t| *t == Target::Studio(id.clone()));
            }
            Some(Target::Person(key, role_name)) => {
                let studio = crate::panel::data::studio(&machines, &key);
                let role = studio.and_then(|s| {
                    s.studio
                        .roles
                        .iter()
                        .find(|r| r.role == role_name)
                        .map(|r| (s, r))
                });
                if let Some((studio, role)) = role.map(|(s, r)| (s, r.clone())) {
                    self.panel_person_enter(studio, &role);
                }
            }
            _ => {}
        }
    }

    /// Enter on a person: a live one focuses its terminal — a remote
    /// one lands through the same machine-qualified address the
    /// sidebar uses; a closed one runs `backlot reopen --person`
    /// when the seat can reopen — resumable or a fixed seat like the
    /// dock menu's rule — else the notice the hint carried (S5 §2).
    fn panel_person_enter(&mut self, studio: StudioRef<'_>, role: &RoleSummary) {
        if role.state == "closed" {
            let fixed = role.role == "producer" || !role.role.starts_with("actor");
            if !(role.resumable || fixed) {
                self.show_notice(
                    "Cannot reopen",
                    &format!("{} has no saved session to resume", role.name),
                );
                return;
            }
            self.run_action(studio_cmds::reopen(&studio.key(), &role.role));
            return;
        }
        if role.state == "live"
            && let Some(terminal) = role.terminal.clone()
        {
            self.focus_pane(terminal);
            self.panel_leave();
        }
    }
}
