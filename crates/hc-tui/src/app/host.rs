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
    /// `cli::run` reports the hc terminal hosting this client
    /// (`self_caller`, verified against the endpoint socket). `None`
    /// on a remote endpoint or a foreign session.
    pub(crate) fn set_own_pane_id(&mut self, own: Option<String>) {
        self.own_pane_id = own;
    }

    /// `true` when `id` is the pane this client runs inside: the core
    /// refuses a terminal attaching to itself, so "which pane next"
    /// picks skip it — it stays reachable as an explicit target.
    pub(super) fn is_own_pane(&self, id: &str) -> bool {
        self.own_pane_id.as_deref() == Some(id)
    }

    fn apply_appearance(&mut self, appearance: HostAppearance) {
        self.host_appearance = Some(appearance);
        if self.shell.theme_runtime.auto_switch {
            let (palette, name) =
                resolve_effective_theme(&self.shell.theme_runtime, self.host_appearance);
            self.shell.palette = palette;
            self.shell.theme_name = name;
            self.shell.studio_injected_sidebar_bg = false;
        }
        self.sync_studio_chrome();
    }

    /// `true` when any measured fact moved — default colors, palette
    /// entries or cell pitch all feed painted cells. An unchanged
    /// bundle skips the pane/observer/lease re-seed entirely.
    pub fn set_host_facts(&mut self, facts: HostFacts) -> bool {
        if self.host_facts == facts {
            return false;
        }
        self.probe
            .host_facts
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
        true
    }

    /// `true` when the batch changed appearance or measured facts —
    /// what the frame paints. Capability answers (keyboard flags,
    /// pixel-mouse, device attributes) feed wire modes, not pixels.
    /// The whole batch folds into one facts application, so the ~258
    /// replies of the startup probe storm re-seed pane VTs once;
    /// appearance ordering and the probe guards are per reply, exactly
    /// as if each had arrived alone.
    pub(super) fn on_host_replies<I: IntoIterator<Item = HostReply>>(
        &mut self,
        replies: I,
    ) -> bool {
        let appearance = self.host_appearance;
        let mut facts = self.host_facts.clone();
        let mut query_colors = false;
        for reply in replies {
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
                    // A color-scheme report also refreshes the defaults/
                    // palette — batched into one re-query per batch.
                    query_colors = true;
                }
                HostReply::Background(color) if !self.host_appearance_explicit => {
                    self.apply_appearance(inferred_appearance(color));
                }
                _ => {}
            }
            crate::host::apply_reply(reply, &mut facts);
        }
        if query_colors {
            let _ = (self.host_control)(crate::host::color_query_sequence().as_bytes());
        }
        self.host_reports_key_releases = self.probe_device_attributes
            && self
                .probe_keyboard_flags
                .is_some_and(|flags| flags & 1 != 0);
        let facts_changed = self.set_host_facts(facts);
        appearance != self.host_appearance || facts_changed
    }

    /// `true` when the reply changed appearance or measured facts —
    /// the single-reply case of `on_host_replies`.
    pub(super) fn on_host_reply(&mut self, reply: HostReply) -> bool {
        self.on_host_replies(std::iter::once(reply))
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
        )
    }

    /// The startup probe storm (OSC 10/11 plus OSC 4;0..=255) folds
    /// into one facts application: pane VTs re-seed once, and a
    /// capability-only batch never touches the surface facts.
    #[tokio::test]
    async fn host_reply_batch_applies_facts_once_and_skips_capability_noop() {
        use std::sync::atomic::Ordering;
        let mut app = app(&crate::config::Config::default());
        let rgb = |v: u8| hc_vt::Rgb { r: v, g: v, b: v };
        let storm = std::iter::once(HostReply::Foreground(rgb(1)))
            .chain(std::iter::once(HostReply::Background(rgb(2))))
            .chain((0u16..=255).map(|i| HostReply::Palette(i as u8, rgb(i as u8))));
        assert!(app.on_host_replies(storm));
        assert_eq!(app.probe.host_facts.load(Ordering::Relaxed), 1);
        assert!(
            app.host_facts.theme.palette.iter().all(Option::is_some),
            "the storm left palette slots unset"
        );
        // Capability answers feed wire modes, not painted facts: the
        // unchanged bundle must not re-seed a single pane.
        assert!(!app.on_host_replies([
            HostReply::KeyboardFlags(1),
            HostReply::PrimaryDeviceAttributes,
            HostReply::SgrPixelMouse(true),
        ]));
        assert_eq!(app.probe.host_facts.load(Ordering::Relaxed), 1);
        assert!(app.host_reports_key_releases);
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
        app.area = (120, 40);
        app.want_cursor.pos = Some((110, 35));
        app.want_cursor.anchor = Some((110, 35));
        let cursor = app.resolve_host_cursor();
        assert_eq!((cursor.x, cursor.y, cursor.visible), (110, 35, true));
        app.want_cursor.pos = None;
        app.want_cursor.anchor = Some((40, 10));
        let cursor = app.resolve_host_cursor();
        assert_eq!((cursor.x, cursor.y, cursor.visible), (40, 10, false));
        app.want_cursor.anchor = None;
        app.area = (80, 24);
        let cursor = app.resolve_host_cursor();
        assert_eq!((cursor.x, cursor.y, cursor.visible), (79, 23, false));
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
