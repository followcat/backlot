//! Inline `t` composer & lifecycle states for the Lot Reader (`spec.md` §4).

use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::reader_chrome::draw_divider;
use super::reader_meta::{ReaderMeta, ReaderOrigin, is_fixed_seat};
use crate::chrome::help::CursorState;
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::{display_width, truncate_end};
use crate::chrome::text_editor::TextEditor;

/// Ephemeral inline composer / reply status footer state inside an open Reader.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum InlineTellState {
    #[default]
    Reading,
    Composing,
    Sending {
        action_id: u64,
        target: String,
        text: String,
    },
    Sent {
        target: String,
        until: Instant,
    },
    Failed {
        err: String,
    },
}

impl InlineTellState {
    /// Whether text input is actively captured by the Reader's composer.
    pub(crate) fn is_composing(&self) -> bool {
        matches!(self, Self::Composing)
    }

    /// Whether the 2-row compose/status band is expanded at the bottom of the Reader.
    pub(crate) fn has_band(&self) -> bool {
        !matches!(self, Self::Reading)
    }
}

/// Optional live compose context passed during Reader frame rendering.
#[derive(Clone, Copy)]
pub(crate) struct ReaderComposeOpts<'a> {
    pub editor: Option<&'a TextEditor>,
    pub target: &'a str,
    pub studio: bool,
}

/// Resolve `(target_label, empty_draft_seed)` for telling from an open Reader entry.
/// Non-producer live studio roles and closed fixed seats seed `/tell <role> `;
/// closed non-fixed actors route cleanly to `producer` (`None` seed).
pub(crate) fn reader_tell_target(meta: &ReaderMeta) -> (String, Option<String>) {
    match &meta.origin {
        ReaderOrigin::HollywoodExchange(_) => ("hollywood".to_owned(), None),
        ReaderOrigin::StudioExchange(..) => {
            if let Some(role) = meta.role.as_deref()
                && !role.is_empty()
                && role != "producer"
                && role != "user"
                && (meta.role_state.as_deref() != Some("closed") || is_fixed_seat(role))
            {
                let label = role.strip_prefix("actor-").unwrap_or(role);
                return (label.to_owned(), Some(format!("/tell {role} ")));
            }
            ("producer".to_owned(), None)
        }
        ReaderOrigin::SheetDeliveries(..) | ReaderOrigin::Records(_) | ReaderOrigin::Card(_) => {
            ("producer".to_owned(), None)
        }
        ReaderOrigin::None => ("producer".to_owned(), None),
    }
}

/// Render the divider (`divider_y`) and compose/status row (`divider_y + 1`),
/// returning the clickable compose row rect and optional hardware cursor state.
pub(super) fn draw_compose_band(
    buf: &mut Buffer,
    area: Rect,
    divider_y: u16,
    state: &InlineTellState,
    compose: Option<ReaderComposeOpts<'_>>,
    edge: Style,
    palette: &Palette,
) -> (Option<Rect>, Option<CursorState>) {
    let row_y = divider_y + 1;
    draw_divider(buf, area, divider_y, edge, palette);
    if area.width <= 4 {
        return (None, None);
    }
    let inner = Rect::new(area.x + 2, row_y, area.width - 4, 1);
    let bg = Style::default().bg(palette.panel_bg);
    buf.set_style(Rect::new(area.x + 1, row_y, area.width - 2, 1), bg);

    match state {
        InlineTellState::Reading => (None, None),
        InlineTellState::Composing => {
            let target = compose.map(|c| c.target).unwrap_or("producer");
            let prefix = format!("› tell {target}: ");
            let prefix_w = display_width(&prefix);
            let max_prefix = usize::from(inner.width.saturating_sub(4));
            let fitted_prefix = truncate_end(&prefix, max_prefix);
            let used = display_width(&fitted_prefix) as u16;
            put_text(
                buf,
                inner.x,
                row_y,
                used,
                &fitted_prefix,
                bg.fg(palette.accent).add_modifier(Modifier::BOLD),
            );
            let field_w = inner.width.saturating_sub(used);
            let field = Rect::new(inner.x + used, row_y, field_w, 1);
            let empty_editor = TextEditor::default();
            let editor = compose.and_then(|c| c.editor).unwrap_or(&empty_editor);
            let cursor = if field_w > 0 {
                crate::chrome::text_editor::render(buf, field, editor, bg.fg(palette.text)).map(
                    |mut cs| {
                        cs.visible = true;
                        cs
                    },
                )
            } else {
                let _ = prefix_w;
                None
            };
            (Some(inner), cursor)
        }
        InlineTellState::Sending { target, .. } => {
            let msg = format!("■ sending to {target}…");
            let fitted = truncate_end(&msg, usize::from(inner.width));
            put_text(
                buf,
                inner.x,
                row_y,
                display_width(&fitted) as u16,
                &fitted,
                bg.fg(palette.yellow),
            );
            (None, None)
        }
        InlineTellState::Sent { target, .. } => {
            let msg = format!("■ sent to {target}");
            let fitted = truncate_end(&msg, usize::from(inner.width));
            put_text(
                buf,
                inner.x,
                row_y,
                display_width(&fitted) as u16,
                &fitted,
                bg.fg(palette.green),
            );
            (None, None)
        }
        InlineTellState::Failed { err } => {
            let first_line = err.lines().next().unwrap_or(err.as_str()).trim();
            let msg = if first_line.is_empty() {
                "■ send failed — ⏎ retry · esc keep draft".to_owned()
            } else {
                format!("■ send failed: {first_line} — ⏎ retry · esc keep draft")
            };
            let fitted = truncate_end(&msg, usize::from(inner.width));
            put_text(
                buf,
                inner.x,
                row_y,
                display_width(&fitted) as u16,
                &fitted,
                bg.fg(palette.red),
            );
            (Some(inner), None)
        }
    }
}

/// Render tiered key hints on the bottom strip while inline `Composing` is active.
pub(super) fn draw_composing_hints(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    studio: bool,
    palette: &Palette,
) {
    if area.width <= 2 {
        return;
    }
    let strip = Rect::new(area.x + 1, y, area.width - 2, 1);
    let bg = Style::default().bg(palette.surface0);
    buf.set_style(strip, bg);
    let avail = usize::from(area.width.saturating_sub(4));
    if avail == 0 {
        return;
    }
    let tiers: &[&str] = if studio {
        &[
            "⏎ send  ↑↓ history  tab /verb  esc keep draft",
            "⏎ send  ↑↓ history  esc keep draft",
            "⏎ send  ↑↓ history  esc",
            "⏎ send  esc",
        ]
    } else {
        &[
            "⏎ send  ↑↓ history  esc keep draft",
            "⏎ send  ↑↓ history  esc",
            "⏎ send  esc",
        ]
    };
    let chosen = tiers
        .iter()
        .copied()
        .find(|t| display_width(t) <= avail)
        .unwrap_or("⏎ send");
    let fitted = truncate_end(chosen, avail);
    put_text(
        buf,
        area.x + 2,
        y,
        display_width(&fitted) as u16,
        &fitted,
        bg.fg(palette.subtext0),
    );
}
