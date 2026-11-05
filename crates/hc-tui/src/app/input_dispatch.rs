// Derived from herdr src/client/shell/input.rs and composition.rs (Apache-2.0).
use super::*;

impl App {
    /// One neutral input event.
    pub(super) fn on_input(&mut self, ev: UiEvent, width: u16) {
        match ev {
            UiEvent::Resize { cols, rows } => {
                self.area = (cols, rows);
                if let Some(geometry) = crate::mouse_geometry::HostGeometry::current() {
                    let pitch = (
                        geometry.width_px / u32::from(geometry.cols),
                        geometry.height_px / u32::from(geometry.rows),
                    );
                    if pitch.0 > 0 && pitch.1 > 0 {
                        self.on_host_reply(crate::host::HostReply::CellSize {
                            width_px: pitch.0,
                            height_px: pitch.1,
                        });
                    }
                }
            }
            UiEvent::Focus(focused) => {
                self.outer_focused = Some(focused);
                if focused {
                    let _ = (self.host_control)(crate::host::SCHEME_QUERY.as_bytes());
                    self.mark_active_seen();
                } else {
                    self.release_pane_leases();
                }
                self.pane.focused(focused);
            }
            UiEvent::Paste(text) => {
                // Paste is never parsed as UI keys.
                if self.help {
                    if self.help_state.search_focused
                        && self
                            .help_state
                            .query
                            .insert(&String::from_utf8_lossy(&text))
                    {
                        self.help_state.scroll = 0;
                    }
                } else if self.mode == Mode::Normal && !self.prefix.armed() {
                    self.pane.paste(&text);
                }
            }
            UiEvent::Key(key) => self.on_key(key, width),
            UiEvent::Text(text) => {
                // IME/commit text: bytes only, never a UI binding.
                if self.help {
                    if self.help_state.search_focused && self.help_state.query.insert(&text) {
                        self.help_state.scroll = 0;
                    }
                } else if self.mode == Mode::Normal && !self.prefix.armed() {
                    self.pane.input_encoded(text.into_bytes());
                }
            }
            UiEvent::Mouse(mouse) => self.on_host_mouse(mouse),
            UiEvent::Unsupported => {}
            UiEvent::InputRejected(reason) => self.reject_input(reason),
        }
    }

    /// Derived from herdr src/client/shell/input.rs `host_text_commit`
    /// (Apache-2.0): while the host reports every key as an escape
    /// code, plain arriving text is an IME/compose commit — forward it
    /// as text, never as invented key reports. A legacy host keeps
    /// only non-ASCII text on this route; non-ASCII has no key.
    pub(super) fn host_text_commit(&self, key: &input::TerminalKey) -> Option<String> {
        if !self.host_reports_all_keys
            || key.input.action != hc_vt::Action::Press
            || self.mode != Mode::Normal
            || self.help
        {
            return None;
        }
        let plain_text = key.vt_bytes.first().is_some_and(|b| *b != 0x1b);
        // Our `Mode::Normal` includes the armed prefix — the reference's
        // `mode != Terminal` check covers its separate Prefix mode, so
        // the armed prefix must also opt out (#5255 direction reversed:
        // Ctrl+B then plain text still runs bindings).
        if self.prefix.armed() {
            return None;
        }
        let text = key.text_commit.as_ref().filter(|_| plain_text)?;
        (self.host_reports_key_releases || !text.is_ascii()).then(|| text.clone())
    }

    /// `shell_runtime.rs::sync_client_shell_keyboard_report_all`: the
    /// host push escalates to report-all while the pane negotiates it
    /// or a binding mode is active — pop + push, same base flags.
    pub(super) fn sync_keyboard_report_all(&mut self) {
        const REPORT_ALL: u8 = 0b0000_1000;
        let pane_wants = self
            .pane
            .vt()
            .and_then(|vt| vt.kitty_flags().ok())
            .is_some_and(|f| f & REPORT_ALL != 0);
        let desired = pane_wants || self.mode == Mode::Navigate || self.prefix.armed();
        if desired == self.host_reports_all_keys {
            return;
        }
        let mut sequence = b"\x1b[<u".to_vec(); // pop
        sequence.extend_from_slice(if desired {
            // base 1|2|4 + REPORT_ALL_KEYS (8) + ASSOCIATED_TEXT
            // (16) = 31: `terminal_modes.rs`.
            b"\x1b[>31u".as_slice()
        } else {
            // `ime_compatible_keyboard_enhancement_flags` = 7.
            b"\x1b[>7u".as_slice()
        });
        if (self.host_control)(&sequence).is_ok() {
            self.host_reports_all_keys = desired;
        }
    }

    /// Injects the host-mode control sink (keyboard pop/push bytes).
    /// The default is inert so tests never touch a real terminal.
    pub fn set_host_control(&mut self, sink: HostControlSink) {
        self.host_control = sink;
        self.last_window_title = None;
    }

    /// `set_host_erase_byte`: only a measured `^H` VERASE arms the
    /// Backspace rewrite — never assumed.
    pub fn set_host_erase_byte(&mut self, erase: Option<u8>) {
        self.host_erase_is_ctrl_h = erase == Some(0x08);
    }

    /// Derived from herdr src/client/shell/input.rs `host_erase_key`
    /// (Apache-2.0): a raw 0x08 (or ESC 0x08) from a `^H`-erase host is
    /// its Backspace key — reissued with kind and raw bytes preserved.
    pub(super) fn host_erase_key(&self, key: input::TerminalKey) -> input::TerminalKey {
        if !self.host_erase_is_ctrl_h {
            return key;
        }
        let mods = match key.vt_bytes.as_slice() {
            [0x08] => hc_vt::Mods::NONE,
            [0x1b, 0x08] => hc_vt::Mods::ALT,
            _ => return key,
        };
        input::TerminalKey::new(hc_vt::KeyInput {
            key: hc_vt::Key::Backspace,
            mods,
            action: key.input.action,
        })
        .with_vt_bytes(key.vt_bytes)
    }
}
