// Derived from herdr src/client/shell/input.rs (Apache-2.0).
//! Source press/consumed/repeat/release policy, with hc attach generations.
use std::time::Instant;

use crossterm::event::{KeyCode, KeyModifiers};

use super::input::{Decision, TerminalKey};
use super::key_leases::{InputLease, InputLeaseKey, RepeatPlan};
use super::{App, Mode};
use hc_vt::{Action, Key};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Context {
    mode: Mode,
    prefix: bool,
    help: bool,
    locked: bool,
    dialog: bool,
    menu: bool,
    navigator: bool,
    settings: bool,
    panel: bool,
}

impl App {
    fn key_context(&self) -> Context {
        Context {
            mode: self.mode.clone(),
            prefix: self.prefix.armed(),
            help: self.help,
            locked: self.keyboard_locked,
            dialog: self.dialog.is_some(),
            menu: self.menu.is_some(),
            navigator: self.navigator.is_some(),
            settings: self.settings.is_some(),
            panel: self.panel_focused(),
        }
    }

    pub(super) fn on_key(&mut self, key: TerminalKey, width: u16) {
        if let Some(text) = self.host_text_commit(&key) {
            // `prepare_committed_text`: committed text retires the word
            // gesture and clears a retained selection.
            self.word_selection = None;
            if self.selection.take().is_some() {
                self.stop_selection_autoscroll();
                self.selection_highlight_clear_deadline = None;
                self.sched.mark_dirty(Instant::now());
            }
            self.reset_displayed_scroll();
            self.pane.input_encoded(text.into_bytes());
            return;
        }
        let mut key = self.host_erase_key(key);
        if key.input.action == Action::Press {
            key.dispatch_id = self.next_dispatch_id;
            self.next_dispatch_id += 1;
        }
        if self.host_reports_key_releases && key.text_commit.is_some() {
            key.physical_identity_hint = true;
        }
        let lease_key = InputLeaseKey::new(0, &key);
        let previous = self
            .pane_leases
            .forwarded_for(&lease_key)
            .map(|lease| self.lease_route(lease));
        let key = self.pane_leases.normalize_press(&lease_key, key);
        if let Some(route) = previous {
            self.unpark_drained_route(route);
        }
        match key.input.action {
            Action::Press => {
                let initial = self.key_context();
                let mut target = self.route_key_press(&key, width);
                if target.is_some() && self.pane_page_scroll(&key) {
                    // A plain page key paged the pane's own scrollback
                    // instead of reaching the program; its release is
                    // swallowed with the press's consumed lease.
                    target = None;
                }
                if target.is_some() {
                    self.reset_displayed_scroll();
                }
                if target.is_some() && !self.pane.key(&key) {
                    target = None;
                }
                let resulting = self.key_context();
                let plan = self.pane_leases.complete_press(
                    lease_key,
                    &key,
                    Some(&initial),
                    Some(&resulting),
                    target,
                );
                self.execute_repeat(lease_key, key, plan, width);
            }
            Action::Repeat => {
                let context = self.key_context();
                let plan = self
                    .pane_leases
                    .plan_repeat(lease_key, &key, Some(&context));
                self.execute_repeat(lease_key, key, plan, width);
            }
            Action::Release => {
                // The release of a press the image bridge consumed never
                // reaches a program — including the popup, which eats
                // releases unconditionally while open.
                if !self.image_paste_suppress.remove(&key.identity())
                    && let Some(popup) = self.popup.as_mut()
                {
                    popup.pane.key(&key);
                }
                self.release_pane_lease(&key);
            }
        }
    }

    pub(super) fn reconcile_key_routes(&mut self) {
        for lease in self.pane_leases.forwarded_mut() {
            if let Some(route) = self.pane.dispatched_route(&lease.key) {
                lease.target = route;
            }
        }
        for route in self.pane.parked_routes() {
            self.unpark_drained_route(route);
        }
    }

    fn route_key_press(&mut self, key: &TerminalKey, width: u16) -> Option<u64> {
        // Reprocessed repeats enter the same binding dispatcher. Only the key
        // sent to the program retains Repeat; routing itself tests the chord.
        let press = key.clone().with_action(Action::Press);
        // The clipboard-image trigger precedes every consumer, popup
        // included: a matching press either starts an upload (consumed)
        // or, with no image on the clipboard, continues into normal
        // routing byte-for-byte. Re-dispatched repeats of a consumed
        // press stop here. Copy mode keeps the key for its own dispatch —
        // the reference's modal-paste arm never pastes into the pane.
        if self.copy_or_terminal_mode() != Mode::Copy && self.route_image_paste_key(key).is_some() {
            return None;
        }
        // The reference's Ctrl+G arm precedes the popup and the lock:
        // copy mode exits without copying, selections clear, then the
        // lock toggles.
        if key.input.key == Key::Char('g')
            && key.input.mods == hc_vt::Mods::CTRL
            && (self.keyboard_locked
                || (!self.help
                    && self.dialog.is_none()
                    && self.menu.is_none()
                    && self.navigator.is_none()
                    && self.settings.is_none()))
        {
            if key.input.action == Action::Press {
                self.exit_copy_mode(false);
                self.mode = Mode::Normal;
                self.navigate_target = None;
                self.selection = None;
                self.clear_popup_selection();
                self.stop_selection_autoscroll();
                self.keyboard_locked = !self.keyboard_locked;
                self.prefix.reset();
            }
            return None;
        }
        if self.keyboard_locked {
            self.clear_popup_selection();
            return self.pane.target().map(|_| self.pane.generation());
        }
        // The floating popup eats every key while open — the reference
        // routes raw input to it before UI bindings (`popup.rs`). A
        // retained popup selection copies on Ctrl/Cmd+C first; every
        // other key reaching the popup clears it.
        if self.popup.is_some() {
            let popup_combo = crate::input::terminal_key_of(&press);
            if matches!(popup_combo.code, KeyCode::Char('c' | 'C'))
                && matches!(
                    popup_combo.modifiers,
                    KeyModifiers::CONTROL | KeyModifiers::SUPER
                )
                && self.copy_popup_selection()
            {
                self.clear_popup_selection();
                return None;
            }
            self.clear_popup_selection();
            self.popup_key(&press);
            return None;
        }
        // A popup still opening swallows input — the reference's
        // `popup_pending` arm.
        if self.popup_pending.is_some() {
            return None;
        }
        if self.peek_key(&press) {
            return None;
        }
        if self.dialog.is_some() {
            self.dialog_key(&press);
            return None;
        }
        if self.menu.is_some() {
            self.menu_key(&press);
            return None;
        }
        if self.mode == Mode::Resize {
            self.resize_key(&press, width);
            return None;
        }
        if self.help {
            self.handle_help_key(&press);
            return None;
        }
        if self.navigator.is_some() {
            self.handle_navigator_key(&press);
            return None;
        }
        if self.route_settings_key(&press) {
            return None;
        }
        // The reference clears the word gesture on any key and lets a
        // retained (non `copy_on_select`) selection copy on Ctrl/Cmd+C;
        // every other key reaching pane dispatch clears the highlight.
        // Both arms stay out of Copy mode — the copy-mode selection and
        // its search state survive until its own keys act.
        self.word_selection = None;
        let outside_copy = self.mode != Mode::Copy && self.copy_or_terminal_mode() != Mode::Copy;
        if outside_copy
            && !self.shell.copy_on_select
            && matches!(press.input.key, Key::Char('c' | 'C'))
            && matches!(press.input.mods, m if m == hc_vt::Mods::CTRL || m == hc_vt::Mods::SUPER)
            && self
                .selection
                .as_ref()
                .is_some_and(crate::selection::Selection::is_visible)
        {
            self.request_selection_copy();
            self.selection = None;
            self.stop_selection_autoscroll();
            self.selection_highlight_clear_deadline = None;
            self.sched.mark_dirty(Instant::now());
            return None;
        }
        if outside_copy && self.selection.take().is_some() {
            self.stop_selection_autoscroll();
            self.selection_highlight_clear_deadline = None;
            self.sched.mark_dirty(Instant::now());
        }
        if self.mode == Mode::Navigate {
            if let Some(nav) = super::input::navigate_key(&press, &self.live) {
                self.on_navigate(nav);
            }
            return None;
        }
        // The reference's `ClientShellMode::Copy` arm: the prefix keys out
        // to the prefix machine only while no search prompt is open;
        // every other key belongs to copy mode alone. This precedes the
        // panel arms so copy mode owns every key while it is on — an open
        // search prompt takes even the chord's keys.
        if self.mode == Mode::Copy {
            if self
                .copy_mode
                .as_ref()
                .is_none_or(|copy_mode| copy_mode.search_prompt.is_none())
                && self
                    .live
                    .matches_prefix(&crate::input::terminal_key_of(&press))
            {
                self.mode = Mode::Normal;
                let _ = self.prefix.feed(&press, &self.live);
                return None;
            }
            self.route_copy_mode_key(&press);
            return None;
        }
        // `prefix+<panel.key>` is the studio panel's own chord — it
        // fires whether the panel is closed, open or focused. A herdr
        // binding on the same tail still wins while the panel does not
        // own the keys. The chord sits ahead of the focused-panel
        // catch-all so the toggle stays reachable while the panel owns
        // every other key.
        if self.prefix.armed() && self.panel_key_press(&press) {
            let matched = crate::input::terminal_key_of(&press);
            if self.panel_focused()
                || crate::input::resolve_prefix_binding(&self.live.keybinds, &matched).is_none()
            {
                self.prefix.reset();
                self.prefix.swallow(&press);
                self.panel_toggle();
                return None;
            }
        }
        // A focused panel owns every key like the navigator does; the
        // prefix itself still arms so the toggle chord stays reachable.
        if self.panel_focused() {
            self.handle_panel_key(&press);
            return None;
        }
        match (!self.prefix.armed())
            .then(|| {
                crate::input::resolve_direct_binding(
                    &self.live.keybinds,
                    &crate::input::terminal_key_of(&press),
                )
            })
            .flatten()
        {
            Some(crate::input::KeybindMatch::Action(action)) => {
                self.on_action(action, width);
                None
            }
            Some(crate::input::KeybindMatch::Command(command)) => {
                self.run_custom_command(&command);
                None
            }
            None => {
                let target = match self.prefix.feed(&press, &self.live) {
                    Decision::Action(action) => {
                        self.on_action(action, width);
                        None
                    }
                    Decision::Command(command) => {
                        self.run_custom_command(&command);
                        None
                    }
                    Decision::Held => None,
                    Decision::ToTerm(forwarded)
                        if matches!(forwarded.input.key, Key::Functional(_)) =>
                    {
                        // The pinned Linux thin-client consumes these reports;
                        // actual reference lock/keypad/media captures produce no
                        // program bytes. Keep their typed shape out of text paths.
                        None
                    }
                    Decision::ToTerm(forwarded) => {
                        debug_assert_eq!(forwarded.input, press.input);
                        self.pane.target().map(|_| self.pane.generation())
                    }
                };
                // The reference's Prefix `return_mode`: every exit from
                // the prefix machine re-arms Copy when its pane is still
                // focused (`sync_copy_focus` no-ops while armed).
                self.sync_copy_focus();
                target
            }
        }
    }

    fn execute_repeat(
        &mut self,
        lease_key: InputLeaseKey<u8>,
        key: TerminalKey,
        plan: RepeatPlan<Context, u64>,
        width: u16,
    ) {
        match plan {
            RepeatPlan::Forwarded(target) => {
                let route = self
                    .pane_leases
                    .forwarded_for(&lease_key)
                    .map(|lease| self.lease_route(lease))
                    .unwrap_or(target);
                if self.pane.accepts_route(route) && !self.pane_page_scroll(&key) {
                    self.reset_displayed_scroll();
                    self.pane.key(&key);
                }
            }
            RepeatPlan::Reprocess {
                context,
                repetitions,
                tracked,
            } => {
                for _ in 0..repetitions {
                    let current = self.key_context();
                    if !self.pane_leases.reprocess_allowed(
                        lease_key,
                        &context,
                        Some(&current),
                        tracked,
                    ) {
                        break;
                    }
                    let repeated = key.clone().with_action(Action::Repeat);
                    if self.route_key_press(&repeated, width).is_some()
                        && !self.pane_page_scroll(&repeated)
                    {
                        self.reset_displayed_scroll();
                        self.pane.key(&repeated);
                    }
                }
            }
            RepeatPlan::Ignore => {}
        }
    }

    pub(super) fn take_pane_lease(
        &mut self,
        key: &TerminalKey,
    ) -> Option<InputLease<Context, u64>> {
        if let Some(lease) = self.pane_leases.remove(&InputLeaseKey::new(0, key)) {
            return Some(lease);
        }
        let Key::Char(c) = key.identity() else {
            return None;
        };
        fn single(mut chars: impl Iterator<Item = char>) -> Option<char> {
            match (chars.next(), chars.next()) {
                (Some(c), None) => Some(c),
                _ => None,
            }
        }
        let other = if c.is_lowercase() {
            single(c.to_uppercase())
        } else {
            single(c.to_lowercase())
        }
        .filter(|other| *other != c);
        for candidate in key.shifted_codepoint.into_iter().chain(other) {
            let alternate = TerminalKey::new(hc_vt::KeyInput {
                key: Key::Char(candidate),
                mods: key.input.mods,
                action: key.input.action,
            });
            if let Some(lease) = self.pane_leases.remove(&InputLeaseKey::new(0, &alternate)) {
                return Some(lease);
            }
        }
        self.pane_leases.remove_sole_text_press(0)
    }
}

impl App {
    fn resize_key(&mut self, key: &TerminalKey, width: u16) {
        use crossterm::event::KeyCode;
        let combo = crate::input::terminal_key_of(key);
        let binding = &self.live.keybinds.resize_mode;
        if matches!(combo.code, KeyCode::Esc | KeyCode::Enter)
            || binding.matches_prefix_key(&combo)
            || binding.matches_direct_key(&combo)
        {
            self.mode = Mode::Normal;
            return;
        }
        use crate::input::KeybindAction as A;
        let action = match combo.code {
            KeyCode::Left | KeyCode::Char('h') => Some(A::ResizePaneLeft),
            KeyCode::Down | KeyCode::Char('j') => Some(A::ResizePaneDown),
            KeyCode::Up | KeyCode::Char('k') => Some(A::ResizePaneUp),
            KeyCode::Right | KeyCode::Char('l') => Some(A::ResizePaneRight),
            _ => None,
        };
        if let Some(action) = action {
            self.on_action(action, width);
        }
    }
}
