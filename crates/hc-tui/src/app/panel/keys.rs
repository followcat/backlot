//! Focused-panel keys: the toggle chord check, the navigator-style
//! key ownership, Enter actions (focus, collapse, two-step reopen)
//! and the injectable `priority`/`reopen` runner.

use backlot_model::summary::RoleSummary;

use crate::app::input::TerminalKey;
use crate::panel::Target;
use crate::panel::data::StudioRef;

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
                self.panel.closed_hint = None;
                self.panel.finish_confirm = None;
                if self.panel_fullscreen() {
                    // An overlay dock backs out one step (S2 §1): the
                    // expanded overlay drops to the 16-column strip
                    // when that fits beside the panes, else closes.
                    // The collapsed overlay closes outright.
                    if self.panel.mode == crate::panel::PanelMode::Expanded
                        && self.area.0 >= super::STRIP_BESIDE_COLS
                    {
                        self.panel.mode = crate::panel::PanelMode::Collapsed;
                    } else {
                        self.panel_close();
                    }
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
            KeyCode::Char('?') if plain => self.help = true,
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
    /// `closed — press o to reopen` hint (S2 §4), a task row opens
    /// its sheet and the `✓ <n> accepted` fold toggles.
    pub(super) fn panel_activate(&mut self) {
        let machines = self.panel_summaries();
        let targets = self.panel_targets();
        let index = self.panel_selected_index(&targets);
        // Activating any row drops the closed-role hint — only Enter
        // itself may leave it up on its own row.
        self.panel.closed_hint = None;
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
    /// sidebar uses; a closed one shows the `closed — press o to
    /// reopen` hint — `o` does the reopen itself (S2 §4).
    fn panel_person_enter(&mut self, studio: StudioRef<'_>, role: &RoleSummary) {
        if role.state == "closed" {
            if !role.resumable {
                self.show_notice(
                    "Cannot reopen",
                    &format!("{} has no saved session to resume", role.name),
                );
                return;
            }
            self.panel.closed_hint = Some((studio.key(), role.role.clone()));
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
