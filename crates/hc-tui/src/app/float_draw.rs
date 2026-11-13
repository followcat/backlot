// Fresh code for hc-tui's per-tab floating terminal.
//! The persistent float's frame dressing: a dimmed backdrop, a drop
//! shadow, rounded borders, a rich title, and the focused/unfocused
//! border split. Ephemeral popups keep the reference's plain frame —
//! `draw_popups` reaches in here only for `popup.persistent` floats.

use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders};
use unicode_width::UnicodeWidthStr;

use super::popup::PopupState;
use super::*;
use crate::chrome::snapshot::ClientEndpointId;

impl App {
    /// A persistent float is big enough that its surroundings deserve the
    /// dialog treatment: one pass dims the pane surface it covers — the
    /// float's own cells repaint after, so only the backdrop keeps `DIM`
    /// and the sidebar/tab-bar/status chrome outside `pane_surface`
    /// stays undimmed.
    pub(super) fn dim_float_backdrop(&self, buf: &mut Buffer) {
        let area = self.frame_area();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                let cell = &mut buf[(x, y)];
                cell.set_style(cell.style().add_modifier(Modifier::DIM));
            }
        }
    }

    /// Classic TUI drop shadow, staggered: the right column starts one
    /// row down, the bottom row one column over. Painted with the
    /// palette's darkest surface as a background change only, so covered
    /// glyphs stay legible; clipped to the pane surface so the sidebar
    /// and status chrome never sit under it.
    pub(super) fn draw_float_shadow(&self, buf: &mut Buffer, outer: Rect) {
        let clip = self.frame_area();
        let color = self.shell.palette.surface_dim;
        let mut shade = |x: u16, y: u16| {
            if clip.contains(Position::new(x, y)) {
                buf[(x, y)].set_bg(color);
            }
        };
        for y in outer.top() + 1..=outer.bottom() {
            shade(outer.right(), y);
        }
        for x in outer.left() + 1..=outer.right() {
            shade(x, outer.bottom());
        }
    }

    /// The border color: accent while the float owns input. A visible
    /// float loses its keys only while `keyboard_locked` — the lock arm
    /// runs before the popup's — which is the one muted-border state.
    pub(super) fn float_border_color(&self, persistent: bool) -> ratatui::style::Color {
        let focused = !self.keyboard_locked;
        if !persistent || focused {
            self.shell.palette.accent
        } else {
            self.shell.palette.surface_dim
        }
    }

    /// The float's block: rounded borders, left title
    /// `float <binding> — <tab>[ — <program>]`, right-aligned hint
    /// `<binding> hide · esc sends to program` while it still fits clear
    /// of the title.
    pub(super) fn float_block(&self, outer: Rect) -> Block<'static> {
        let Some(popup) = self.popup.as_ref() else {
            return Block::default().borders(Borders::ALL);
        };
        let palette = &self.shell.palette;
        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(
                Style::default()
                    .fg(self.float_border_color(popup.persistent))
                    .bg(palette.panel_bg),
            )
            .style(Style::default().bg(palette.panel_bg))
            .title(format!(" {} ", self.float_title(popup)));
        if let Some(hint) = self.float_hint(popup, outer.width) {
            block = block.title(Line::from(format!(" {hint} ")).alignment(Alignment::Right));
        }
        block
    }

    /// `float <binding> — <tab>` plus ` — <program>` when the record
    /// reports `herdr.title`. A key-less float (impossible today, kept
    /// defensive) falls back to the program title or a bare `popup`.
    fn float_title(&self, popup: &PopupState) -> String {
        let program = self
            .view
            .inventory
            .get(&popup.terminal)
            .and_then(|t| t.labels.get("herdr.title"))
            .map(String::as_str)
            .filter(|t| !t.is_empty());
        let Some(key) = &popup.key else {
            return program.unwrap_or("popup").to_owned();
        };
        let mut title = format!("float {}", key.command);
        if let Some(tab) = self.float_owner_tab_title(&key.owner) {
            title.push_str(" — ");
            title.push_str(&tab);
        }
        if let Some(program) = program {
            title.push_str(" — ");
            title.push_str(program);
        }
        title
    }

    /// The owner tab's display title: its snapshot label, else the
    /// `tab:<len>:<name>` document-name tail of the owner key — remote
    /// owners and offline snapshots keep a readable name either way.
    fn float_owner_tab_title(&self, owner: &str) -> Option<String> {
        let (machine, tab) = Self::owner_parts(owner)?;
        let endpoint_id = machine
            .map(|m| ClientEndpointId::Remote(m.to_string()))
            .unwrap_or(ClientEndpointId::Local);
        let label = self
            .projection
            .endpoints
            .iter()
            .find(|e| e.endpoint_id == endpoint_id)
            .and_then(|e| e.snapshot.as_deref())
            .and_then(|s| s.tabs.iter().find(|t| t.tab_id == tab))
            .map(|t| t.label.clone())
            .filter(|l| !l.is_empty());
        label.or_else(|| {
            let (_, name) = tab
                .rsplit('/')
                .next()?
                .strip_prefix("tab:")?
                .split_once(':')?;
            (!name.is_empty()).then(|| name.to_string())
        })
    }

    /// `<binding> hide · esc sends to program`, dropped before it can
    /// collide with the title: borders plus a small gap must remain.
    fn float_hint(&self, popup: &PopupState, outer_width: u16) -> Option<String> {
        let command = popup.key.as_ref()?.command.as_str();
        let hint = format!("{command} hide · esc sends to program");
        let need = UnicodeWidthStr::width(hint.as_str())
            + UnicodeWidthStr::width(self.float_title(popup).as_str())
            + 6;
        (usize::from(outer_width) >= need).then_some(hint)
    }
}
