//! Art-Deco framed modal helpers (`╔══╡ <TITLE> ╞═══╗` … `╚═══╝`)
//! and chevron primary buttons (`‹ ↵ action ›`) for Babylon popups.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::palette::Palette;
use super::popup_frame::overlay_bg;
use super::render::{display_width, put_text};
use super::text::truncate_end;

/// Paint double outer borders (`╔═══╗`, `║`, `╚═══╝`) with `border_fg` on
/// top/bottom (and all sides when destructive `border_fg == palette.red`) and
/// `elev.gilt` (`gilt_style`) on the vertical rails of standard dialogs.
pub(crate) fn paint_deco_border(
    buf: &mut Buffer,
    border: Rect,
    border_fg: Color,
    palette: &Palette,
    bg: Color,
) {
    if border.width < 2 || border.height < 2 {
        return;
    }
    let top_bot = Style::default()
        .fg(border_fg)
        .bg(bg)
        .remove_modifier(Modifier::DIM);
    let side = if border_fg == palette.red {
        top_bot
    } else {
        palette
            .elevation()
            .gilt_style(bg)
            .remove_modifier(Modifier::DIM)
    };
    let span = (border.width - 2) as usize;
    let bar = "═".repeat(span);
    put_text(
        buf,
        border.x,
        border.y,
        border.width,
        &format!("╔{bar}╗"),
        top_bot,
    );
    for row in border.y + 1..border.bottom() - 1 {
        put_text(buf, border.x, row, 1, "║", side);
        put_text(buf, border.right() - 1, row, 1, "║", side);
    }
    put_text(
        buf,
        border.x,
        border.bottom() - 1,
        border.width,
        &format!("╚{bar}╝"),
        top_bot,
    );
}

/// Overlay `╔══╡ [◉ ]<title> ╞═══╗` onto the top border of a `PopupChrome::Deco`
/// frame, returning the x coordinate immediately after ` ╞` so callers (such
/// as the New Studio wizard) can append a right-aligned border hint.
pub(crate) fn draw_deco_title(
    buf: &mut Buffer,
    border: Rect,
    title: &str,
    border_fg: Color,
    destructive: bool,
    palette: &Palette,
) -> Option<u16> {
    if border.width < 11 {
        return None;
    }
    let bg = overlay_bg(palette);
    let border_style = Style::default()
        .fg(border_fg)
        .bg(bg)
        .remove_modifier(Modifier::DIM);
    let title_fg = if destructive {
        palette.red
    } else {
        palette.text
    };
    let title_style = Style::default()
        .fg(title_fg)
        .bg(bg)
        .add_modifier(Modifier::BOLD)
        .remove_modifier(Modifier::DIM);

    // Reserve `╔══╡ ` (5) + optional `◉ ` (2) + ` ╞═╗` (4).
    let mark_w: u16 = if destructive { 2 } else { 0 };
    let max_title = border.width.saturating_sub(9 + mark_w);
    if max_title == 0 {
        return None;
    }
    let label = truncate_end(title.trim(), max_title as usize);
    let label_w = display_width(&label);
    if label_w == 0 {
        return None;
    }

    let mut x = border.x;
    put_text(buf, x, border.y, 5, "╔══╡ ", border_style);
    x += 5;
    if destructive {
        let mark_style = Style::default()
            .fg(palette.red)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
            .remove_modifier(Modifier::DIM);
        put_text(buf, x, border.y, 2, "◉ ", mark_style);
        x += 2;
    }
    put_text(buf, x, border.y, label_w, &label, title_style);
    x += label_w;
    put_text(buf, x, border.y, 2, " ╞", border_style);
    x += 2;
    let fill = border.right().saturating_sub(1).saturating_sub(x);
    if fill > 0 {
        put_text(
            buf,
            x,
            border.y,
            fill,
            &"═".repeat(fill as usize),
            border_style,
        );
    }
    put_text(buf, border.right() - 1, border.y, 1, "╗", border_style);
    Some(x)
}

/// Format a primary action button label: `‹ <label> ›` in Deco mode or the
/// untouched Herdr label when `deco == false`.
pub(crate) fn primary_label(label: &str, deco: bool) -> String {
    if deco {
        format!("‹ {} ›", label.trim())
    } else {
        label.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use ratatui::buffer::{Buffer, CellWidth};
    use ratatui::layout::Rect;
    use ratatui::style::Modifier;

    use super::*;
    use crate::chrome::dialogs::{Confirm, Rename, render_confirm_styled, render_rename_styled};
    use crate::chrome::popup_frame::{PopupChrome, dim_backdrop, open_centered};
    use crate::chrome::text_editor::TextEditor;
    use crate::config::PopupBackdrop;

    fn row_str(buf: &Buffer, y: u16, rect: Rect) -> String {
        (rect.x..rect.right())
            .map(|x| buf[(x, y)].symbol())
            .collect()
    }

    #[test]
    fn deco_frame_preserves_gilt_sides_shadow_dim_and_cjk_wide_edges() {
        let palette = Palette::backlot();
        let elev = palette.elevation();
        let area = Rect::new(0, 0, 50, 14);
        let mut buf = Buffer::empty(area);
        // Seed wide CJK graphemes across the dialog boundaries and shadow strip.
        for y in 0..area.height {
            let mut x = 0u16;
            while x + 1 < area.width {
                buf[(x, y)].set_symbol("漢");
                buf[(x + 1, y)].set_symbol("");
                x += 2;
            }
        }
        dim_backdrop(&mut buf, &palette, PopupBackdrop::Dim, None);
        let geom = open_centered(
            &mut buf,
            area,
            30,
            4,
            10,
            4,
            palette.accent,
            PopupChrome::Deco,
            &palette,
            true,
        )
        .unwrap();
        draw_deco_title(
            &mut buf,
            geom.border,
            "SIGN OFF",
            palette.accent,
            false,
            &palette,
        );

        let top = row_str(&buf, geom.border.y, geom.border);
        assert!(top.starts_with("╔══╡ SIGN OFF ╞═"), "{top}");
        assert!(top.ends_with("═╗"), "{top}");
        assert_eq!(buf[(geom.border.x, geom.border.y)].fg, palette.accent);
        assert_eq!(buf[(geom.border.x, geom.border.y + 1)].symbol(), "║");
        assert_eq!(buf[(geom.border.x, geom.border.y + 1)].fg, elev.gilt);
        // Shadow column & bottom shadow row stay single-cell wide without split CJK tails.
        let sx = geom.outer.right();
        for y in geom.outer.y + 1..=geom.outer.bottom() {
            assert_eq!(buf[(sx, y)].cell_width(), 1, "shadow width at y={y}");
        }
        assert_eq!(buf[(geom.outer.x, geom.outer.y)].symbol(), " ");
    }

    #[test]
    fn dialogs_switch_cleanly_between_deco_and_rounded_herdr_chrome() {
        let palette = Palette::catppuccin();
        let rename = Rename {
            title: "rename pane",
            input: TextEditor::new("stage", false),
        };
        let mut deco_r_buf = Buffer::empty(Rect::new(0, 0, 80, 20));
        let dr = render_rename_styled(&mut deco_r_buf, &rename, &palette, true, true).unwrap();
        let dr_text: String = (dr.area.y..dr.area.bottom())
            .map(|y| row_str(&deco_r_buf, y, dr.area))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(dr_text.contains("╔══╡ rename pane ╞"), "{dr_text}");
        assert!(dr_text.contains("‹ ↵ save ›"), "{dr_text}");

        let mut plain_r_buf = Buffer::empty(Rect::new(0, 0, 80, 20));
        let pr = render_rename_styled(&mut plain_r_buf, &rename, &palette, true, false).unwrap();
        assert_eq!(plain_r_buf[(pr.area.x + 1, pr.area.y + 1)].symbol(), "╭");

        let confirm = Confirm {
            title: "Close pane?".into(),
            detail: "Close the focused pane.".into(),
        };
        let mut deco_c_buf = Buffer::empty(Rect::new(0, 0, 80, 20));
        let dc = render_confirm_styled(&mut deco_c_buf, &confirm, &palette, true, true).unwrap();
        let dc_text: String = (dc.area.y..dc.area.bottom())
            .map(|y| row_str(&deco_c_buf, y, dc.area))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(dc_text.contains("╔══╡ ◉ Close pane? ╞"), "{dc_text}");
        assert!(dc_text.contains("‹ ↵ confirm ›"), "{dc_text}");
        assert_eq!(deco_c_buf[(dc.primary.x, dc.primary.y)].bg, palette.red);
        assert!(
            deco_c_buf[(dc.primary.x + 2, dc.primary.y)]
                .modifier
                .contains(Modifier::BOLD)
        );
    }
}
