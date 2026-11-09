// Derived from herdr src/pane/terminal.rs and src/server/pane_input.rs
// (Apache-2.0).
//! Plain PageUp/PageDown page the pane's own scrollback when the
//! program's state says the keys are not application input. The
//! reference applies this per event in `apply_client_terminal_input_events`:
//! presses and repeats scroll one pane height, releases are swallowed,
//! and anything else encodes for the program.
use super::*;

impl Pane {
    /// The reference's `plain_page_keys_use_host_scrollback`: main
    /// screen, no mouse reporting, and not the bare application-cursor
    /// mode a pager uses — bracketed paste marks a line editor, where
    /// the reference still pages its own scrollback. Unknown mode
    /// state fails closed like the reference's `.ok()?` chain, so the
    /// key forwards like it did before.
    pub(crate) fn plain_page_keys_use_host_scrollback(&self) -> bool {
        let Some(vt) = self.vt.as_ref() else {
            return false;
        };
        let Ok(alternate_screen) = vt.alt_screen() else {
            return false;
        };
        let Ok(application_cursor) = vt.mode(1) else {
            return false;
        };
        let Ok(bracketed_paste) = vt.mode(2004) else {
            return false;
        };
        !alternate_screen && !vt.mouse_tracking() && (!application_cursor || bracketed_paste)
    }

    /// A pane-bound key the reference converts to host scrollback:
    /// `true` once the viewport moved — the caller treats the key as
    /// UI-consumed (its release is swallowed, its repeats reprocess and
    /// page again). Modified keys, releases and program-eligible keys
    /// report `false` and forward normally.
    pub(crate) fn page_scroll(&mut self, key: &crate::app::input::TerminalKey) -> bool {
        if !matches!(
            key.input.action,
            hc_vt::Action::Press | hc_vt::Action::Repeat
        ) || key.input.mods != hc_vt::Mods::NONE
            || !self.plain_page_keys_use_host_scrollback()
        {
            return false;
        }
        let direction: isize = match key.input.key {
            hc_vt::Key::PageUp => -1,
            hc_vt::Key::PageDown => 1,
            _ => return false,
        };
        let Some(vt) = self.vt.as_mut() else {
            return false;
        };
        let rows = vt.size().map(|(_, rows)| rows).unwrap_or(1).max(1) as isize;
        vt.scroll_delta(direction * rows);
        true
    }
}
