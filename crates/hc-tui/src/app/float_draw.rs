// Fresh code for hc-tui's per-tab floating terminal.
//! The persistent float's frame dressing: rich title and the
//! focused/unfocused border split. Backdrop dimming, outer gutter,
//! and drop shadows live in `chrome/popup_frame.rs`.

use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders};
use unicode_width::UnicodeWidthStr;

use super::popup::PopupState;
use super::*;
use crate::chrome::snapshot::ClientEndpointId;

impl App {
    /// The border color: accent while the float owns input. A visible
    /// float loses its keys only while `keyboard_locked` — the lock arm
    /// runs before the popup's — which is the one muted-border state.
    pub(super) fn float_border_color(&self, persistent: bool) -> ratatui::style::Color {
        let focused = !self.keyboard_locked;
        if !persistent || focused {
            self.shell.palette.accent
        } else {
            self.shell.palette.elevation().border_muted
        }
    }

    /// The float's block: rounded borders, left title
    /// `float <binding> — <tab>[ — <program>]`, right-aligned hint
    /// `<binding> hide · esc sends to program` while it still fits clear
    /// of the title.
    pub(super) fn float_block(&self, border: Rect) -> Block<'static> {
        let Some(popup) = self.popup.as_ref() else {
            return Block::default().borders(Borders::ALL);
        };
        let palette = &self.shell.palette;
        let overlay_bg = crate::chrome::popup_frame::overlay_bg(palette);
        // ratatui lays titles on the border row between the corner
        // glyphs — `inner_w` — and lets a right-aligned title overwrite
        // a left title's tail when they overlap. The hint therefore
        // only renders when both padded titles plus a separator cell
        // fit; the title is then ellipsized into what remains, always
        // leaving a cell clear before ╮.
        let inner_w = usize::from(border.width.saturating_sub(2));
        let full_title = self.float_title(popup);
        let hint = self.float_hint(popup, inner_w, &full_title);
        let title_budget = inner_w.saturating_sub(3).saturating_sub(
            hint.as_ref()
                .map_or(0, |h| UnicodeWidthStr::width(h.as_str()) + 2),
        );
        let title = crate::chrome::text::truncate_end(&full_title, title_budget);
        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(
                Style::default()
                    .fg(self.float_border_color(popup.persistent))
                    .bg(overlay_bg),
            )
            .style(Style::default().bg(overlay_bg))
            .title(format!(" {title} "));
        if let Some(hint) = hint {
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

    /// `<binding> hide · esc sends to program`, rendered only when the
    /// full title, both titles' one-cell padding, and one separator
    /// cell still fit between the corners — `title + hint + 5 <=
    /// inner_w`. Any tighter and ratatui would let the hint clip the
    /// title's tail.
    fn float_hint(&self, popup: &PopupState, inner_w: usize, title: &str) -> Option<String> {
        let command = popup.key.as_ref()?.command.as_str();
        let hint = format!("{command} hide · esc sends to program");
        let need = UnicodeWidthStr::width(title) + UnicodeWidthStr::width(hint.as_str()) + 5;
        (inner_w >= need).then_some(hint)
    }
}
