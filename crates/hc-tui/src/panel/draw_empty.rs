//! Empty Lot dock plaque (`─── ✦ ───` / `no studios on the lot` /
//! `n new studio · alt+y hollywood`, or compact `✦ no studios · n new`).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::{display_width, truncate_end};

/// Paint the empty Lot dock hint inside `rect`..`avail_bottom`, using the
/// centered 3-line plaque when `body.height >= 5` and at least 3 rows are free,
/// or the compact single-line hint when shorter.
pub(super) fn draw_empty(
    buf: &mut Buffer,
    body: Rect,
    rect: Rect,
    avail_bottom: u16,
    palette: &Palette,
) {
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    let avail_h = avail_bottom.saturating_sub(rect.y);
    let dim = Style::default().fg(palette.overlay0);
    let muted = Style::default().fg(palette.surface1);
    let sub = Style::default().fg(palette.subtext0);
    let key_style = Style::default()
        .fg(palette.accent)
        .add_modifier(Modifier::BOLD);
    let star = Style::default().fg(palette.accent);

    if body.height >= 5 && avail_h >= 3 {
        let top_y = rect.y + (avail_h - 3) / 2;
        draw_star_rule(buf, rect.x, top_y, rect.width, muted, star);
        draw_centered_line(
            buf,
            rect.x,
            top_y + 1,
            rect.width,
            "no studios on the lot",
            sub,
        );
        draw_plaque_actions(buf, rect.x, top_y + 2, rect.width, key_style, dim);
    } else {
        draw_compact_empty(buf, rect.x, rect.y, rect.width, star, key_style, dim);
    }
}

fn draw_star_rule(buf: &mut Buffer, x: u16, y: u16, width: u16, muted: Style, star: Style) {
    if width >= 9 {
        let cx = x + (width - 9) / 2;
        put_text(buf, cx, y, 4, "─── ", muted);
        put_text(buf, cx + 4, y, 1, "✦", star);
        put_text(buf, cx + 5, y, 4, " ───", muted);
    } else {
        draw_centered_line(buf, x, y, width, "─── ✦ ───", star);
    }
}

fn draw_centered_line(buf: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: Style) {
    let shown = truncate_end(text, width as usize);
    let w = display_width(&shown) as u16;
    let cx = x + width.saturating_sub(w) / 2;
    put_text(buf, cx, y, w.min(width), &shown, style);
}

fn draw_plaque_actions(buf: &mut Buffer, x: u16, y: u16, width: u16, key: Style, dim: Style) {
    const FULL_W: u16 = 30; // `"n new studio · alt+y hollywood"`
    if width >= FULL_W {
        let mut cx = x + (width - FULL_W) / 2;
        put_text(buf, cx, y, 1, "n", key);
        cx += 1;
        put_text(buf, cx, y, 14, " new studio · ", dim);
        cx += 14;
        put_text(buf, cx, y, 5, "alt+y", key);
        cx += 5;
        put_text(buf, cx, y, 10, " hollywood", dim);
    } else {
        draw_centered_line(buf, x, y, width, "n new studio · alt+y hollywood", dim);
    }
}

fn draw_compact_empty(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    star: Style,
    key: Style,
    dim: Style,
) {
    const FULL: &str = "✦ no studios · n new";
    const FULL_W: u16 = 20;
    if width >= FULL_W {
        let mut cx = x + (width - FULL_W) / 2;
        put_text(buf, cx, y, 1, "✦", star);
        cx += 1;
        put_text(buf, cx, y, 14, " no studios · ", dim);
        cx += 14;
        put_text(buf, cx, y, 1, "n", key);
        cx += 1;
        put_text(buf, cx, y, 4, " new", dim);
    } else {
        let shown = truncate_end(FULL, width as usize);
        put_text(buf, x, y, width, &shown, dim);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(buf: &Buffer, area: Rect) -> String {
        (area.y..area.bottom())
            .map(|y| {
                (area.x..area.right())
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn empty_lot_plaque_renders_centered_three_lines_at_tall_and_compact_at_short() {
        let palette = Palette::backlot();
        let tall = Rect::new(0, 0, 34, 7);
        let mut tall_buf = Buffer::empty(tall);
        draw_empty(
            &mut tall_buf,
            tall,
            Rect::new(0, 0, 34, 1),
            tall.bottom(),
            &palette,
        );
        let rendered_tall = text(&tall_buf, tall);
        assert!(rendered_tall.contains("─── ✦ ───"), "{rendered_tall}");
        assert!(
            rendered_tall.contains("no studios on the lot"),
            "{rendered_tall}"
        );
        assert!(
            rendered_tall.contains("n new studio · alt+y hollywood"),
            "{rendered_tall}"
        );

        let short = Rect::new(0, 0, 34, 4);
        let mut short_buf = Buffer::empty(short);
        draw_empty(
            &mut short_buf,
            short,
            Rect::new(0, 0, 34, 1),
            short.bottom(),
            &palette,
        );
        let rendered_short = text(&short_buf, short);
        assert!(
            rendered_short.contains("✦ no studios · n new"),
            "{rendered_short}"
        );
    }

    #[test]
    fn row_empty_renders_through_panel_draw_at_tall_and_short_heights() {
        use crate::panel::{Panel, PanelMode, StudioKeys};

        let palette = Palette::backlot();
        let keys = StudioKeys::default();
        let panel = Panel::new();

        for (h, want) in [
            (10u16, "no studios on the lot"),
            (5u16, "✦ no studios · n new"),
        ] {
            let area = Rect::new(0, 0, 36, h);
            let mut buf = Buffer::empty(area);
            super::super::draw::render(
                &mut buf,
                area,
                &[],
                |_| true,
                |_| true,
                &panel,
                PanelMode::Expanded,
                None,
                None,
                0,
                None,
                &keys,
                None,
                false,
                false,
                true,
                &Default::default(),
                &palette,
                1,
            );
            let out = text(&buf, area);
            assert!(out.contains(want), "height {h} missing `{want}`:\n{out}");
        }
    }
}
