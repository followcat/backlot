//! The modal interception chain — every overlay that owns all keys
//! while open, checked in order before pane dispatch.

use super::super::input::TerminalKey;
use super::super::{App, Mode};

impl App {
    /// `true` when a modal owned the key: herdr's dialog → the S3
    /// native dialogs → New Studio → the Task & Delivery Sheet →
    /// the context menu → resize/help/navigator → the update card →
    /// settings. `Esc` inside any of them restores the owner's
    /// focus; nothing reaches a program.
    pub(super) fn modal_key(&mut self, key: &TerminalKey, width: u16) -> bool {
        if self.dialog.is_some() {
            self.dialog_key(key);
            return true;
        }
        // The S3 native dialogs own every key while open — `Esc`
        // always cancels cleanly (A6.2).
        if self.studio_dialog.is_some() {
            self.studio_dialog_key(key);
            return true;
        }
        // The New Studio dialog owns every key while it is open —
        // Esc steps back or closes, nothing reaches a program.
        if self.new_studio.is_some() {
            self.new_studio_key(key);
            return true;
        }
        // The Hollywood setup dialog owns every key while it is
        // open — even while the Call Sheet stays expanded under it.
        if self.hw_setup.is_some() {
            self.hw_setup_key(key);
            return true;
        }
        // The Task & Delivery Sheet owns every key while it is open
        // — tabs, movement, `o` and `Esc` all run inside it (S2 §3).
        if self.sheet.is_some() {
            self.handle_sheet_key(key);
            return true;
        }
        if self.menu.is_some() {
            self.menu_key(key);
            return true;
        }
        if self.mode == Mode::Resize {
            self.resize_key(key, width);
            return true;
        }
        if self.help {
            self.handle_help_key(key);
            return true;
        }
        if self.navigator.is_some() {
            self.handle_navigator_key(key);
            return true;
        }
        if self.update_modal.is_some() {
            self.handle_update_key(key);
            return true;
        }
        self.route_settings_key(key)
    }
}
