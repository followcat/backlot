// Derived from herdr src/app/popup.rs, src/popup_size.rs and
// herdr-app src/window.rs `peek_terminal` + defaults/mods/peek/main.js
// (Apache-2.0).
//! The floating popup terminal and the worker-peek overlay. Both float over
//! the tab that opened them: leaving the tab (or losing the terminal)
//! dismisses them — the reference's `popup_owner_tab_id` rule.
use super::input::TerminalKey;
use super::pane::{Pane, Role, Spec};
use super::*;
use crate::config::{CustomCommandAction, CustomCommandKeybind};
use crate::operations::Action;
use crate::popup_size::{PopupSize, resolve_popup_geometry};
use ratatui::layout::Rect;
use std::collections::BTreeMap;

/// Fit an attach to resolved popup terminal cells.
fn popup_fit(pane: &mut Pane, geometry: crate::popup_size::PopupResolvedGeometry) {
    pane.allocated(geometry.terminal_cols, geometry.terminal_rows);
}

/// The reference marks popup-owned terminals so client surfaces can keep
/// them out of the regular inventory views.
pub(crate) const POPUP_LABEL: &str = "hc-tui.popup";

/// A running popup attach.
pub(super) struct PopupState {
    pub pane: Pane,
    pub terminal: String,
    pub width: Option<PopupSize>,
    pub height: Option<PopupSize>,
    /// Tab the popup was opened over; a focus change dismisses it.
    pub owner_tab: Option<String>,
    /// The terminal showed up in the inventory at least once — only then
    /// does disappearance mean "exited" instead of "not yet published".
    pub seen: bool,
}

/// The `[[keys.command]]` geometry stashed while `terminal.run` is in
/// flight — `Outcome::Ran` opens the popup with it.
#[derive(Debug, Clone, Copy)]
pub(super) struct PopupPending {
    pub width: Option<PopupSize>,
    pub height: Option<PopupSize>,
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
            CustomCommandAction::Pane => match self.creation(Some(false), true) {
                Ok(mut creation) => {
                    creation.params.argv = Vec::new();
                    creation.params.line = Some(command.command.clone());
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
    /// attach when the address lands (`on_popup_spawned`).
    fn spawn_popup(&mut self, command: &CustomCommandKeybind) {
        if self.popup.is_some() || self.popup_pending.is_some() {
            self.close_popup();
        }
        let geometry = resolve_popup_geometry(command.width, command.height, self.frame_area());
        let mut params = hc_proto::terminal::RunParams {
            line: Some(command.command.clone()),
            rm: true,
            keep: false,
            no_notify: true,
            no_parent: true,
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
        });
        self.submit_action(Action::RunPopup { params });
    }

    /// `Outcome::Ran`: the popup terminal exists — attach it active.
    pub(super) fn on_popup_spawned(&mut self, terminal: String) {
        let Some(pending) = self.popup_pending.take() else {
            return;
        };
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
            seen: false,
        });
    }

    /// The surface rectangle the popup resolves against.
    fn frame_area(&self) -> Rect {
        Rect::new(0, 0, self.area.0, self.area.1)
    }

    /// Close the popup: detach and kill the owned terminal — the
    /// reference's `close_popup_pane` drops the runtime; the core's
    /// equivalent is `terminal.kill` on the `hc-tui.popup` child.
    pub(super) fn close_popup(&mut self) {
        if let Some(mut popup) = self.popup.take() {
            popup.pane.want(None);
            if let Some(term) = self.view.inventory.get(&popup.terminal) {
                let created_ms = term.created_ms;
                self.submit_action(Action::Kill {
                    terminal: popup.terminal,
                    created_ms,
                });
            }
            self.sched.mark_dirty(Instant::now());
        }
    }

    /// Keep the popup's owner rule and inventory liveness after every
    /// publish — a tab switch or a dead terminal dismisses it.
    pub(super) fn reconcile_popups(&mut self) {
        if let Some(popup) = &mut self.popup {
            let seen = self.view.inventory.get(&popup.terminal).is_some();
            popup.seen |= seen;
            let owner_gone = popup.owner_tab.is_some()
                && popup.owner_tab != self.projection.snapshot.focused_tab_id;
            let lost =
                popup.seen && matches!(self.view.link, hc_proto::watch::Link::Online) && !seen;
            if owner_gone || lost {
                self.close_popup();
            }
        }
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
    }

    /// Every key while the popup is up is program input; `Esc` (no mods)
    /// closes — the reference routes popup keys before UI bindings.
    pub(super) fn popup_key(&mut self, key: &TerminalKey) -> bool {
        let Some(popup) = self.popup.as_mut() else {
            return false;
        };
        if key.input.action == hc_vt::Action::Press
            && key.input.key == hc_vt::Key::Escape
            && key.input.mods == hc_vt::Mods::NONE
        {
            self.close_popup();
            return true;
        }
        popup.pane.key(key);
        true
    }

    /// Paste/IME text goes to the popup while it is open.
    pub(super) fn popup_text(&mut self, bytes: Vec<u8>) -> bool {
        if let Some(popup) = self.popup.as_mut() {
            popup.pane.input_encoded(bytes);
            return true;
        }
        false
    }

    /// Re-resolve geometry on a host resize and refit the popup attach.
    pub(super) fn popup_resize(&mut self) {
        let area = self.frame_area();
        if let Some(popup) = self.popup.as_mut()
            && let Some(geometry) = resolve_popup_geometry(popup.width, popup.height, area)
        {
            popup_fit(&mut popup.pane, geometry);
        }
    }

    /// A press outside the popup dismisses it; inside stays modal for now
    /// (mouse forwarding into the popup surface is a documented gap).
    pub(super) fn popup_mouse(&mut self, mouse: &super::input::MouseInput) -> bool {
        let Some(popup) = &self.popup else {
            return false;
        };
        let geometry = resolve_popup_geometry(popup.width, popup.height, self.frame_area());
        let inside = geometry.is_some_and(|g| {
            mouse.col >= g.outer.x
                && mouse.col < g.outer.x + g.outer.width
                && mouse.row >= g.outer.y
                && mouse.row < g.outer.y + g.outer.height
        });
        if !inside
            && matches!(
                mouse.kind,
                super::input::MouseEventKind::Down(super::input::MouseButton::Left)
            )
        {
            self.close_popup();
        }
        true
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
    pub(super) fn draw_popups(
        &mut self,
        buf: &mut ratatui::buffer::Buffer,
        area: Rect,
        now: Instant,
    ) {
        use ratatui::style::{Modifier, Style};
        use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget};
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
            super::draw::draw_terminal(&mut peek.pane, surface, buf, now, true);
        }
        let Some(geometry) = self
            .popup
            .as_ref()
            .and_then(|p| resolve_popup_geometry(p.width, p.height, area))
        else {
            return;
        };
        Clear.render(geometry.outer, buf);
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
            .title(format!(" {title} "));
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
        let wish = super::draw::draw_terminal(&mut popup.pane, surface, buf, now, false);
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
