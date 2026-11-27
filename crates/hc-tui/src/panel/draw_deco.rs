//! Babylon Deco Lot dock outer seam and `✦ L O T · N` header on `stage_well` (S4 §1–§2).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::config::StudioKeys;
use super::data::{MachineSummary, studio_count};
use super::{Panel, PanelMode};
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::{display_width, truncate_end};

#[allow(clippy::too_many_arguments)]
pub(super) fn render_lot_seam_and_header(
    buf: &mut Buffer,
    area: Rect,
    machines: &[MachineSummary],
    panel: &Panel,
    mode: PanelMode,
    keys: &StudioKeys,
    overlay: bool,
    pane_refuses: bool,
    lot_focused: bool,
    palette: &Palette,
) -> Rect {
    let elev = palette.elevation();
    let entered_focus = lot_focused && panel.entered_studio.is_some();
    let deco = !panel.plain_chrome;
    let header_bg = if deco { elev.stage_well } else { elev.surface };

    if area.width > 1 {
        let seam_fg = if lot_focused {
            palette.accent
        } else {
            elev.border_muted
        };
        for y in area.y..area.bottom() {
            let sym = if entered_focus && y == area.y {
                "┏"
            } else if entered_focus && area.height > 1 && y + 1 == area.bottom() {
                "┗"
            } else if entered_focus {
                "┃"
            } else {
                "│"
            };
            let bg = if y == area.y { header_bg } else { elev.surface };
            if let Some(cell) = buf.cell_mut((area.x, y)) {
                cell.set_symbol(sym);
                cell.set_style(Style::default().fg(seam_fg).bg(bg));
            }
        }
    }

    let inner = if area.width > 1 {
        Rect::new(area.x + 1, area.y, area.width - 1, area.height)
    } else {
        area
    };
    let title = Rect::new(inner.x, inner.y, inner.width, 1);
    buf.set_style(title, Style::default().bg(header_bg));

    let studios = studio_count(machines);
    let title_fg = if lot_focused {
        palette.accent
    } else {
        palette.text
    };

    if !deco {
        let title_text = match mode {
            PanelMode::Collapsed if entered_focus => format!(" ▸ lot {studios}"),
            PanelMode::Collapsed => format!(" lot {studios}"),
            _ if entered_focus => format!(" ▸ lot · {studios} "),
            _ => format!(" lot · {studios}"),
        };
        put_text(
            buf,
            title.x,
            title.y,
            title.width,
            &title_text,
            Style::default()
                .fg(title_fg)
                .bg(header_bg)
                .add_modifier(Modifier::BOLD),
        );
        let hint_room = title
            .width
            .saturating_sub(display_width(&title_text) as u16 + 2) as usize;
        let hint = super::draw_hints::header_hint(
            panel.focused,
            overlay,
            pane_refuses,
            mode,
            keys,
            area.width,
            hint_room,
        );
        let hint_w = hint.as_deref().map_or(0, display_width) as u16;
        if entered_focus && title.width >= 8 {
            draw_entered_header_tail(
                buf,
                title,
                &title_text,
                hint.as_deref(),
                hint_w,
                palette,
                header_bg,
            );
        } else if let Some(hint) = hint {
            put_text(
                buf,
                title.x + title.width - hint_w,
                title.y,
                hint_w,
                &hint,
                Style::default().fg(elev.muted_surface).bg(header_bg),
            );
        }
        return title;
    }

    let lead = if entered_focus { "▸" } else { "✦" };
    let wide_label = format!(" L O T · {studios}");
    let tight_label = format!(" LOT·{studios}");
    let suffix = if entered_focus { " " } else { "" };
    let full_len = 2 + display_width(&wide_label) + suffix.len();
    let label = if title.width >= full_len as u16 + 4 || title.width < 10 {
        wide_label
    } else {
        tight_label
    };
    let title_text = format!(" {lead}{label}{suffix}");

    if title.width >= 2 {
        put_text(
            buf,
            title.x,
            title.y,
            1,
            " ",
            Style::default().bg(header_bg),
        );
        put_text(
            buf,
            title.x + 1,
            title.y,
            1,
            lead,
            Style::default()
                .fg(palette.accent)
                .bg(header_bg)
                .add_modifier(Modifier::BOLD),
        );
    }
    let after_lead = title.x.saturating_add(2);
    let label_room = title.right().saturating_sub(after_lead);
    if label_room > 0 {
        let rest = format!("{label}{suffix}");
        put_text(
            buf,
            after_lead,
            title.y,
            label_room,
            &truncate_end(&rest, label_room as usize),
            Style::default()
                .fg(title_fg)
                .bg(header_bg)
                .add_modifier(Modifier::BOLD),
        );
    }

    let hint_room = title
        .width
        .saturating_sub(display_width(&title_text) as u16 + 2) as usize;
    let hint = if entered_focus {
        if hint_room >= "esc studios".len() {
            Some("esc studios".to_string())
        } else if hint_room >= 3 {
            Some("esc".to_string())
        } else {
            None
        }
    } else {
        super::draw_hints::header_hint(
            panel.focused,
            overlay,
            pane_refuses,
            mode,
            keys,
            area.width,
            hint_room,
        )
    };
    let hint_w = hint.as_deref().map_or(0, display_width) as u16;
    if entered_focus && title.width >= 8 {
        draw_entered_header_tail(
            buf,
            title,
            &title_text,
            hint.as_deref(),
            hint_w,
            palette,
            header_bg,
        );
    } else if let Some(hint) = hint {
        put_text(
            buf,
            title.x + title.width - hint_w,
            title.y,
            hint_w,
            &hint,
            Style::default().fg(elev.muted_surface).bg(header_bg),
        );
    }
    title
}

fn draw_entered_header_tail(
    buf: &mut Buffer,
    title: Rect,
    title_text: &str,
    hint: Option<&str>,
    hint_w: u16,
    palette: &Palette,
    header_bg: Color,
) {
    let elev = palette.elevation();
    let cap_end = title.x + title.width - 1;
    put_text(
        buf,
        cap_end,
        title.y,
        1,
        "┓",
        Style::default().fg(palette.accent).bg(header_bg),
    );
    let fill_start = title.x + (display_width(title_text) as u16).min(title.width);
    let fill_end = cap_end.saturating_sub(hint_w + u16::from(hint_w > 0));
    if fill_end > fill_start {
        put_text(
            buf,
            fill_start,
            title.y,
            fill_end - fill_start,
            &"━".repeat((fill_end - fill_start) as usize),
            Style::default().fg(palette.accent).bg(header_bg),
        );
    }
    if let Some(hint) = hint {
        put_text(
            buf,
            cap_end.saturating_sub(hint_w),
            title.y,
            hint_w,
            hint,
            Style::default().fg(elev.muted_surface).bg(header_bg),
        );
    }
}
