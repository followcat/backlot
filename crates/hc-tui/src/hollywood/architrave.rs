//! Ornamental architrave and marquee sill chrome for the Hollywood band:
//! - top width ladder (`>= 120`: `╔═╡ ‹✦› H O L L Y W O O D ‹✦› ╞═ … ╞═╗`,
//!   `100..=119`: `╔═╡ ✦ H O L L Y W O O D ╞═ … ╞═╗`,
//!   `< 100`: `╔╡✦ HOLLYWOOD╞═ … ╞╗`) on `elev.stage_well`;
//! - bottom marquee sill `╚══▪══ … ╡ ✦ ╞ … ══▪══╝` on `elev.stage_well`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::Target;
use super::draw::HollywoodRender;
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::display_width;

pub(super) fn width_of(text: &str) -> u16 {
    display_width(text) as u16
}

/// Geometry and styles resolved for the top architrave row.
pub(super) struct TitleFrame {
    pub(super) x: u16,
    pub(super) right_cap: &'static str,
    pub(super) right_cap_w: u16,
    pub(super) clasp_w: u16,
    pub(super) base: Style,
    brass: Style,
}

impl TitleFrame {
    pub(super) fn right_start(&self, area: Rect, right_w: u16) -> u16 {
        let right_total = if right_w > 0 {
            self.clasp_w + right_w + self.right_cap_w
        } else {
            self.right_cap_w
        };
        (area.x + area.width).saturating_sub(right_total)
    }
}

/// Bottom marquee sill `╚══▪══ … ╡ ✦ ╞ … ══▪══╝` on `elev.stage_well`,
/// mirror-symmetric around the centered `╡ ✦ ╞` clasp with `▪` every 3 cells.
pub(crate) fn draw_marquee_sill(buf: &mut Buffer, area: Rect, y: u16, palette: &Palette) {
    let w = area.width;
    if w == 0 {
        return;
    }
    let elev = palette.elevation();
    let well = elev.stage_well;
    buf.set_style(Rect::new(area.x, y, w, 1), Style::default().bg(well));
    let brass = elev.gilt_style(well);
    let bulb = Style::default().fg(elev.bulb_warm).bg(well);
    let gem = Style::default()
        .fg(palette.accent)
        .bg(well)
        .add_modifier(Modifier::BOLD);

    if w == 1 {
        put_text(buf, area.x, y, 1, "╚", brass);
        return;
    }
    if w == 2 {
        put_text(buf, area.x, y, 2, "╚╝", brass);
        return;
    }

    put_text(buf, area.x, y, 1, "╚", brass);
    put_text(buf, area.x + w - 1, y, 1, "╝", brass);

    let (clasp_x, clasp_right) = if w >= 11 {
        let cx = area.x + (w - 5) / 2;
        put_text(buf, cx, y, 2, "╡ ", brass);
        put_text(buf, cx + 2, y, 1, "✦", gem);
        put_text(buf, cx + 3, y, 2, " ╞", brass);
        (cx, cx + 5)
    } else if w >= 5 {
        let cx = area.x + w / 2;
        put_text(buf, cx, y, 1, "✦", gem);
        (cx, cx + 1)
    } else {
        for x in area.x + 1..area.x + w - 1 {
            put_text(buf, x, y, 1, "═", brass);
        }
        return;
    };

    for x in area.x + 1..clasp_x {
        let dist = clasp_x - x;
        if dist.is_multiple_of(3) {
            put_text(buf, x, y, 1, "▪", bulb);
        } else {
            put_text(buf, x, y, 1, "═", brass);
        }
    }
    for x in clasp_right..area.x + w - 1 {
        let dist = x - clasp_right + 1;
        if dist.is_multiple_of(3) {
            put_text(buf, x, y, 1, "▪", bulb);
        } else {
            put_text(buf, x, y, 1, "═", brass);
        }
    }
}

fn draw_title_prefix(buf: &mut Buffer, area: Rect, y: u16, palette: &Palette) -> u16 {
    let elev = palette.elevation();
    let well = elev.stage_well;
    let brass = elev.gilt_style(well);
    let mark_style = Style::default()
        .fg(palette.accent)
        .bg(well)
        .add_modifier(Modifier::BOLD);
    let mut x = area.x;
    let limit = area.x + area.width;
    let put = |b: &mut Buffer, cx: &mut u16, s: &str, st: Style| {
        let w = display_width(s) as u16;
        let take = w.min(limit.saturating_sub(*cx));
        if take > 0 {
            put_text(b, *cx, y, take, s, st);
            *cx += take;
        }
    };
    if area.width >= 120 {
        put(buf, &mut x, "╔═╡ ", brass);
        put(buf, &mut x, "‹", brass);
        put(buf, &mut x, "✦", mark_style);
        put(buf, &mut x, "›", brass);
        put(buf, &mut x, " H O L L Y W O O D ", mark_style);
        put(buf, &mut x, "‹", brass);
        put(buf, &mut x, "✦", mark_style);
        put(buf, &mut x, "›", brass);
        put(buf, &mut x, " ╞═ ", brass);
    } else if area.width >= 100 {
        put(buf, &mut x, "╔═╡ ", brass);
        put(buf, &mut x, "✦ H O L L Y W O O D", mark_style);
        put(buf, &mut x, " ╞═ ", brass);
    } else {
        put(buf, &mut x, "╔╡", brass);
        put(buf, &mut x, "✦ HOLLYWOOD", mark_style);
        put(buf, &mut x, "╞═ ", brass);
    }
    x
}

/// Fill the title row background, register its hit rect, draw the left width-ladder
/// plaque, and resolve right-cap frame geometry.
pub(super) fn init_title_frame(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    palette: &Palette,
    hits: &mut HollywoodRender,
) -> TitleFrame {
    let row = Rect::new(area.x, y, area.width, 1);
    hits.title = row;
    hits.rows.push((row, Target::Title));
    let elev = palette.elevation();
    let well = elev.stage_well;
    let base = Style::default().bg(well);
    let brass = elev.gilt_style(well);
    buf.set_style(row, base);

    let x = draw_title_prefix(buf, area, y, palette);
    let right_cap = if area.width >= 100 {
        " ╞═╗"
    } else {
        " ╞╗"
    };
    TitleFrame {
        x,
        right_cap,
        right_cap_w: width_of(right_cap),
        clasp_w: 2, // `"╡ "`
        base,
        brass,
    }
}

/// Draw the gilt `═` beam, optional `╡ <right_segs>` plaque, and closing `╞═╗`/`╞╗` cap.
pub(super) fn finish_title_frame(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    frame: &TitleFrame,
    beam_start: u16,
    chosen_right: &[(String, Style)],
    right_w: u16,
) {
    let right_start = frame.right_start(area, right_w);
    if beam_start < right_start {
        let fill = right_start - beam_start;
        put_text(
            buf,
            beam_start,
            y,
            fill,
            &"═".repeat(fill as usize),
            frame.brass,
        );
    }

    if right_w > 0 && right_start >= frame.x {
        put_text(buf, right_start, y, frame.clasp_w, "╡ ", frame.brass);
        let mut rx = right_start + frame.clasp_w;
        for (seg, style) in chosen_right {
            let sw = width_of(seg);
            put_text(buf, rx, y, sw, seg, *style);
            rx += sw;
        }
        put_text(buf, rx, y, frame.right_cap_w, frame.right_cap, frame.brass);
    } else if area.width >= frame.right_cap_w {
        let cap_x = (area.x + area.width).saturating_sub(frame.right_cap_w);
        put_text(
            buf,
            cap_x,
            y,
            frame.right_cap_w,
            frame.right_cap,
            frame.brass,
        );
    }
}
