//! Keyboard focus and navigation inside the left sidebar's Cast section
//! (`Ctrl+B a` / `[keys] focus_cast`).

use std::time::Instant;

use crossterm::event::{KeyCode, KeyModifiers};

use crate::app::input::TerminalKey;
use crate::focus::{CastTarget, TellTarget, WorkspaceRegion};
use crate::studio::{self, StudioKey};

use super::App;

impl App {
    /// Whether this press matches `[keys] focus_cast` as a prefix chord tail.
    pub(crate) fn cast_focus_press(&self, key: &TerminalKey) -> bool {
        if !self.studio.active {
            return false;
        }
        let matched = crate::input::terminal_key_of(key);
        self.panel_config.keys.focus_cast.matches_chord(&matched)
    }

    /// Whether this press matches `[keys] focus_cast` as a direct chord.
    pub(crate) fn cast_focus_direct_press(&self, key: &TerminalKey) -> bool {
        if !self.studio.active || self.panel_editing() || self.hollywood.editing {
            return false;
        }
        let matched = crate::input::terminal_key_of(key);
        self.panel_config.keys.focus_cast.matches_direct(&matched)
    }

    pub(crate) fn cast_focused(&self) -> bool {
        self.focus.workspace == WorkspaceRegion::Cast
    }

    #[cfg(test)]
    pub(crate) fn cast_chip(&self) -> Option<usize> {
        self.cast_chip
    }

    /// Resolved studio shown by Cast (`studio.current` or sole live studio).
    pub(crate) fn cast_studio_key(&self) -> Option<StudioKey> {
        let machines = self.panel_summaries();
        let studios = crate::panel::data::studios(&machines);
        if let Some(cur) = &self.studio.current
            && studios.iter().any(|s| &s.key() == cur)
        {
            return Some(cur.clone());
        }
        let live: Vec<_> = studios
            .iter()
            .filter(|s| s.studio.state != "finished")
            .collect();
        (live.len() == 1).then(|| live[0].key())
    }

    /// Selectable targets in the Cast sidebar matching draw order.
    pub(crate) fn cast_targets(&self) -> Vec<CastTarget> {
        let mut out = Vec::new();
        let machines = self.panel_summaries();
        if let Some(key) = self.cast_studio_key()
            && let Some(studio_ref) = crate::panel::data::studio(&machines, &key)
        {
            let ordered = studio::cast_order(studio_ref.studio);
            for role in &ordered {
                if self.shell.studio_chrome && role.state == "closed" {
                    continue;
                }
                out.push(CastTarget::Role {
                    studio: key.clone(),
                    role: role.role.clone(),
                });
            }
            if self.shell.studio_chrome && ordered.iter().any(|r| r.state == "closed") {
                out.push(CastTarget::ClosedRoles {
                    studio: key.clone(),
                });
            }
            for (_n, terminal) in
                crate::shells::studio_shells(&self.view, studio_ref.machine, &studio_ref.studio.id)
            {
                if self.view.inventory.get(&terminal).is_some() {
                    out.push(CastTarget::Shell {
                        studio: key.clone(),
                        terminal,
                    });
                }
            }
        }
        // Check `others` toggle if any non-studio agents exist in snapshot.
        let studios = crate::panel::data::studios(&machines);
        let mut seated: std::collections::BTreeSet<&str> = studios
            .iter()
            .flat_map(|s| s.studio.roles.iter())
            .filter_map(|r| r.terminal.as_deref())
            .collect();
        seated.extend(
            self.view
                .inventory
                .terminals()
                .values()
                .filter(|t| {
                    t.labels
                        .keys()
                        .any(|k| k == crate::shells::SHELL_LABEL || k.starts_with("backlot."))
                })
                .map(|t| t.address.as_str()),
        );
        let others: Vec<String> = crate::chrome::agents::ordered_agent_pane_ids(
            &self.projection.snapshot,
            self.shell.agent_panel_sort,
        )
        .into_iter()
        .filter(|id| !seated.contains(id.as_str()))
        .collect();
        if !others.is_empty() {
            out.push(CastTarget::OthersToggle);
            if self.studio.others_expanded {
                out.extend(others.into_iter().map(CastTarget::OtherAgent));
            }
        }
        out
    }

    fn initial_cast_cursor(&self, targets: &[CastTarget]) -> Option<CastTarget> {
        if targets.is_empty() {
            return None;
        }
        if let Some(focused) = self.focused.as_deref() {
            let machines = self.panel_summaries();
            if let Some(key) = self.cast_studio_key()
                && let Some(st) = crate::panel::data::studio(&machines, &key)
                && let Some(role) = st
                    .studio
                    .roles
                    .iter()
                    .find(|r| r.terminal.as_deref() == Some(focused))
            {
                let cand = CastTarget::Role {
                    studio: key,
                    role: role.role.clone(),
                };
                if targets.contains(&cand) {
                    return Some(cand);
                }
            }
            if let Some(cand) = targets
                .iter()
                .find(|t| matches!(t, CastTarget::Shell { terminal, .. } if terminal == focused))
            {
                return Some(cand.clone());
            }
        }
        targets
            .iter()
            .find(|t| matches!(t, CastTarget::Role { role, .. } if role == "producer"))
            .or_else(|| {
                self.cast_remembered
                    .as_ref()
                    .filter(|t| targets.contains(t))
            })
            .or_else(|| targets.first())
            .cloned()
    }

    /// Enter Cast focus (`Ctrl+B a` or `Tab` from `Tree`).
    pub(crate) fn cast_focus_enter(&mut self) {
        if !self.studio.active {
            return;
        }
        self.shell_state.sidebar_collapsed = false;
        self.nav_leave_overlays();
        if self.mode == crate::app::Mode::Navigate {
            self.mode = crate::app::Mode::Normal;
            self.navigate_target = None;
        }
        self.peek = None;
        let targets = self.cast_targets();
        let initial = self.initial_cast_cursor(&targets);
        self.cast_focus = initial.clone();
        self.cast_chip = None;
        if initial.is_some() {
            self.cast_remembered = initial;
        }
        self.set_workspace_focus(WorkspaceRegion::Cast);
        self.shell_state.reveal_focused_agent = true;
        self.sched.mark_dirty(Instant::now());
    }

    /// Leave Cast focus back to `Terminal`, remembering the last row.
    pub(crate) fn cast_focus_leave(&mut self) {
        self.cast_chip = None;
        if let Some(cur) = self.cast_focus.take() {
            self.cast_remembered = Some(cur);
        }
        if matches!(self.focus.workspace, WorkspaceRegion::Cast) {
            self.set_workspace_focus(WorkspaceRegion::Terminal);
            self.sched.mark_dirty(Instant::now());
        }
    }

    /// Sync Cast cursor when `Alt+Up/Down` or `Alt+PageUp/PageDown` changes pane/studio while in Cast.
    pub(crate) fn cast_follow_active(&mut self) {
        if !self.cast_focused() {
            return;
        }
        let targets = self.cast_targets();
        let next = self.initial_cast_cursor(&targets);
        self.cast_focus = next.clone();
        if next.is_some() {
            self.cast_remembered = next;
        }
        self.shell_state.reveal_focused_agent = true;
    }

    /// Key handling while `WorkspaceRegion::Cast` owns input.
    pub(crate) fn handle_cast_key(&mut self, key: &TerminalKey) {
        let combo = crate::input::terminal_key_of(key);
        let (code, modifiers) = crate::config::normalize_key_combo((combo.code, combo.modifiers));
        let plain = modifiers.is_empty();
        let press = key.input.action == hc_vt::Action::Press;
        match code {
            KeyCode::Esc if press => self.cast_focus_leave(),
            KeyCode::Tab | KeyCode::BackTab if plain || modifiers == KeyModifiers::SHIFT => {
                self.tree_focus_enter();
            }
            KeyCode::Up | KeyCode::Char('k') if plain => self.cast_move(-1),
            KeyCode::Down | KeyCode::Char('j') if plain => self.cast_move(1),
            KeyCode::Left | KeyCode::Char('h') if plain && press => self.cast_chip_move(-1),
            KeyCode::Right | KeyCode::Char('l') if plain && press => self.cast_chip_move(1),
            KeyCode::Enter if press => {
                if let Some(chip_idx) = self.cast_chip {
                    self.cast_execute_chip(chip_idx);
                } else {
                    self.cast_activate();
                }
            }
            KeyCode::Char('m') if plain && press => self.cast_open_menu(),
            KeyCode::Char('o') if plain && press => self.cast_reopen(),
            KeyCode::Char('t') if plain && press => self.cast_tell(),
            KeyCode::Char('p') if plain && press => self.cast_peek(),
            KeyCode::Char(' ') if plain && press => {
                if matches!(self.cast_focus, Some(CastTarget::OthersToggle)) {
                    self.studio.others_expanded = !self.studio.others_expanded;
                    self.sched.mark_dirty(Instant::now());
                }
            }
            KeyCode::Char(c @ ('y' | 'd')) if plain && press => self.cast_decide(c == 'y'),
            KeyCode::Char('r') if plain && press => self.cast_tell(),
            KeyCode::Char('x') if plain && press => self.cast_ack_ask(),
            _ => {}
        }
    }

    fn cast_role_chips(&self) -> Vec<crate::panel::chips::RowChip> {
        let Some(CastTarget::Role { studio, role }) = self.cast_focus.as_ref() else {
            return Vec::new();
        };
        let machines = self.panel_summaries();
        let Some(r) = crate::panel::data::studio(&machines, studio)
            .and_then(|s| s.studio.roles.iter().find(|r| r.role == *role))
        else {
            return Vec::new();
        };
        // Cast Left/Right cycles inline actions when the role needs you (prompt/ask/error).
        if (r.attention_user && r.can_answer_prompt)
            || r.asks.as_ref().is_some_and(|a| !a.is_empty())
            || crate::panel::chips::is_role_quota_or_error(r)
        {
            crate::panel::chips::row_chips_for_person(r)
        } else {
            Vec::new()
        }
    }

    fn cast_chip_move(&mut self, delta: isize) {
        let chips = self.cast_role_chips();
        if chips.is_empty() {
            self.cast_chip = None;
            return;
        }
        let next = match self.cast_chip {
            Some(cur) => (cur as isize + delta).rem_euclid(chips.len() as isize) as usize,
            None if delta >= 0 => 0,
            None => chips.len().saturating_sub(1),
        };
        self.cast_chip = Some(next);
        self.sched.mark_dirty(Instant::now());
    }

    fn cast_execute_chip(&mut self, chip_idx: usize) {
        let chips = self.cast_role_chips();
        let Some(chip) = chips.get(chip_idx).copied() else {
            self.cast_activate();
            return;
        };
        use crate::panel::chips::RowChipKind;
        match chip.kind {
            RowChipKind::Allow => self.cast_decide(true),
            RowChipKind::Deny => self.cast_decide(false),
            RowChipKind::Reply | RowChipKind::Tell => self.cast_tell(),
            RowChipKind::Ack => self.cast_ack_ask(),
            RowChipKind::Peek => self.cast_peek(),
            RowChipKind::Menu => self.cast_open_menu(),
            RowChipKind::Open => self.cast_activate(),
            RowChipKind::Switch => {
                if let Some(CastTarget::Role { studio, role }) = self.cast_focus.clone() {
                    let machines = self.panel_summaries();
                    if let Some(st) = crate::panel::data::studio(&machines, &studio)
                        && let Some(r) = st.studio.roles.iter().find(|r| r.role == role)
                    {
                        self.recast_dialog_open(
                            Some(&studio),
                            &r.role,
                            &r.name,
                            r.runner.clone(),
                            r.model.clone(),
                            r.effort.clone(),
                        );
                    }
                }
            }
            _ => self.cast_activate(),
        }
    }

    fn cast_move(&mut self, delta: isize) {
        self.cast_chip = None;
        let targets = self.cast_targets();
        if targets.is_empty() {
            return;
        }
        let cur = self
            .cast_focus
            .as_ref()
            .and_then(|c| targets.iter().position(|t| t == c))
            .unwrap_or(0) as isize;
        let next = (cur + delta).clamp(0, targets.len() as isize - 1) as usize;
        let chosen = targets[next].clone();
        self.cast_focus = Some(chosen.clone());
        self.cast_remembered = Some(chosen);
        self.shell_state.reveal_focused_agent = true;
        self.sched.mark_dirty(Instant::now());
    }

    fn cast_activate(&mut self) {
        let Some(cur) = self.cast_focus.clone() else {
            return;
        };
        match cur {
            CastTarget::Role { studio, role } => {
                let machines = self.panel_summaries();
                let found = crate::panel::data::studio(&machines, &studio)
                    .and_then(|s| s.studio.roles.iter().find(|r| r.role == role).cloned());
                let Some(r) = found else {
                    return;
                };
                if r.state == "live"
                    && let Some(term) = r.terminal
                {
                    self.cast_focus_leave();
                    self.focus_pane(term);
                } else if r.state == "closed" {
                    self.show_notice(
                        "Person closed",
                        &format!("{} is closed — press o to reopen", r.name),
                    );
                } else if r.state == "standby" {
                    self.show_notice(
                        "Terminal unavailable",
                        &crate::studio::standby_unavailable(&r),
                    );
                }
            }
            CastTarget::ClosedRoles { studio } => self.open_cast_reopen_picker(&studio),
            CastTarget::Shell { terminal, .. } => {
                self.cast_focus_leave();
                self.focus_pane(terminal);
            }
            CastTarget::OthersToggle => {
                self.studio.others_expanded = !self.studio.others_expanded;
            }
            CastTarget::OtherAgent(pane_id) => {
                self.cast_focus_leave();
                self.focus_pane(pane_id);
            }
        }
    }

    fn cast_open_menu(&mut self) {
        let point = {
            let body = self.hit_map.agent_body;
            if body.is_empty() {
                (2, 12)
            } else {
                (body.x + 2, body.y + 1)
            }
        };
        let machines = self.panel_summaries();
        match self.cast_focus.clone() {
            Some(CastTarget::Role { studio, role }) => {
                if let Some(st) = crate::panel::data::studio(&machines, &studio)
                    && let Some(r) = st.studio.roles.iter().find(|r| r.role == role)
                {
                    let offline = !self.machine_reachable(st.machine);
                    self.open_studio_menu(
                        crate::studio_menu::StudioMenu::role(&studio, r, offline),
                        point,
                    );
                }
            }
            Some(CastTarget::Shell { studio, .. }) => {
                self.studio_header_menu(&studio, point);
            }
            _ => {
                if let Some(key) = self.cast_studio_key() {
                    self.studio_header_menu(&key, point);
                }
            }
        }
    }

    fn cast_tell(&mut self) {
        let (key, target) = match self.cast_focus.clone() {
            Some(CastTarget::Role { studio, role }) => (studio, TellTarget::Role(role)),
            _ => match self.cast_studio_key() {
                Some(studio) => (studio, TellTarget::default()),
                None => return,
            },
        };
        self.panel.mode = crate::panel::PanelMode::Expanded;
        self.enter_lot_compose(&key, target);
    }

    fn cast_peek(&mut self) {
        let Some(CastTarget::Role { studio, role }) = self.cast_focus.clone() else {
            return;
        };
        let machines = self.panel_summaries();
        let Some(r) = crate::panel::data::studio(&machines, &studio)
            .and_then(|s| s.studio.roles.iter().find(|r| r.role == role).cloned())
        else {
            return;
        };
        let Some(terminal) = r.terminal.clone() else {
            return;
        };
        let studio_title = self.studio_peek_title(&studio, &r.name);
        self.open_peek(terminal);
        if let Some(peek) = self.peek.as_mut() {
            peek.studio_title = Some(studio_title);
            if r.attention_user && r.can_answer_prompt {
                peek.prompt = Some((studio, r.role));
            }
        }
    }

    fn cast_decide(&mut self, allow: bool) {
        let Some(CastTarget::Role { studio, role }) = self.cast_focus.clone() else {
            return;
        };
        let machines = self.panel_summaries();
        let Some(r) = crate::panel::data::studio(&machines, &studio)
            .and_then(|s| s.studio.roles.iter().find(|r| r.role == role))
            .cloned()
        else {
            return;
        };
        if !r.can_answer_prompt {
            return;
        }
        if r.attention_user {
            self.run_action(crate::studio_cmds::decide(&studio, &r.role, allow));
        } else if r.attention_seq.is_some() {
            self.studio_dialog = Some(crate::studio_dialog::StudioDialog::dir_override(
                &studio, &r.role, allow,
            ));
        }
    }

    fn cast_ack_ask(&mut self) {
        let Some(CastTarget::Role { studio, role }) = self.cast_focus.clone() else {
            return;
        };
        match self.ask_target(&studio, &role) {
            Some(target) => {
                self.run_action(crate::studio_cmds::ask_acknowledge(&studio, &role, target));
            }
            None => {
                let machines = self.panel_summaries();
                if crate::panel::data::studio(&machines, &studio)
                    .and_then(|s| s.studio.roles.iter().find(|r| r.role == role))
                    .is_some_and(|r| r.asks.is_some())
                {
                    self.show_notice("Cast", "that question cannot be acknowledged here");
                }
            }
        }
    }
}
