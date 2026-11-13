// Derived from herdr src/app/popup.rs, src/popup_size.rs and
// herdr-app src/window.rs `peek_terminal` + defaults/mods/peek/main.js
// (Apache-2.0).
//! The floating popup terminal and the worker-peek overlay. Both float over
//! the tab that opened them: leaving the tab (or losing the terminal)
//! dismisses them — the reference's `popup_owner_tab_id` rule.
use super::input::{MouseButton, MouseEventKind, MouseInput, TerminalKey};
use super::pane::{Pane, Role, Spec};
use super::*;
use crate::app::creation::program_env;
use crate::config::{CustomCommandAction, CustomCommandKeybind};
use crate::operations::Action;
use crate::popup_size::{PopupSize, resolve_popup_geometry};
use ratatui::layout::Rect;
use std::collections::BTreeMap;

/// Fit an attach to resolved popup terminal cells.
pub(super) fn popup_fit(pane: &mut Pane, geometry: crate::popup_size::PopupResolvedGeometry) {
    pane.allocated(geometry.terminal_cols, geometry.terminal_rows);
}

/// The reference marks popup-owned terminals so client surfaces can keep
/// them out of the regular inventory views.
pub(crate) const POPUP_LABEL: &str = "hc-tui.popup";

/// A persistent float's owning tab, as `layout_key(machine, tab_id)` —
/// rediscovery matches inventory records through it after a restart.
pub(crate) const FLOAT_OWNER_LABEL: &str = "hc-tui.owner-tab";

/// A persistent float's binding label (`alt+f`), kept on the terminal so
/// a restart can rebuild the same map key.
pub(crate) const FLOAT_COMMAND_LABEL: &str = "hc-tui.command";

/// A running popup attach.
pub(super) struct PopupState {
    pub pane: Pane,
    pub terminal: String,
    pub width: Option<PopupSize>,
    pub height: Option<PopupSize>,
    /// Tab the popup was opened over; a focus change dismisses it.
    pub owner_tab: Option<String>,
    /// `persistent` popups are per-tab floats — a hide demotes them into
    /// `floats` + a `size_leases` attach instead of killing the program.
    pub persistent: bool,
    /// Map identity while a persistent float is shown.
    pub key: Option<float::FloatKey>,
    /// Core `created_ms` from the `terminal.run` result (or the adopted
    /// record) — the incarnation match for `Action::Kill`.
    pub created_ms: u64,
    /// The terminal showed up in the inventory at least once — only then
    /// does disappearance mean "exited" instead of "not yet published".
    pub seen: bool,
}

/// The `[[keys.command]]` geometry stashed while `terminal.run` is in
/// flight — `Outcome::Ran` opens the popup with it.
#[derive(Debug, Clone)]
pub(super) struct PopupPending {
    pub width: Option<PopupSize>,
    pub height: Option<PopupSize>,
    /// Persistent floats bind their owning tab at request time, so an
    /// `Outcome::Ran` landing after a switch never adopts the new tab.
    pub persistent: bool,
    pub owner: String,
    pub command: String,
}

/// A reporting popup's captured button — the reference's
/// `ClientPaneMouseGesture` for `hit.popup` targets: the press's hit is
/// kept so drag/release forward even while the pointer leaves the
/// popup's frame.
#[derive(Clone)]
pub(super) struct PopupGesture {
    pub button: MouseButton,
    pub hit: crate::chrome::render::PaneHit,
}

/// The `pane` command's zoomed overlay: when its one-shot terminal exits,
/// the split collapses and focus/zoom return — the reference's
/// `overlay_panes` + `restore_overlay_after_exit`.
pub(super) struct CommandOverlay {
    pub terminal: String,
    /// The terminal's core `created_ms`, captured once the run lands in
    /// the inventory — `edit::hide` needs it for the `closed` tombstone
    /// match in `project_snapshot`.
    pub created_ms: Option<u64>,
    /// Same inventory-lag guard as `PopupState.seen` — the run must be
    /// observed once before disappearance counts as "exited".
    pub seen: bool,
}

/// A peek overlay: an observe-only attach over a running terminal plus its
/// keep-as-tab facts (`herdr-app`'s `terminal.peek` overlay).
pub(super) struct PeekState {
    pub pane: Pane,
    pub terminal: String,
    pub created_ms: u64,
    pub owner_tab: Option<String>,
    /// Peeked terminals exist at open; loss is real immediately.
    pub seen: bool,
}

impl App {
    /// `[[keys.command]]` dispatch. `popup` runs `terminal.run` into the
    /// floating popup, `pane` runs it as a new split of the focused pane,
    /// `shell` issues `machine.exec`, and a plugin action shows the
    /// runtime-less unavailable notice (parity.md).
    pub(super) fn run_custom_command(&mut self, command: &CustomCommandKeybind) {
        match command.action {
            CustomCommandAction::Shell => {
                let machine = self
                    .focused
                    .as_deref()
                    .and_then(|s| s.split_once('/').map(|(m, _)| m.to_owned()));
                self.submit_action(Action::Exec {
                    line: command.command.clone(),
                    machine,
                });
            }
            // The reference splits horizontally then zooms the new pane
            // over the whole tab (`spawn_pane_command` + `zoomed = true`).
            // Its command is one-shot: `rm` removes the exited terminal
            // and the overlay reconcile collapses the split back.
            CustomCommandAction::Pane => match self.creation(Some(false), true) {
                Ok(mut creation) => {
                    creation.params.argv = Vec::new();
                    creation.params.line = Some(command.command.clone());
                    creation.params.keep = false;
                    creation.params.rm = true;
                    creation.report_created = true;
                    self.submit_action(Action::Create(Box::new(creation)));
                }
                Err(error) => self.show_notice("Command unavailable", &error),
            },
            CustomCommandAction::Popup => self.spawn_popup(command),
            CustomCommandAction::PluginAction => self.show_notice(
                "Unavailable",
                "Plugin commands need the herdr plugin runtime; hc has none",
            ),
        }
    }

    /// `type = "popup"`: `terminal.run` with the popup marker label, then
    /// attach when the address lands (`on_popup_spawned`). A `persistent`
    /// binding toggles the tab's float instead.
    fn spawn_popup(&mut self, command: &CustomCommandKeybind) {
        if command.persistent {
            self.spawn_persistent_float(command);
            return;
        }
        if self.popup.is_some() || self.popup_pending.is_some() {
            // A persistent float demotes onto a hidden lease; only an
            // ephemeral popup dies to make room.
            self.dismiss_popup();
        }
        let geometry = resolve_popup_geometry(command.width, command.height, self.frame_area());
        let mut params = hc_proto::terminal::RunParams {
            line: Some(command.command.clone()),
            rm: true,
            keep: false,
            no_notify: true,
            no_parent: true,
            env: program_env(),
            labels: BTreeMap::from([(POPUP_LABEL.into(), "1".into())]),
            display: self.host_facts.display(),
            ..Default::default()
        };
        if let Some(machine) = self
            .focused
            .as_deref()
            .and_then(|s| s.split_once('/').map(|(m, _)| m.to_owned()))
        {
            params.on = Some(machine);
        }
        if let Some(geometry) = geometry {
            params.size = Some(hc_proto::terminal::InitialSize {
                cols: geometry.terminal_cols,
                rows: geometry.terminal_rows,
            });
        }
        self.popup_pending = Some(PopupPending {
            width: command.width,
            height: command.height,
            persistent: false,
            owner: String::new(),
            command: String::new(),
        });
        self.submit_action(Action::RunPopup { params });
    }

    /// `Outcome::Ran`: the popup terminal exists — attach it active, or
    /// park a persistent float hidden when its owner tab lost focus
    /// while the run was in flight.
    pub(super) fn on_popup_spawned(&mut self, terminal: String, created_ms: u64) {
        let Some(pending) = self.popup_pending.take() else {
            return;
        };
        if pending.persistent {
            let key = float::FloatKey {
                owner: pending.owner,
                command: pending.command,
            };
            if self.focused_owner_key().as_ref() == Some(&key.owner) {
                self.dismiss_popup();
                self.show_float(key, terminal, pending.width, pending.height, created_ms);
            } else {
                // The request bound its owner up front — a tab switch
                // in flight never steals the new float onto this tab.
                self.hide_float(
                    key,
                    float::FloatEntry {
                        terminal,
                        width: pending.width,
                        height: pending.height,
                        restore: true,
                        created_ms,
                        seen: false,
                    },
                );
            }
            return;
        }
        let mut pane = Pane::new(self.pane_tx.clone(), self.open_attach.clone());
        pane.set_host_facts(self.host_facts.clone());
        pane.want(Some(Spec {
            terminal: terminal.clone(),
            role: Role::Active,
        }));
        pane.on_view(&self.view);
        let area = self.frame_area();
        if let Some(geometry) = resolve_popup_geometry(pending.width, pending.height, area) {
            popup_fit(&mut pane, geometry);
        }
        self.popup = Some(PopupState {
            pane,
            terminal,
            width: pending.width,
            height: pending.height,
            owner_tab: self.projection.snapshot.focused_tab_id.clone(),
            persistent: false,
            key: None,
            created_ms,
            seen: false,
        });
    }

    /// The surface rectangle the popup resolves against: the pane surface
    /// (right of the sidebar, under the tab bar) — the reference's
    /// `layout.pane_surface` in `composition.rs`.
    pub(super) fn frame_area(&self) -> Rect {
        self.shell
            .layout_config()
            .layout(
                self.area.0,
                self.area.1,
                self.shell_state.sidebar_collapsed,
                super::pane_geometry::focused_tab_count(self),
                self.sidebar_width,
            )
            .pane_surface
    }

    /// Close the popup: detach and kill the owned terminal — the
    /// reference's `close_popup_pane` drops the runtime; the core's
    /// equivalent is `terminal.kill` on the `hc-tui.popup` child.
    pub(super) fn close_popup(&mut self) {
        self.popup_selection = None;
        self.popup_gesture = None;
        if let Some(mut popup) = self.popup.take() {
            popup.pane.want(None);
            // `rm` terminals are reaped on exit when the last attach
            // detaches, so the kill can legitimately arrive after the
            // core already removed it — `allow_gone` keeps that quiet.
            self.submit_action(Action::Kill {
                terminal: popup.terminal,
                created_ms: popup.created_ms,
                allow_gone: true,
            });
            self.sched.mark_dirty(Instant::now());
        }
    }

    /// Keep the popup's owner rule and inventory liveness after every
    /// publish — a tab switch or a dead terminal dismisses an ephemeral
    /// popup; a persistent float demotes, dies only with its owner tab,
    /// or drops when its `rm` terminal is reaped.
    pub(super) fn reconcile_popups(&mut self) {
        let focused_owner = self.focused_owner_key();
        if let Some(popup) = &mut self.popup {
            let seen = self.view.inventory.get(&popup.terminal).is_some();
            popup.seen |= seen;
            let lost =
                popup.seen && matches!(self.view.link, hc_proto::watch::Link::Online) && !seen;
            if popup.persistent {
                // The qualified owner tab disappeared for good — kill.
                // `sync_float_focus` normally demotes a focus switch the
                // same event; the wrong-owner arm stays defensive.
                let owner_gone = popup.key.as_ref().is_some_and(|key| {
                    Self::float_owner_gone(&self.projection.endpoints, &key.owner)
                });
                let wrong_tab = popup
                    .key
                    .as_ref()
                    .is_some_and(|key| focused_owner.as_ref() != Some(&key.owner));
                if lost || owner_gone {
                    self.close_popup();
                } else if wrong_tab {
                    self.demote_float(true);
                }
            } else {
                let owner_gone = popup.owner_tab.is_some()
                    && popup.owner_tab != self.projection.snapshot.focused_tab_id;
                if owner_gone || lost {
                    self.close_popup();
                }
            }
        }
        self.reconcile_floats();
        if let Some(peek) = &mut self.peek {
            let seen = self.view.inventory.get(&peek.terminal).is_some();
            peek.seen |= seen;
            let owner_gone = peek.owner_tab.is_some()
                && peek.owner_tab != self.projection.snapshot.focused_tab_id;
            let lost =
                peek.seen && matches!(self.view.link, hc_proto::watch::Link::Online) && !seen;
            if owner_gone || lost {
                self.close_peek();
            }
        }
        // A `keys.command` pane is one-shot (`rm`): when its program ends
        // the split collapses and prior focus/zoom return — the
        // reference's `restore_overlay_after_exit` in `app/api.rs`.
        if let Some(overlay) = &mut self.command_overlay {
            let term = self.view.inventory.get(&overlay.terminal);
            if let Some(term) = term {
                overlay.seen = true;
                overlay.created_ms = Some(term.created_ms);
            }
            let ended = overlay.seen
                && matches!(self.view.link, hc_proto::watch::Link::Online)
                && term.is_none_or(|t| t.state != "live" || t.finished_ms.is_some());
            if ended {
                let overlay = self.command_overlay.take().expect("checked above");
                self.submit_action(Action::Forget {
                    terminal: overlay.terminal,
                    created_ms: overlay.created_ms.unwrap_or_default(),
                });
            }
        }
    }

    /// Every key while the popup is up is program input — the reference's
    /// `popup_input_target` forwards them all (even Escape) to the popup
    /// terminal; the popup closes when its terminal exits or is removed.
    pub(super) fn popup_key(&mut self, key: &TerminalKey) -> bool {
        let Some(popup) = self.popup.as_mut() else {
            return false;
        };
        popup.pane.key(key);
        true
    }

    /// IME text goes to the popup while it is open — the reference's
    /// `ClientPaneInputEvent::TextCommit`; the popup selection clears on
    /// the same arm.
    pub(super) fn popup_text(&mut self, bytes: Vec<u8>) -> bool {
        if self.popup.is_some() {
            self.clear_popup_selection();
        }
        if let Some(popup) = self.popup.as_mut() {
            popup.pane.input_encoded(bytes);
            return true;
        }
        false
    }

    /// Host paste goes to the popup as a semantic paste — the
    /// reference's `ClientPaneInputEvent::Paste` — so the wire encodes
    /// it under the popup terminal's bracketed-paste mode instead of
    /// landing verbatim.
    pub(super) fn popup_paste(&mut self, text: &[u8]) -> bool {
        if self.popup.is_some() {
            self.clear_popup_selection();
        }
        if let Some(popup) = self.popup.as_mut() {
            popup.pane.paste(text);
            return true;
        }
        false
    }

    /// Re-resolve geometry on a host resize and refit the popup attach —
    /// the reference drops the popup selection on a size change.
    pub(super) fn popup_resize(&mut self) {
        self.popup_selection = None;
        let area = self.frame_area();
        if let Some(popup) = self.popup.as_mut()
            && let Some(geometry) = resolve_popup_geometry(popup.width, popup.height, area)
        {
            popup_fit(&mut popup.pane, geometry);
        }
    }

    /// Send a host mouse event to the popup terminal — the reference's
    /// `push_pane_mouse_event` for `hit.popup`. Pixel position is used
    /// when the popup program enabled SGR pixel reporting (1016), like
    /// `send_pane_mouse` for panes.
    fn send_popup_mouse(&mut self, hit: &crate::chrome::render::PaneHit, mouse: MouseInput) {
        let Some(popup) = self
            .popup
            .as_mut()
            .filter(|popup| popup.terminal == hit.pane_id)
        else {
            return;
        };
        let mut input = pane_mouse::native_input(hit, mouse);
        if let Some(vt) = popup.pane.vt_ref()
            && vt.mode(1016).unwrap_or(false)
        {
            let (cw, ch) = vt.cell_px();
            if let Some(crate::mouse_geometry::Position::Pixels { x, y }) =
                self.host_mouse_pixels.and_then(|p| {
                    p.pane_position(
                        hit.inner_rect,
                        u32::from(hit.inner_rect.width) * cw,
                        u32::from(hit.inner_rect.height) * ch,
                    )
                })
            {
                input.position = hc_vt::MousePosition::Pixels { x, y };
            }
        }
        popup.pane.mouse(input);
    }

    /// The reference's popup mouse arm. Its shared `pane_mouse_gesture`
    /// continuation runs first, then `popup_pending` eats everything,
    /// then selection owns non-reporting presses
    /// (`handle_popup_mouse_selection`); inside the frame, presses and
    /// wheel events always reach the popup terminal (its VT decides
    /// whether they encode), tracked motion follows `mouse_reporting`,
    /// and a reporting press captures the button until release. Outside
    /// events are ignored while a popup is open.
    pub(super) fn popup_mouse(&mut self, mouse: &MouseInput) -> bool {
        // A pane gesture armed before the popup opened keeps its
        // button's drag/release — `route_pane_mouse` owns the rest.
        if self.pane_mouse_gesture.is_some() {
            return false;
        }
        if let Some(gesture) = self.popup_gesture.clone() {
            let gesture_event = matches!(mouse.kind,
                MouseEventKind::Drag(button) | MouseEventKind::Up(button)
                    if button == gesture.button);
            if gesture_event {
                let hit = self
                    .hit_map
                    .popup
                    .as_ref()
                    .filter(|hit| hit.pane_id == gesture.hit.pane_id)
                    .cloned()
                    .unwrap_or_else(|| gesture.hit.clone());
                self.send_popup_mouse(&hit, *mouse);
                if mouse.kind == MouseEventKind::Up(gesture.button) {
                    self.popup_gesture = None;
                }
                return true;
            }
            if matches!(
                mouse.kind,
                MouseEventKind::Down(_) | MouseEventKind::Drag(_) | MouseEventKind::Up(_)
            ) {
                return true;
            }
        }
        if self.popup_pending.is_some() {
            return true;
        }
        if let Some(hit) = self.hit_map.popup.clone() {
            if self.handle_popup_mouse_selection(&hit, mouse) {
                return true;
            }
            if super::mouse::contains(hit.inner_rect, (mouse.col, mouse.row)) {
                match mouse.kind {
                    MouseEventKind::Down(button) => {
                        self.send_popup_mouse(&hit, *mouse);
                        if self
                            .popup
                            .as_ref()
                            .and_then(|popup| popup.pane.vt_ref())
                            .is_some_and(|vt| vt.mouse_tracking())
                        {
                            self.popup_gesture = Some(PopupGesture {
                                button,
                                hit: hit.clone(),
                            });
                        }
                    }
                    MouseEventKind::Moved
                        if self
                            .popup
                            .as_ref()
                            .and_then(|popup| popup.pane.vt_ref())
                            .is_some_and(|vt| vt.mouse_tracking()) =>
                    {
                        self.send_popup_mouse(&hit, *mouse);
                    }
                    MouseEventKind::ScrollUp
                    | MouseEventKind::ScrollDown
                    | MouseEventKind::ScrollLeft
                    | MouseEventKind::ScrollRight => {
                        self.send_popup_mouse(&hit, *mouse);
                    }
                    MouseEventKind::Up(_) | MouseEventKind::Drag(_) | MouseEventKind::Moved => {}
                }
            }
            return true;
        }
        self.popup.is_some()
    }

    /// Peek floats like herdr-app's 0.8 dialog; a press outside closes it.
    fn peek_outer(&self) -> Option<Rect> {
        self.peek.as_ref()?;
        resolve_popup_geometry(
            Some(PopupSize::Percent(80)),
            Some(PopupSize::Percent(80)),
            self.frame_area(),
        )
        .map(|g| g.outer)
    }

    pub(super) fn peek_mouse(&mut self, mouse: &super::input::MouseInput) -> bool {
        let Some(outer) = self.peek_outer() else {
            return false;
        };
        let inside = mouse.col >= outer.x
            && mouse.col < outer.x + outer.width
            && mouse.row >= outer.y
            && mouse.row < outer.y + outer.height;
        if !inside
            && matches!(
                mouse.kind,
                super::input::MouseEventKind::Down(super::input::MouseButton::Left)
            )
        {
            self.close_peek();
        }
        true
    }

    /// Draw the peek overlay, then the popup over it. The popup owns the
    /// host cursor while open; a peek never gets one (observe-only).
    pub(super) fn draw_popups(&mut self, buf: &mut ratatui::buffer::Buffer, now: Instant) {
        use ratatui::style::{Modifier, Style};
        use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget};
        self.hit_map.popup = None;
        if self.popup.is_none() && self.peek.is_none() {
            return;
        }
        // The source composes overlays by rebuilding the wire frame as
        // ratatui cells (wire.rs u16_to_modifier drops extended underline
        // bits); neither overlay dims the background.
        for cell in &mut buf.content {
            cell.modifier = Modifier::from_bits_truncate(cell.modifier.bits() & !0xf000);
        }
        let palette = &self.shell.palette;
        if let Some(outer) = self.peek_outer()
            && let Some(peek) = self.peek.as_mut()
        {
            Clear.render(outer, buf);
            let title = self
                .view
                .inventory
                .get(&peek.terminal)
                .and_then(|t| t.labels.get("herdr.title").cloned())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| peek.terminal.clone());
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(palette.accent).bg(palette.panel_bg))
                .style(Style::default().bg(palette.panel_bg))
                .title(format!(
                    " peek {title} — enter keeps it as a tab, esc closes "
                ));
            let inner = block.inner(outer);
            block.render(outer, buf);
            let header = Rect::new(inner.x, inner.y, inner.width, 1);
            let surface = Rect::new(
                inner.x,
                inner.y + 1,
                inner.width,
                inner.height.saturating_sub(1),
            );
            Paragraph::new(format!(" {}", title))
                .style(
                    Style::default()
                        .fg(palette.text)
                        .bg(palette.panel_bg)
                        .add_modifier(Modifier::BOLD),
                )
                .render(header, buf);
            Clear.render(surface, buf);
            super::draw::draw_terminal(&mut peek.pane, surface, buf, now, true);
            // The peek surface is composed into the rebuilt ratatui
            // frame: its extended underline bits drop too.
            for cell in &mut buf.content {
                cell.modifier = Modifier::from_bits_truncate(cell.modifier.bits() & !0xf000);
            }
        }
        let Some(geometry) = self
            .popup
            .as_ref()
            .and_then(|p| resolve_popup_geometry(p.width, p.height, self.frame_area()))
        else {
            return;
        };
        Clear.render(geometry.outer, buf);
        // The reference title is the terminal's manual label or "popup".
        let title = self
            .popup
            .as_ref()
            .and_then(|p| self.view.inventory.get(&p.terminal))
            .and_then(|t| t.labels.get("herdr.title").cloned())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "popup".to_owned());
        let Some(popup) = self.popup.as_mut() else {
            return;
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(palette.accent).bg(palette.panel_bg))
            .style(Style::default().bg(palette.panel_bg))
            .title(title);
        let inner = block.inner(geometry.outer);
        block.render(geometry.outer, buf);
        // The attach's cells are `terminal_cols` wide — one column
        // narrower than `inner` when the popup is wide, like the
        // reference's `terminal_width` asymmetry.
        let surface = Rect::new(
            inner.x,
            inner.y,
            inner.width.min(geometry.terminal_cols),
            inner.height.min(geometry.terminal_rows),
        );
        // The block fills the interior with `panel_bg`; the reference
        // leaves the covered cells on the terminal default background
        // (`blit_pane_surface` only paints terminal cells), so reset
        // exactly the surface area and keep the panel fill outside it.
        Clear.render(surface, buf);
        let wish = super::draw::draw_terminal(&mut popup.pane, surface, buf, now, false);
        // The popup's VT cells went through the same ratatui rebuild in
        // the reference — extended underline bits collapse to single.
        for cell in &mut buf.content {
            cell.modifier = Modifier::from_bits_truncate(cell.modifier.bits() & !0xf000);
        }
        let popup_terminal = popup.terminal.clone();
        self.hit_map.popup = Some(crate::chrome::render::PaneHit {
            rect: geometry.outer,
            inner_rect: inner,
            scrollbar_rect: None,
            scroll: None,
            pane_id: popup_terminal,
        });
        self.render_popup_selection(buf, inner);
        // The returned wish is surface-relative; shift into frame cells.
        self.want_cursor = super::draw::CursorWish {
            pos: wish.pos.map(|(x, y)| (surface.x + x, surface.y + y)),
            anchor: wish.anchor.map(|(x, y)| (surface.x + x, surface.y + y)),
            ..wish
        };
    }

    /// `terminal.peek` — observe a running terminal in the floating view.
    /// Anything already kept (a pane exists) just focuses its tab instead,
    /// like herdr-app's `↗ go to`.
    pub(super) fn open_peek(&mut self, terminal: String) {
        let Some(created_ms) = self.view.inventory.get(&terminal).map(|t| t.created_ms) else {
            self.show_notice("Peek unavailable", "the terminal is not running");
            return;
        };
        self.close_peek();
        let mut pane = Pane::new(self.pane_tx.clone(), self.open_attach.clone());
        pane.set_host_facts(self.host_facts.clone());
        pane.want(Some(Spec {
            terminal: terminal.clone(),
            role: Role::Observe,
        }));
        pane.on_view(&self.view);
        self.peek = Some(PeekState {
            pane,
            terminal,
            created_ms,
            owner_tab: self.projection.snapshot.focused_tab_id.clone(),
            seen: true,
        });
    }

    pub(super) fn close_peek(&mut self) {
        if let Some(mut peek) = self.peek.take() {
            peek.pane.want(None);
            self.sched.mark_dirty(Instant::now());
        }
    }

    /// Peek keys: `Esc` dismisses, `Enter` keeps as a tab — the peek bar's
    /// `↗ keep` and `✕` buttons (Observe attaches never type).
    pub(super) fn peek_key(&mut self, key: &TerminalKey) -> bool {
        if self.peek.is_none() {
            return false;
        }
        if key.input.action != hc_vt::Action::Press {
            return true;
        }
        match (key.input.key, key.input.mods) {
            (hc_vt::Key::Escape, hc_vt::Mods::NONE) => self.close_peek(),
            (hc_vt::Key::Enter, hc_vt::Mods::NONE) => self.keep_peek(),
            _ => {}
        }
        true
    }

    /// `↗ keep`: adopt the peeked terminal into a fresh tab of the focused
    /// space — the layout CAS plus `terminal.keep`, then dismiss the peek.
    fn keep_peek(&mut self) {
        let Some(peek) = self.peek.take() else {
            return;
        };
        let created_ms = self
            .view
            .inventory
            .get(&peek.terminal)
            .map(|t| t.created_ms)
            .unwrap_or(peek.created_ms);
        let terminal = peek.terminal.clone();
        let mut pane = peek.pane;
        pane.want(None);
        match self.creation(None, false) {
            Ok(creation) => {
                let labels = creation
                    .params
                    .labels
                    .iter()
                    .filter(|(k, _)| k.as_str() != "herdr.seen")
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                self.submit_action(Action::PeekKeep {
                    edit: creation.edit,
                    terminal,
                    created_ms,
                    labels,
                    order: creation.order,
                });
            }
            Err(error) => self.show_notice("Keep unavailable", &error),
        }
    }
}
