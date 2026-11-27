// Derived from herdr src/client/shell/input.rs (Apache-2.0).
//! Source press/consumed/repeat/release policy, with hc attach generations.
use std::time::Instant;

use crossterm::event::{KeyCode, KeyModifiers};

use super::input::{Decision, TerminalKey};
use super::key_leases::{InputLease, InputLeaseKey, RepeatPlan};
use super::{App, Mode};
use hc_vt::{Action, Key};

#[path = "key_routing_modals.rs"]
mod key_routing_modals;

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
    hollywood: bool,
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
            hollywood: self.hollywood.expanded(),
        }
    }

    /// Resolve a studio-navigation key when it owns the press: studio
    /// keys work from anywhere while studios exist; role keys win only
    /// inside a studio workspace (or while a studio row or dock holds
    /// focus), so outside one herdr keeps its own bindings.
    fn studio_nav_for_key(
        &self,
        key: &crate::config::TerminalKey,
    ) -> Option<crate::panel::StudioNav> {
        if !self.studio.active || crate::panel::data::studios(&self.panel_summaries()).is_empty() {
            return None;
        }
        let nav = self.panel_config.keys.nav(key)?;
        let role_key = matches!(
            nav,
            crate::panel::StudioNav::PreviousRole | crate::panel::StudioNav::NextRole
        );
        (!role_key
            || self.studio.lot.studio_workspace
            || self.studio.focus.is_some()
            || self.panel_focused())
        .then_some(nav)
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
                    self.dead_pane_handoff();
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
                // The release of a press the image bridge or a float
                // binding consumed never reaches a program — including
                // the popup, which eats releases unconditionally while
                // open.
                if !self.image_paste_suppress.remove(&key.identity())
                    && !self.popup_key_suppress.remove(&key.identity())
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
        self.sync_focus_before_key();
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
        // The floating popup eats every key while open — the reference
        // routes raw input to it before UI bindings (`popup.rs`), and a
        // shown float takes even the Ctrl+G lock chord and keeps the keys
        // while the keyboard is locked, so nothing lands on the pane
        // behind it; its own popup chord still works when locked, so the
        // float can always be hidden and the lock reached again. A retained popup selection copies on Ctrl/Cmd+C
        // first; every other key reaching the popup clears it. A
        // persistent float yields only its popup binding chords: they run
        // their command, so its own toggle key hides the float rather
        // than typing into the program. Plain page keys page the popup's
        // own scrollback under the same rule as a pane's
        // (`page_keys.rs`); any key the program does get jumps the view
        // back to the bottom. Intercepted presses record their identity
        // so the matching release never reaches a program that saw no
        // press.
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
            let intercepted = self
                .popup
                .as_ref()
                .is_some_and(|popup| popup.persistent)
                .then(|| crate::input::resolve_direct_binding(&self.live.keybinds, &popup_combo))
                .flatten();
            match intercepted {
                Some(crate::input::KeybindMatch::Command(command))
                    if command.action == crate::config::CustomCommandAction::Popup =>
                {
                    self.popup_key_suppress.insert(press.identity());
                    self.run_custom_command(&command);
                }
                _ => {
                    if self
                        .popup
                        .as_mut()
                        .is_some_and(|popup| popup.pane.page_scroll(key))
                    {
                        self.popup_key_suppress.insert(press.identity());
                        self.sched.mark_dirty(Instant::now());
                    } else {
                        if let Some(vt) = self.popup.as_mut().and_then(|popup| popup.pane.vt()) {
                            vt.scroll_to_bottom();
                        }
                        self.popup_key(&press);
                    }
                }
            }
            return None;
        }
        // The reference's Ctrl+G arm precedes the lock:
        // allowed only when already locked or in bare `WorkspaceRegion::Terminal`
        // with no stacked modals (`FocusState::allows_keyboard_lock`).
        if key.input.key == Key::Char('g')
            && key.input.mods == hc_vt::Mods::CTRL
            && (self.keyboard_locked || self.focus.allows_keyboard_lock())
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
        // A popup still opening swallows input — the reference's
        // `popup_pending` arm.
        if self.popup_pending.is_some() {
            return None;
        }
        let text_input_modal = self.dialog.is_some()
            || self.studio_dialog.is_some()
            || self.new_studio.is_some()
            || self.hw_switch.is_some()
            || self.settings.is_some()
            || self.update_modal.is_some();
        if !text_input_modal
            && !matches!(self.mode, Mode::Copy | Mode::Resize)
            && press.input.action == hc_vt::Action::Press
            && let Some(nav) = self.studio_nav_for_key(&crate::input::terminal_key_of(&press))
        {
            self.peek = None;
            self.menu = None;
            self.sheet = None;
            self.help = false;
            self.navigator = None;
            if self.mode == Mode::Navigate {
                self.mode = Mode::Normal;
                self.navigate_target = None;
            }
            self.prefix.reset();
            self.prefix.swallow(&press);
            // `studio_nav` releases the overlays itself so a studio step
            // can keep the dock or the tree in charge of the keyboard.
            self.studio_nav(nav);
            return None;
        }
        if self.peek_key(&press) {
            return None;
        }
        // Every open modal owns the key before pane/region dispatch;
        // `Esc` inside restores the underlying `WorkspaceRegion`.
        if self.modal_key(&press, width) {
            self.sync_focus();
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
            if !matches!(
                self.focus.workspace,
                crate::focus::WorkspaceRegion::Terminal
            ) || crate::input::resolve_prefix_binding(&self.live.keybinds, &matched).is_none()
            {
                self.prefix.reset();
                self.prefix.swallow(&press);
                self.panel_toggle();
                self.sync_focus();
                return None;
            }
        }
        // `prefix+shift+<dock key>` is the dock's close chord (S2 §4) —
        // it fires only while a dock is open, so a herdr binding on
        // the same tail keeps working in a dock-less client.
        if self.prefix.armed() && self.panel_close_press(&press) {
            self.prefix.reset();
            self.prefix.swallow(&press);
            self.panel_close();
            self.sync_focus();
            return None;
        }
        // `Ctrl+B m` from a focused pane opens that pane's studio
        // menu — a role pane's role menu, a shell pane's studio menu
        // (S3 §1).
        if self.prefix.armed() && self.studio.active {
            let matched = crate::input::terminal_key_of(&press);
            if matched.code == crossterm::event::KeyCode::Char('m') && matched.modifiers.is_empty()
            {
                self.prefix.reset();
                self.prefix.swallow(&press);
                self.pane_menu();
                self.sync_focus();
                return None;
            }
        }
        // W1b: `Ctrl+B t` (`[keys] focus_tree`) focuses the left machines tree.
        if self.prefix.armed() && self.tree_focus_press(&press) {
            let matched = crate::input::terminal_key_of(&press);
            if !matches!(
                self.focus.workspace,
                crate::focus::WorkspaceRegion::Terminal
            ) || crate::input::resolve_prefix_binding(&self.live.keybinds, &matched).is_none()
            {
                self.prefix.reset();
                self.prefix.swallow(&press);
                self.tree_focus_enter();
                self.sync_focus();
                return None;
            }
        }
        // `Ctrl+B a` (`[keys] focus_cast`) focuses the left Cast section.
        if self.prefix.armed() && self.cast_focus_press(&press) {
            let matched = crate::input::terminal_key_of(&press);
            if !matches!(
                self.focus.workspace,
                crate::focus::WorkspaceRegion::Terminal
            ) || crate::input::resolve_prefix_binding(&self.live.keybinds, &matched).is_none()
            {
                self.prefix.reset();
                self.prefix.swallow(&press);
                self.cast_focus_enter();
                self.sync_focus();
                return None;
            }
        }
        if !self.prefix.armed() && self.tree_focus_direct_press(&press) {
            self.prefix.reset();
            self.prefix.swallow(&press);
            self.tree_focus_enter();
            self.sync_focus();
            return None;
        }
        if !self.prefix.armed() && self.cast_focus_direct_press(&press) {
            self.prefix.reset();
            self.prefix.swallow(&press);
            self.cast_focus_enter();
            self.sync_focus();
            return None;
        }
        // U1: studio keys win over herdr direct bindings from anywhere
        // in studio mode; role keys win only inside a studio workspace.
        if let Some(nav) = self.studio_nav_for_key(&crate::input::terminal_key_of(&press)) {
            self.prefix.reset();
            self.prefix.swallow(&press);
            self.studio_nav(nav);
            self.sync_focus();
            return None;
        }
        // H4: Hollywood Call Sheet toggle (`alt+y` / `Ctrl+B i`). Works across
        // all non-modal workspace regions (`Terminal`, `Tree`, `Cast`, `Lot`, `CallSheet`).
        if self.hollywood_key_press(&press) {
            self.prefix.reset();
            self.prefix.swallow(&press);
            self.hollywood_toggle();
            self.sync_focus();
            return None;
        }
        // The stopped-stage toast's `s` offer outranks region keys.
        if self.action_toast_start_key(&press) {
            self.sync_focus();
            return None;
        }
        // `focus_lot` (`alt+l`): evaluated BEFORE `CallSheet` / `Lot` / `Tree` / `Cast`
        // dispatch so `Alt+L` jumps cleanly between regions and enters studios,
        // while still yielding to `hollywood.view` (Reader) and active compose drafts.
        if self.hollywood.view.is_none() && self.focus_lot_press(&press) {
            self.prefix.reset();
            self.prefix.swallow(&press);
            if key.input.action == Action::Press {
                self.focus_lot();
            }
            self.sync_focus();
            return None;
        }
        // Non-terminal region dispatch — prefix arming stays uniform across
        // Tree, Cast, Lot, and CallSheet so `Ctrl+B <key>` chords stay reachable.
        if self.hollywood.view.is_some()
            || matches!(
                self.focus.workspace,
                crate::focus::WorkspaceRegion::CallSheet(_)
            )
            || self.hollywood.expanded()
        {
            self.handle_hollywood_key(&press);
            self.sync_focus();
            return None;
        }
        if matches!(self.focus.workspace, crate::focus::WorkspaceRegion::Tree) {
            if self.prefix.armed() {
                self.prefix.reset();
                self.prefix.swallow(&press);
                return None;
            }
            if self
                .live
                .matches_prefix(&crate::input::terminal_key_of(&press))
            {
                let _ = self.prefix.feed(&press, &self.live);
                return None;
            }
            self.handle_tree_key(&press);
            self.sync_focus();
            return None;
        }
        if matches!(self.focus.workspace, crate::focus::WorkspaceRegion::Cast) {
            if self.prefix.armed() {
                self.prefix.reset();
                self.prefix.swallow(&press);
                return None;
            }
            if self
                .live
                .matches_prefix(&crate::input::terminal_key_of(&press))
            {
                let _ = self.prefix.feed(&press, &self.live);
                return None;
            }
            self.handle_cast_key(&press);
            self.sync_focus();
            return None;
        }
        if matches!(self.focus.workspace, crate::focus::WorkspaceRegion::Lot(_))
            || self.panel_focused()
        {
            self.handle_panel_key(&press);
            self.sync_focus();
            return None;
        }
        // The failure toast's own keys (§3.3): `C` writes the exact
        // command through OSC 52, `Esc` dismisses — only while the
        // card is up and no modal above took the press.
        if self.action_toast_key(&press) {
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
                    // S1: the stage log is read-only — no key ever
                    // reaches the stage process (Ctrl+C included).
                    Decision::ToTerm(_) if self.stage_terminal_focused() => None,
                    // U2: the opening page owns every remaining key —
                    // direct bindings and prefix chords already ran,
                    // and nothing reaches the terminals beneath.
                    Decision::ToTerm(_) if self.opening_visible() => {
                        self.opening_key(&press);
                        None
                    }
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
                if self.pane.accepts_route(route)
                    && !self.stage_terminal_focused()
                    && !self.pane_page_scroll(&key)
                {
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
    pub(super) fn resize_key(&mut self, key: &TerminalKey, width: u16) {
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
