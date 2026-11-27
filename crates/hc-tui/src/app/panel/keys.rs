//! Focused-panel keys: the toggle chord check, the navigator-style
//! key ownership, Enter actions (focus, collapse, direct reopen)
//! and the injectable `priority`/`reopen` runner.

use backlot_model::summary::RoleSummary;

use crate::app::input::TerminalKey;
use crate::panel::Target;
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
        // An armed `finish <id>? [y/n]` outranks even a `›` line's
        // edit — its letters answer the prompt, never land in a
        // draft (the Call Sheet's same rule).
        if self.panel.finish_confirm.is_some() && key.input.action == hc_vt::Action::Press {
            match code {
                crossterm::event::KeyCode::Char('y') if plain => {
                    self.panel_decide(true);
                    return;
                }
                crossterm::event::KeyCode::Char('d') if plain => {
                    self.panel_decide(false);
                    return;
                }
                crossterm::event::KeyCode::Char('n') if plain => {
                    self.panel.finish_confirm = None;
                    return;
                }
                crossterm::event::KeyCode::Esc => {
                    self.panel.finish_confirm = None;
                    return;
                }
                _ => {}
            }
        }
        // `Compose`: the `›` line owns every key — `⏎` sends (or runs
        // a `/verb`) back to `Resting`, `esc` rests on `›` keeping its
        // draft (even if empty), plain `↑`/`↓` step per-studio history
        // without leaving Compose, `Tab` completes `/verb`, the rest is
        // text for the studio's draft.
        if self.panel_editing() {
            if !matches!(
                key.input.action,
                hc_vt::Action::Press | hc_vt::Action::Repeat
            ) {
                return;
            }
            match code {
                crossterm::event::KeyCode::Esc if modifiers.is_empty() => {
                    if key.input.action != hc_vt::Action::Press {
                        return;
                    }
                    self.panel.editing = false;
                    self.panel.reset_history_nav();
                }
                crossterm::event::KeyCode::Up if modifiers.is_empty() => {
                    if let Some(Target::Input(studio_key)) = self.panel.selected.clone() {
                        self.panel_sync_history(&studio_key);
                        let current = self
                            .panel
                            .draft(&studio_key)
                            .map(|d| d.as_str().to_owned())
                            .unwrap_or_default();
                        if let Some(entry) = self.panel.history_mut(&studio_key).step_up(&current) {
                            let draft = self.panel.draft_mut(&studio_key);
                            draft.clear();
                            let _ = draft.insert(&entry);
                        }
                    }
                }
                crossterm::event::KeyCode::Down if modifiers.is_empty() => {
                    if let Some(Target::Input(studio_key)) = self.panel.selected.clone() {
                        self.panel_sync_history(&studio_key);
                        if let Some(entry) = self.panel.history_mut(&studio_key).step_down() {
                            let draft = self.panel.draft_mut(&studio_key);
                            draft.clear();
                            let _ = draft.insert(&entry);
                        }
                    }
                }
                crossterm::event::KeyCode::Enter if modifiers.is_empty() => {
                    if key.input.action != hc_vt::Action::Press {
                        return;
                    }
                    self.panel_input_submit();
                }
                crossterm::event::KeyCode::Tab if modifiers.is_empty() => {
                    if key.input.action != hc_vt::Action::Press {
                        return;
                    }
                    self.panel_input_complete();
                }
                _ => {
                    if let Some(Target::Input(studio_key)) = self.panel.selected.clone() {
                        let _ = self.panel.draft_mut(&studio_key).handle_key(key);
                    }
                }
            }
            return;
        }
        if matches!(
            code,
            crossterm::event::KeyCode::Tab | crossterm::event::KeyCode::BackTab
        ) && (plain || modifiers == crossterm::event::KeyModifiers::SHIFT)
        {
            let forward = code == crossterm::event::KeyCode::Tab;
            match self.focus.workspace.clone() {
                crate::focus::WorkspaceRegion::Lot(crate::focus::LotLevel::Studios) => {
                    self.panel_studios_tab(forward);
                }
                crate::focus::WorkspaceRegion::Lot(crate::focus::LotLevel::InsideStudio(
                    studio,
                )) => {
                    self.panel_section_tab(&studio, forward);
                }
                crate::focus::WorkspaceRegion::Lot(crate::focus::LotLevel::RowActions {
                    studio,
                    ..
                }) => {
                    self.panel_row_chip_move(&studio, if forward { 1 } else { -1 });
                }
                _ => {}
            }
            return;
        }
        if matches!(
            code,
            crossterm::event::KeyCode::Left
                | crossterm::event::KeyCode::Right
                | crossterm::event::KeyCode::Char('h')
                | crossterm::event::KeyCode::Char('l')
        ) && plain
        {
            let right = matches!(
                code,
                crossterm::event::KeyCode::Right | crossterm::event::KeyCode::Char('l')
            );
            let delta = if right { 1 } else { -1 };
            match self.focus.workspace.clone() {
                crate::focus::WorkspaceRegion::Lot(crate::focus::LotLevel::InsideStudio(
                    studio,
                ))
                | crate::focus::WorkspaceRegion::Lot(crate::focus::LotLevel::RowActions {
                    studio,
                    ..
                }) => {
                    self.panel_row_chip_move(&studio, delta);
                }
                crate::focus::WorkspaceRegion::Lot(crate::focus::LotLevel::Studios) => {
                    let targets = self.panel_targets();
                    let idx = self.panel_selected_index(&targets);
                    let cur = targets.get(idx).cloned();
                    if right {
                        match cur {
                            Some(Target::Studio(k) | Target::Block(k) | Target::Strip(k)) => {
                                self.enter_studio_in_lot(&k);
                            }
                            Some(_) => self.panel_activate(),
                            None => {}
                        }
                    } else if let Some(Target::Studio(k)) = cur {
                        self.panel.expanded_blocks.remove(&k);
                        self.panel.collapsed_blocks.insert(k.clone());
                        self.panel.selected = Some(Target::Block(k));
                        self.panel_reveal_selected();
                    }
                }
                _ => {}
            }
            return;
        }
        let move_delta = if matches!(
            code,
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Char('j')
        ) && plain
        {
            Some(1)
        } else if matches!(
            code,
            crossterm::event::KeyCode::Up | crossterm::event::KeyCode::Char('k')
        ) && plain
        {
            Some(-1)
        } else {
            let page = self.panel_hits.viewport_rows.max(1) as isize;
            if code == crossterm::event::KeyCode::PageDown && plain {
                Some(page)
            } else if code == crossterm::event::KeyCode::PageUp && plain {
                Some(-page)
            } else if code == crossterm::event::KeyCode::Home && plain {
                Some(isize::MIN)
            } else if code == crossterm::event::KeyCode::End && plain {
                Some(isize::MAX)
            } else {
                None
            }
        };
        if let Some(delta) = move_delta {
            match self.focus.workspace.clone() {
                crate::focus::WorkspaceRegion::Lot(crate::focus::LotLevel::InsideStudio(
                    studio,
                )) => {
                    self.panel_move_bounded(&studio, delta);
                }
                crate::focus::WorkspaceRegion::Lot(crate::focus::LotLevel::RowActions {
                    studio,
                    ..
                }) => {
                    self.set_workspace_focus(crate::focus::WorkspaceRegion::Lot(
                        crate::focus::LotLevel::InsideStudio(studio.clone()),
                    ));
                    self.panel_move_bounded(&studio, delta);
                }
                _ => self.panel_move(delta),
            }
            return;
        }
        if key.input.action != hc_vt::Action::Press {
            return;
        }
        use crossterm::event::{KeyCode, KeyModifiers};
        match code {
            KeyCode::Esc => {
                self.panel.finish_confirm = None;
                match self.focus.workspace.clone() {
                    crate::focus::WorkspaceRegion::Lot(
                        crate::focus::LotLevel::RowActions { studio, .. },
                    ) => {
                        self.set_workspace_focus(crate::focus::WorkspaceRegion::Lot(
                            crate::focus::LotLevel::InsideStudio(studio),
                        ));
                    }
                    crate::focus::WorkspaceRegion::Lot(
                        crate::focus::LotLevel::InsideStudio(studio),
                    ) => {
                        let machines = self.panel_summaries();
                        let count = crate::panel::data::studios(&machines).len();
                        if count >= 2 {
                            self.panel.selected = Some(Target::Studio(studio));
                            self.set_workspace_focus(crate::focus::WorkspaceRegion::Lot(
                                crate::focus::LotLevel::Studios,
                            ));
                            self.panel_reveal_selected();
                        } else if self.panel_esc_closes() {
                            self.panel_close();
                        } else {
                            self.set_workspace_focus(crate::focus::WorkspaceRegion::Terminal);
                        }
                    }
                    _ => {
                        if self.panel_esc_closes() {
                            self.panel_close();
                        } else {
                            self.set_workspace_focus(crate::focus::WorkspaceRegion::Terminal);
                        }
                    }
                }
            }
            KeyCode::Char('q') if plain => self.panel_close(),
            KeyCode::Char('c') if plain => self.panel_flip_mode(),
            KeyCode::Char(' ') if plain => self.panel_collapse_selected(),
            KeyCode::Enter => {
                if let crate::focus::WorkspaceRegion::Lot(crate::focus::LotLevel::RowActions {
                    studio,
                    chip,
                }) = self.focus.workspace.clone()
                {
                    self.panel_execute_chip(&studio, chip);
                } else {
                    self.panel_activate();
                }
            }
            KeyCode::Char('v') if plain => self.panel_sheet_open(),
            KeyCode::Char('o') if plain => self.panel_reopen_selected(),
            KeyCode::Char('p') if plain => self.panel_peek_selected(),
            KeyCode::Char('?') if plain => {
                self.help = true;
                self.help_state = crate::chrome::help::HelpState::default();
            }
            KeyCode::Char('s') if plain => self.panel_stage_start(),
            KeyCode::Char('r') if plain => {
                let targets = self.panel_targets();
                let idx = self.panel_selected_index(&targets);
                if let Some(Target::Person(k, role)) = targets.get(idx).cloned() {
                    let machines = self.panel_summaries();
                    let has_ask = crate::panel::data::studio(&machines, &k)
                        .and_then(|s| s.studio.roles.iter().find(|r| r.role == role))
                        .is_some_and(|r| r.asks.as_ref().is_some_and(|a| !a.is_empty()));
                    if has_ask {
                        self.enter_lot_compose(&k, crate::focus::TellTarget::Role(role));
                        return;
                    }
                }
                self.panel_reply_selected();
            }
            KeyCode::Char('a') if plain => self.panel_card_apply(),
            KeyCode::Char('x') if plain => {
                let targets = self.panel_targets();
                let idx = self.panel_selected_index(&targets);
                if matches!(targets.get(idx), Some(Target::Person(..))) {
                    self.panel_ack_selected();
                } else {
                    self.panel_card_cancel();
                }
            }
            KeyCode::Char('w') if plain => self.panel_switch_role_selected(),
            KeyCode::Char('f') if plain => self.panel_finish_selected(),
            KeyCode::Char(c @ ('y' | 'd')) if plain => self.panel_decide(c == 'y'),
            KeyCode::Char('n') if plain => {
                if self.panel.finish_confirm.is_some() {
                    self.panel.finish_confirm = None;
                } else {
                    let machine = self
                        .studio
                        .current
                        .as_ref()
                        .and_then(|k| k.machine.clone());
                    self.new_studio_open(machine);
                }
            }
            KeyCode::Char('i') if plain => self.panel_input_focus(false),
            KeyCode::Char('t') if plain => self.panel_input_focus(false),
            KeyCode::Char('/') if plain => self.panel_input_focus(true),
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
        let targets = self.panel_targets();
        let index = self.panel_selected_index(&targets);
        self.panel_activate_target(targets.get(index).cloned());
    }

    /// Activate an explicit `Target` — used both by keyboard Enter and
    /// left-clicks (where `Target::Block` was clicked before folding
    /// recomputed the block into `Target::Studio`).
    pub(super) fn panel_activate_target(&mut self, target: Option<Target>) {
        let machines = self.panel_summaries();
        match target {
            Some(Target::Studio(id)) => {
                self.studio.set_studio_focus(id.clone());
                self.last_current_studio = self.studio.current.clone();
                self.ns_pending_focus = None;
                self.land_studio_member(&id);
                self.enter_studio_in_lot(&id);
            }
            Some(Target::Strip(id)) => {
                self.focus_studio(&id);
                self.panel_leave();
            }
            Some(Target::Task(key, task)) => {
                self.sheet_open(key, Some(task));
            }
            Some(Target::OutwardCard(key, id)) => {
                self.run_action(studio_cmds::card_show_on(key.machine, &id));
            }
            Some(Target::Design(key)) => {
                self.design_read(&key);
            }
            Some(Target::Fold(id)) => {
                if !self.panel.accepted_folds.remove(&id) {
                    self.panel.accepted_folds.insert(id);
                }
                self.panel_reveal_selected();
            }
            Some(Target::ClosedFold(id)) => {
                if !self.panel.closed_folds.remove(&id) {
                    self.panel.closed_folds.insert(id);
                }
                self.panel_reveal_selected();
            }
            Some(Target::Stage(machine)) => {
                // The `s start` the row prints (U6c) — a click or
                // Enter on `stage stopped` is the same call.
                if self.can_start_stage(machine.as_deref())
                    && !self.machine_stage_running(machine.as_deref())
                {
                    self.run_action(studio_cmds::stage_start(machine));
                }
            }
            // R22: the `hollywood setup` hint is the in-TUI dialog's
            // button — same dialog the Call Sheet's row opens.
            Some(Target::Setup) => self.hw_switch_open(false),
            Some(Target::Talk(key, seq)) => {
                let file = crate::panel::data::studio(&machines, &key).and_then(|s| {
                    s.studio
                        .exchange
                        .iter()
                        .find(|e| e.seq == seq)
                        .and_then(|e| e.file.clone())
                });
                if let Some(rel) = file {
                    self.run_action(studio_cmds::report_show(&key, seq, &rel));
                }
            }
            // Selecting the `›` line is Resting — `Enter`/`i`/`/`/a
            // click arms `Compose` on it (R3-B2's explicit states).
            Some(Target::Input(_)) => {
                self.panel.editing = true;
            }
            // A finished studio's `↺ reopen producer [o]` line — `o`
            // owns the reopen; Enter never respawns a closed seat.
            Some(Target::ReopenProducer(key)) => {
                let studio = crate::panel::data::studio(&machines, &key);
                if studio.is_some() {
                    self.show_notice("Studio finished", "press o to reopen the producer");
                }
            }
            Some(Target::Block(id)) => {
                // A single-line block expands — the same manual
                // override Space writes; the selection follows the
                // block's new identity.
                self.panel.collapsed_blocks.remove(&id);
                self.panel.expanded_blocks.insert(id.clone());
                self.panel.selected = Some(Target::Studio(id));
                self.panel_reveal_selected();
            }
            Some(Target::Person(key, role_name)) => {
                let role = crate::panel::data::studio(&machines, &key)
                    .and_then(|s| s.studio.roles.iter().find(|r| r.role == role_name).cloned());
                if let Some(role) = role {
                    self.panel_person_enter(&role);
                }
            }
            _ => {}
        }
    }

    /// Enter on a person: a live one focuses its terminal — a remote
    /// one lands through the same machine-qualified address the
    /// sidebar uses; a closed one is never respawned by Enter —
    /// `o` owns the reopen (R3-M11's one-key-one-meaning).
    /// Enter on a person: a live one focuses its terminal and returns
    /// keyboard focus to Terminal (`[open ↵]`); a closed one is never
    /// respawned by Enter — `o` owns the reopen.
    pub(super) fn panel_person_enter(&mut self, role: &RoleSummary) {
        self.panel_person_focus_pane(role, true);
    }

    /// Focus a live person's pane in the main terminal, optionally
    /// leaving Lot focus for `Terminal` (`Enter` leaves, single-click
    /// keeps focus in Lot per Decision #2).
    pub(super) fn panel_person_focus_pane(&mut self, role: &RoleSummary, leave_lot: bool) {
        if role.state == "closed" {
            self.show_notice(
                "Person closed",
                &format!("{} is closed — press o to reopen", role.name),
            );
            return;
        }
        if role.state == "standby" {
            self.show_notice(
                "Terminal unavailable",
                &crate::studio::standby_unavailable(role),
            );
            return;
        }
        if role.state == "live"
            && let Some(terminal) = role.terminal.clone()
        {
            self.focus_pane_preserving_focus(terminal);
            if leave_lot {
                self.panel_leave();
            }
        }
    }
}
