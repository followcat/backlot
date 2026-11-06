// Derived from herdr src/client/shell/input.rs and src/terminal_theme.rs (Apache-2.0).
//! Host appearance precedence, capability replies and measured display facts.

use super::App;
use crate::chrome::palette::{HostAppearance, resolve_effective_theme};
use crate::host::{HostFacts, HostReply};

fn inferred_appearance(color: hc_vt::Rgb) -> HostAppearance {
    let luminance = u32::from(color.r) * 299 + u32::from(color.g) * 587 + u32::from(color.b) * 114;
    if luminance >= 128_000 {
        HostAppearance::Light
    } else {
        HostAppearance::Dark
    }
}

impl App {
    fn apply_appearance(&mut self, appearance: HostAppearance) {
        self.host_appearance = Some(appearance);
        if self.shell.theme_runtime.auto_switch {
            let (palette, name) =
                resolve_effective_theme(&self.shell.theme_runtime, self.host_appearance);
            self.shell.palette = palette;
            self.shell.theme_name = name;
        }
    }

    pub fn set_host_facts(&mut self, facts: HostFacts) {
        if !self.host_appearance_explicit
            && self.host_facts.theme.background != facts.theme.background
            && let Some(color) = facts.theme.background
        {
            self.apply_appearance(inferred_appearance(color));
        }
        self.host_facts = facts.clone();
        self.pane.set_host_facts(facts.clone());
        for pane in self
            .observers
            .values_mut()
            .chain(self.size_leases.values_mut())
        {
            pane.set_host_facts(facts.clone());
        }
    }

    pub(super) fn on_host_reply(&mut self, reply: HostReply) {
        match reply {
            HostReply::KeyboardFlags(flags) if !self.probe_device_attributes => {
                self.probe_keyboard_flags = Some(flags);
            }
            HostReply::PrimaryDeviceAttributes => self.probe_device_attributes = true,
            HostReply::SgrPixelMouse(supported) => self.host_sgr_pixel_mouse = Some(supported),
            HostReply::Dark | HostReply::Light => {
                self.host_appearance_explicit = true;
                self.apply_appearance(if reply == HostReply::Light {
                    HostAppearance::Light
                } else {
                    HostAppearance::Dark
                });
                // A color-scheme report also refreshes the defaults/palette,
                // even when UI theme switching was disabled by configuration.
                let _ = (self.host_control)(crate::host::color_query_sequence().as_bytes());
            }
            HostReply::Background(color) if !self.host_appearance_explicit => {
                self.apply_appearance(inferred_appearance(color));
            }
            _ => {}
        }
        self.host_reports_key_releases = self.probe_device_attributes
            && self
                .probe_keyboard_flags
                .is_some_and(|flags| flags & 1 != 0);
        let mut facts = self.host_facts.clone();
        crate::host::apply_reply(reply, &mut facts);
        self.set_host_facts(facts);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hc_proto::watch::{OpenRpc, OpenSub, WatchParams, Watcher};
    use std::sync::{Arc, Mutex};

    fn app(config: &crate::config::Config) -> App {
        let rpc: OpenRpc = Arc::new(|| Box::pin(std::future::pending()));
        let sub: OpenSub = Arc::new(|_| Box::pin(std::future::pending()));
        let shell = crate::chrome::ClientShellConfig::from_config(
            config,
            None,
            None,
            std::path::PathBuf::new(),
        );
        App::new(
            Watcher::with_openers(
                rpc,
                sub,
                WatchParams {
                    traversal: Some(crate::project::agent_traversal),
                    ..WatchParams::default()
                },
            ),
            Arc::new(|_| Box::pin(std::future::pending())),
            config.live_keybinds(),
            shell,
            Arc::new(super::super::Probe::default()),
            Box::new(|_, _| {}),
        )
    }

    #[tokio::test]
    async fn explicit_appearance_wins_over_later_background_inference() {
        let mut config = crate::config::Config::default();
        config.theme.auto_switch = true;
        let mut app = app(&config);
        app.on_host_reply(HostReply::Background(hc_vt::Rgb {
            r: 255,
            g: 255,
            b: 255,
        }));
        assert_eq!(app.shell.theme_name, "catppuccin-latte");
        // Pinned herdr src/app/state.rs: Palette::catppuccin_latte.
        assert_eq!(
            app.shell.palette.overlay0,
            ratatui::style::Color::Rgb(156, 160, 176)
        );
        app.on_host_reply(HostReply::Dark);
        app.on_host_reply(HostReply::Background(hc_vt::Rgb {
            r: 255,
            g: 255,
            b: 255,
        }));
        assert_eq!(app.shell.theme_name, "catppuccin");
        app.on_host_reply(HostReply::Light);
        app.on_host_reply(HostReply::Background(hc_vt::Rgb { r: 0, g: 0, b: 0 }));
        assert_eq!(app.shell.theme_name, "catppuccin-latte");
    }

    #[tokio::test]
    async fn hidden_cursor_uses_explicit_anchor_then_clamped_visible_memory() {
        let mut app = app(&crate::config::Config::default());
        let output = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&output);
        app.set_host_control(Box::new(move |bytes| {
            sink.lock().unwrap().extend_from_slice(bytes);
            Ok(())
        }));
        app.area = (120, 40);
        app.want_cursor.pos = Some((110, 35));
        app.want_cursor.anchor = Some((110, 35));
        app.apply_cursor();
        app.want_cursor.pos = None;
        app.want_cursor.anchor = Some((40, 10));
        app.apply_cursor();
        assert!(output.lock().unwrap().ends_with(b"\x1b[11;41H\x1b[?25l"));
        app.want_cursor.anchor = None;
        app.area = (80, 24);
        app.apply_cursor();
        assert!(output.lock().unwrap().ends_with(b"\x1b[24;80H\x1b[?25l"));
    }

    #[tokio::test]
    async fn scheme_and_focus_refresh_only_their_reference_queries() {
        let mut app = app(&crate::config::Config::default());
        let writes = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
        let sink = writes.clone();
        app.set_host_control(Box::new(move |bytes| {
            sink.lock().unwrap().push(bytes.to_vec());
            Ok(())
        }));
        app.on_host_reply(HostReply::Light);
        assert_eq!(app.shell.theme_name, "catppuccin"); // manual theme remains selected
        app.on_input(crate::app::UiEvent::Focus(true), 80);
        app.on_input(crate::app::UiEvent::Focus(false), 80);
        let writes = writes.lock().unwrap();
        assert_eq!(writes.len(), 2);
        assert!(writes[0].starts_with(b"\x1b]10;?"));
        assert!(!writes[0].windows(7).any(|w| w == b"\x1b[?996n"));
        assert_eq!(writes[1], b"\x1b[?996n");
    }

    #[test]
    fn background_inference_matches_the_reference_luminance_boundary() {
        assert_eq!(
            inferred_appearance(hc_vt::Rgb {
                r: 127,
                g: 127,
                b: 127
            }),
            HostAppearance::Dark
        );
        assert_eq!(
            inferred_appearance(hc_vt::Rgb {
                r: 128,
                g: 128,
                b: 128
            }),
            HostAppearance::Light
        );
    }
}
