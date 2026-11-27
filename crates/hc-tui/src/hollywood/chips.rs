//! The one-row studio chip strip (`STUDIOS N  [▸acme-shop ✓ 3c] … [+ new]`):
//! active mark `▸`, health glyph, per-studio waiting/progress badge,
//! uneven-width horizontal scroll with pinned `[+ new]`, and focused
//! `▌` styling when too short for expanded 3-row frames.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::Target;
use super::band::MARGIN;
use super::items::WaitBucket;
use super::rows::{Frame, wait_studio_key};
use super::sheet::{Conn, Status, Wait};
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::display_width;
use crate::panel::data::{StudioRef, studio_label};
use crate::studio::StudioKey;

const NEW_CHIP_TEXT: &str = "[+ new]";
const NEW_CHIP_W: u16 = 7;

/// What the strip drew: the chip rects, label rect, studio count, and overflow.
#[derive(Default)]
pub(crate) struct ChipsRender {
    pub hits: Vec<(Rect, Target)>,
    pub label_rect: Rect,
    pub drawn_studios: usize,
    pub more_left: bool,
    pub more_right: bool,
}

#[derive(Clone, Copy)]
struct StripView<'a> {
    active_studio: Option<&'a StudioKey>,
    narrow: bool,
    local: Option<&'a str>,
    multi: bool,
}

fn studio_segs(
    studio: StudioRef<'_>,
    waits: &[Wait<'_>],
    status: &Status<'_>,
    view: StripView<'_>,
    palette: &Palette,
) -> Vec<(String, Color, bool)> {
    let elev = palette.elevation();
    let label = studio_label(&studio.studio.id, studio.machine, view.local, view.multi);
    let (conn, stage) = status(studio.machine);
    let health = crate::panel::health_of(studio.studio, stage, conn != Conn::Offline);
    let key = (studio.machine, studio.studio.id.as_str());
    let (mut b, mut a, mut c) = (0usize, 0usize, 0usize);
    for w in waits {
        if wait_studio_key(w) == Some(key) {
            match WaitBucket::of(w) {
                WaitBucket::Blocking => b += 1,
                WaitBucket::Asks => a += 1,
                WaitBucket::Cards => c += 1,
                WaitBucket::Ready => {}
            }
        }
    }
    let mut segs = Vec::new();
    segs.push(("[".into(), elev.muted_surface, false));
    if view.active_studio == Some(&studio.key()) {
        segs.push(("▸".into(), palette.accent, false));
    }
    segs.push((label.fit(18), palette.text, false));
    segs.push((" ".into(), palette.text, false));
    segs.push((health.glyph().to_string(), health.color(palette), false));
    if b > 0 {
        segs.push((format!(" !{b}"), palette.red, true));
    } else if a > 0 {
        segs.push((format!(" ?{a}"), palette.peach, true));
    } else if c > 0 {
        segs.push((format!(" {c}c"), palette.peach, true));
    } else if !view.narrow
        && studio.studio.tasks_total > 0
        && !studio.studio.ready_to_finish
        && studio.studio.state != "finished"
    {
        segs.push((
            format!(
                " {}/{}",
                studio.studio.tasks_done, studio.studio.tasks_total
            ),
            palette.subtext0,
            false,
        ));
    }
    segs.push(("]".into(), elev.muted_surface, false));
    segs
}

fn segs_width(segs: &[(String, Color, bool)]) -> u16 {
    segs.iter().map(|(s, _, _)| display_width(s) as u16).sum()
}

/// Compute `(widths, prefix_end, new_x)` matching `chips::render` for
/// uneven-width strip scroll calculation.
fn strip_widths(
    width: u16,
    studios_n: usize,
    frames: &[Frame<'_>],
    waits: &[Wait<'_>],
    status: &Status<'_>,
    view: StripView<'_>,
    palette: &Palette,
) -> (Vec<u16>, u16, u16) {
    let margin = if width >= 20 { MARGIN } else { 1 };
    let label = format!("STUDIOS {studios_n}");
    let label_w = display_width(&label) as u16;
    let chips_start = margin + label_w + 2;
    let new_x = width.saturating_sub(margin + NEW_CHIP_W);
    let widths = frames
        .iter()
        .filter_map(|f| match f {
            Frame::Studio(studio) => {
                let segs = studio_segs(*studio, waits, status, view, palette);
                Some(segs_width(&segs))
            }
            Frame::New => None,
        })
        .collect();
    (widths, chips_start, new_x)
}

fn fit_from(widths: &[u16], start: usize, chips_start: u16, new_x: u16) -> (usize, bool) {
    if start >= widths.len() || new_x <= chips_start {
        return (0, start < widths.len());
    }
    let room_full = new_x.saturating_sub(chips_start);
    let room_caret = new_x.saturating_sub(chips_start + 2);
    let mut sum = 0u16;
    let mut fits_all = true;
    for (offset, w) in widths[start..].iter().enumerate() {
        let gap = u16::from(offset > 0);
        if sum + gap + *w > room_full {
            fits_all = false;
            break;
        }
        sum += gap + *w;
    }
    if fits_all {
        return (widths.len() - start, false);
    }
    let mut x = 0u16;
    let mut count = 0usize;
    for (offset, w) in widths[start..].iter().enumerate() {
        let gap = u16::from(offset > 0);
        if x + gap + *w > room_caret {
            break;
        }
        x += gap + *w;
        count += 1;
    }
    (count, true)
}

/// Adjust `scroll` so studio chip `target_idx` is completely visible in
/// the 1-row strip with uneven chip widths.
#[allow(clippy::too_many_arguments)]
pub(crate) fn strip_reveal_scroll(
    width: u16,
    frames: &[Frame<'_>],
    waits: &[Wait<'_>],
    status: &Status<'_>,
    active_studio: Option<&StudioKey>,
    local: Option<&str>,
    multi: bool,
    current_scroll: usize,
    target_idx: usize,
    palette: &Palette,
) -> usize {
    let studios_n = frames
        .iter()
        .filter(|f| matches!(f, Frame::Studio(_)))
        .count();
    if studios_n == 0 {
        return 0;
    }
    let target = target_idx.min(studios_n - 1);
    let view = StripView {
        active_studio,
        narrow: width < 60,
        local,
        multi,
    };
    let (widths, chips_start, new_x) =
        strip_widths(width, studios_n, frames, waits, status, view, palette);
    let mut start = current_scroll.min(target);
    loop {
        let (drawn, _) = fit_from(&widths, start, chips_start, new_x);
        if target < start + drawn.max(1) || start >= target {
            return start;
        }
        start += 1;
    }
}

/// Draw the 1-row studio chip strip with pinned `[+ new]` at the right.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render(
    buf: &mut Buffer,
    rect: Rect,
    frames: &[Frame<'_>],
    waits: &[Wait<'_>],
    status: &Status<'_>,
    active_studio: Option<&StudioKey>,
    selected: Option<&Target>,
    scroll: usize,
    focused: bool,
    local: Option<&str>,
    multi: bool,
    suppress_highlights: bool,
    palette: &Palette,
) -> ChipsRender {
    let mut out = ChipsRender::default();
    if rect.height == 0 || rect.width < 12 {
        return out;
    }
    let elev = palette.elevation();
    let base = Style::default().bg(elev.surface);
    let margin = if rect.width >= 20 { MARGIN } else { 1 };
    let studios: Vec<(StudioRef<'_>, Target)> = frames
        .iter()
        .filter_map(|f| match f {
            Frame::Studio(s) => Some((*s, f.target())),
            Frame::New => None,
        })
        .collect();
    let studios_n = studios.len();

    if focused && !suppress_highlights {
        put_text(buf, rect.x, rect.y, 1, "▌", base.fg(palette.accent));
    }
    let label = format!("STUDIOS {studios_n}");
    let label_w = display_width(&label) as u16;
    let label_x = rect.x + margin;
    let label_style = if focused && !suppress_highlights {
        base.fg(palette.accent).add_modifier(Modifier::BOLD)
    } else {
        base.fg(elev.muted_surface)
    };
    put_text(buf, label_x, rect.y, label_w, &label, label_style);
    out.label_rect = Rect::new(rect.x, rect.y, margin + label_w, 1);

    let view = StripView {
        active_studio,
        narrow: rect.width < 60,
        local,
        multi,
    };
    let (widths, chips_rel_start, new_rel_x) =
        strip_widths(rect.width, studios_n, frames, waits, status, view, palette);
    let chips_start = rect.x + chips_rel_start;
    let new_x = rect.x + new_rel_x;

    // Pinned `[+ new]` on the right.
    let new_selected = !suppress_highlights && selected == Some(&Target::NewStudio);
    let new_bg = chip_bg(new_selected, palette);
    if new_x > rect.x + margin + label_w {
        let mut cx = new_x;
        let parts = [
            ("[", elev.muted_surface),
            ("+ new", palette.accent),
            ("]", elev.muted_surface),
        ];
        for (txt, fg) in parts {
            let pw = display_width(txt) as u16;
            let st = Style::default()
                .fg(fg)
                .bg(new_bg)
                .add_modifier(if new_selected {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                });
            put_text(buf, cx, rect.y, pw, txt, st);
            cx += pw;
        }
        out.hits
            .push((Rect::new(new_x, rect.y, NEW_CHIP_W, 1), Target::NewStudio));
    }

    let start = if studios_n == 0 {
        0
    } else {
        scroll.min(studios_n - 1)
    };
    let (count, more_right) = fit_from(&widths, start, chips_rel_start, new_rel_x);
    let mut x = chips_start;
    for (studio, target) in studios.into_iter().skip(start).take(count) {
        let segs = studio_segs(studio, waits, status, view, palette);
        let w = segs_width(&segs);
        let sel = !suppress_highlights && selected == Some(&target);
        let bg = chip_bg(sel, palette);
        let mut cx = x;
        for (text, fg, seg_bold) in &segs {
            let sw = display_width(text) as u16;
            let st = Style::default()
                .fg(*fg)
                .bg(bg)
                .add_modifier(if sel || *seg_bold {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                });
            put_text(buf, cx, rect.y, sw, text, st);
            cx += sw;
        }
        out.hits.push((Rect::new(x, rect.y, w, 1), target));
        x += w + 1;
        out.drawn_studios += 1;
    }
    out.more_left = start > 0;
    out.more_right = more_right;
    if out.more_left && chips_start > rect.x + 1 {
        put_text(
            buf,
            chips_start.saturating_sub(2).max(label_x + label_w),
            rect.y,
            1,
            "‹",
            base.fg(palette.accent),
        );
    }
    if out.more_right && new_x >= rect.x + 2 {
        put_text(buf, new_x - 2, rect.y, 1, "›", base.fg(palette.accent));
    }
    // Keep unused constant referenced only via width check.
    debug_assert_eq!(NEW_CHIP_TEXT.len(), NEW_CHIP_W as usize);
    out
}

fn chip_bg(selected: bool, palette: &Palette) -> Color {
    let elev = palette.elevation();
    if selected {
        elev.raised_row
    } else if elev.is_dark {
        palette.surface0
    } else {
        elev.surface
    }
}
