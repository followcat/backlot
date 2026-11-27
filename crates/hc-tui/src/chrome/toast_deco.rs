//! Art-Deco framed action toast painter (`╔═╡ ● <title> ╞═══╗` on opaque
//! `elev.overlay`, with `elev.gilt` borders for successes/stage offers and
//! `palette.red` + `◉` for failures).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use unicode_width::UnicodeWidthStr;

use super::notice_render::ToastChip;
use super::palette::Palette;
use super::render::put_text;
use super::text::truncate_end;

/// Format toast title for Babylon's Deco banner (`"finished <studio>"` becomes
/// `"<studio> · THE END"`).
pub(crate) fn format_title(title: &str) -> String {
    if let Some(studio) = title.strip_prefix("finished ").filter(|s| !s.is_empty()) {
        format!("{studio} · THE END")
    } else if let Some((machine, studio)) = title
        .split_once(": finished ")
        .filter(|(_, s)| !s.is_empty())
    {
        format!("{machine}: {studio} · THE END")
    } else {
        title.to_owned()
    }
}

/// Render the action toast in Deco style (`╔═╡ … ╞═══╗`).
/// Preserves the top-right anchoring, 3-line / 5-line height, and `chip` hit
/// rectangle of [`super::notice_render::render_action_toast`].
pub(crate) fn render_action_toast_deco(
    buffer: &mut Buffer,
    area: Rect,
    title: &str,
    body: Option<&str>,
    chip: Option<ToastChip>,
    top_offset: u16,
    palette: &Palette,
) -> (Rect, Rect) {
    if area.is_empty() {
        return (Rect::default(), Rect::default());
    }
    let fail = chip == Some(ToastChip::CopyCmd);
    let deco_title = format_title(title);
    let hint = chip.map(|c| format!("{}  [esc] dismiss", c.text()));
    let header_width = UnicodeWidthStr::width(deco_title.as_str()).saturating_add(10);
    let body_width = body.map_or(0, UnicodeWidthStr::width).saturating_add(6);
    let hint_width = hint
        .as_deref()
        .map_or(0, UnicodeWidthStr::width)
        .saturating_add(6);
    let width = u16::try_from(header_width.max(body_width).max(hint_width))
        .unwrap_or(u16::MAX)
        .min(area.width);
    let lines: u16 = match body {
        Some(_) => 3,
        None => 1,
    };
    let height = (lines + 2).min(area.height);
    let x = area.right().saturating_sub(width);
    let max_y = area.bottom().saturating_sub(height).max(area.y);
    let y = area.y.saturating_add(top_offset).clamp(area.y, max_y);
    let rect = Rect::new(x, y, width, height);

    super::wide::clear_overlay(buffer, rect);
    let elev = palette.elevation();
    let bg = elev.overlay;
    let fill = Style::default().bg(bg).remove_modifier(Modifier::DIM);
    for row in rect.y..rect.bottom() {
        for col in rect.x..rect.right() {
            buffer[(col, row)].set_symbol(" ").set_style(fill);
        }
    }

    let border_style = if fail {
        Style::default()
            .fg(palette.red)
            .bg(bg)
            .remove_modifier(Modifier::DIM)
    } else {
        elev.gilt_style(bg).remove_modifier(Modifier::DIM)
    };
    paint_deco_toast_frame(buffer, rect, &deco_title, fail, border_style, bg, palette);

    let text_w = rect.width.saturating_sub(6);
    let text_x = rect.x.saturating_add(3);
    let mut chip_hit = Rect::default();
    if let Some(body_text) = body {
        if rect.height >= 4 && text_w > 0 && !body_text.is_empty() {
            put_text(
                buffer,
                text_x,
                rect.y + 1,
                text_w,
                &truncate_end(body_text, text_w as usize),
                Style::default().fg(palette.overlay0).bg(bg),
            );
        }
        if rect.height >= 5
            && let Some(chip) = chip
            && text_w > 0
        {
            let hint_y = rect.y + 3;
            let chip_text = chip.text();
            let chip_w = UnicodeWidthStr::width(chip_text) as u16;
            put_text(
                buffer,
                text_x,
                hint_y,
                text_w,
                chip_text,
                Style::default().fg(palette.teal).bg(bg),
            );
            let rem_w = text_w.saturating_sub(chip_w);
            if rem_w > 0 {
                put_text(
                    buffer,
                    text_x + chip_w,
                    hint_y,
                    rem_w,
                    "  [esc] dismiss",
                    Style::default().fg(palette.overlay0).bg(bg),
                );
            }
            chip_hit = Rect::new(text_x, hint_y, chip_w.min(text_w), 1);
        }
    } else if rect.height >= 3 && text_w >= 13 {
        put_text(
            buffer,
            text_x,
            rect.y + 1,
            text_w,
            "[esc] dismiss",
            Style::default().fg(palette.overlay0).bg(bg),
        );
    }

    super::overlay::fill_default_bg(buffer, rect, palette);
    (rect, chip_hit)
}

fn paint_deco_toast_frame(
    buffer: &mut Buffer,
    rect: Rect,
    title: &str,
    fail: bool,
    border_style: Style,
    bg: ratatui::style::Color,
    palette: &Palette,
) {
    if rect.width < 2 || rect.height == 0 {
        return;
    }
    if rect.width >= 11 {
        let max_t = rect.width.saturating_sub(10) as usize;
        let shown = truncate_end(title, max_t);
        let shown_w = UnicodeWidthStr::width(shown.as_str()) as u16;
        let mut cx = rect.x;
        put_text(buffer, cx, rect.y, 4, "╔═╡ ", border_style);
        cx += 4;
        let (mark, mark_fg) = if fail {
            ("◉", palette.red)
        } else {
            ("●", palette.green)
        };
        let mut mark_style = Style::default().fg(mark_fg).bg(bg);
        if fail {
            mark_style = mark_style.add_modifier(Modifier::BOLD);
        }
        put_text(buffer, cx, rect.y, 1, mark, mark_style);
        cx += 1;
        put_text(buffer, cx, rect.y, 1, " ", Style::default().bg(bg));
        cx += 1;
        put_text(
            buffer,
            cx,
            rect.y,
            shown_w,
            &shown,
            Style::default()
                .fg(palette.text)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        );
        cx += shown_w;
        put_text(buffer, cx, rect.y, 2, " ╞", border_style);
        cx += 2;
        let fill = rect.right().saturating_sub(1).saturating_sub(cx);
        if fill > 0 {
            put_text(
                buffer,
                cx,
                rect.y,
                fill,
                &"═".repeat(fill as usize),
                border_style,
            );
        }
        put_text(buffer, rect.right() - 1, rect.y, 1, "╗", border_style);
    } else {
        let bar = "═".repeat((rect.width - 2) as usize);
        put_text(
            buffer,
            rect.x,
            rect.y,
            rect.width,
            &format!("╔{bar}╗"),
            border_style,
        );
    }

    for row in rect.y + 1..rect.bottom().saturating_sub(1) {
        put_text(buffer, rect.x, row, 1, "║", border_style);
        put_text(buffer, rect.right() - 1, row, 1, "║", border_style);
    }
    if rect.height >= 2 {
        let bar = "═".repeat((rect.width - 2) as usize);
        put_text(
            buffer,
            rect.x,
            rect.bottom() - 1,
            rect.width,
            &format!("╚{bar}╝"),
            border_style,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_text(buf: &Buffer, rect: Rect, dy: u16) -> String {
        (rect.x..rect.right())
            .map(|x| buf[(x, rect.y + dy)].symbol())
            .collect()
    }

    #[test]
    fn success_and_failure_and_the_end_toasts_render_deco() {
        let palette = Palette::backlot();
        let elev = palette.elevation();
        let area = Rect::new(0, 0, 80, 20);

        let mut ok_buf = Buffer::empty(area);
        let (ok_rect, chip) = render_action_toast_deco(
            &mut ok_buf,
            area,
            "priority task-1 → high",
            Some("recorded on disk · stage stopped"),
            Some(ToastChip::StartStage),
            0,
            &palette,
        );
        assert_eq!(ok_rect.height, 5);
        assert!(chip.height == 1 && !chip.is_empty());
        assert_eq!(ok_buf[(ok_rect.x, ok_rect.y)].fg, elev.gilt);
        assert_eq!(ok_buf[(ok_rect.x, ok_rect.y)].bg, elev.overlay);
        assert!(row_text(&ok_buf, ok_rect, 0).starts_with("╔═╡ ● priority task-1 → high ╞"));
        assert!(row_text(&ok_buf, ok_rect, 0).ends_with("═╗"));

        let mut fail_buf = Buffer::empty(area);
        let (fail_rect, _) = render_action_toast_deco(
            &mut fail_buf,
            area,
            "hollywood switch failed",
            Some("quota exceeded"),
            Some(ToastChip::CopyCmd),
            0,
            &palette,
        );
        assert_eq!(fail_buf[(fail_rect.x, fail_rect.y)].fg, palette.red);
        assert!(row_text(&fail_buf, fail_rect, 0).starts_with("╔═╡ ◉ hollywood switch failed ╞"));

        let mut end_buf = Buffer::empty(area);
        let (end_rect, _) = render_action_toast_deco(
            &mut end_buf,
            area,
            "finished acme-shop",
            None,
            None,
            0,
            &palette,
        );
        assert_eq!(end_rect.height, 3);
        assert_eq!(
            row_text(&end_buf, end_rect, 0),
            "╔═╡ ● acme-shop · THE END ╞═╗"
        );

        let mut plain_buf = Buffer::empty(area);
        let (plain_rect, _) = super::super::notice_render::render_action_toast(
            &mut plain_buf,
            area,
            "hollywood switch failed",
            Some("quota exceeded"),
            Some(ToastChip::CopyCmd),
            0,
            &palette,
            false,
        );
        assert_eq!(plain_buf[(plain_rect.x, plain_rect.y)].symbol(), "┌");
        assert_eq!(plain_buf[(plain_rect.x, plain_rect.y + 1)].symbol(), "│");
    }
}
