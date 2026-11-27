//! The band's frames: one film frame per studio and the trailing
//! new-studio frame. Pure drawing — it reads the document through
//! `Frame` and writes cells plus the hit rects the mouse needs.
//!
//! Widths follow the design: four frames across when they fit,
//! `(width - 4 - 3·gap) / 4` clamped to 24..36; two per row on a
//! narrow screen, one under 40 columns. Frames that do not fit scroll
//! sideways, and a `‹` or `›` at the band edge says so.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::frames_draw::draw_studio;
use super::new_frame::draw_new;
use super::rows::{self, Frame};
use super::sheet::Status;
use super::{Hollywood, LayerOpts, MARGIN, Target, chips, plan};
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::panel::data::{MachineSummary, spans_machines, studio_label};

/// Full studio frame height, borders included.
pub(crate) const FRAME_H: u16 = 7;
/// Medium studio frame height (drops role badge and next step).
pub(crate) const FRAME_H_MID: u16 = 5;
/// Minimal studio frame height (border + state/round/tasks + border).
pub(crate) const FRAME_H_MIN: u16 = 3;
/// The air between two frames.
pub(crate) const GAP: u16 = 2;
/// Frame width bounds.
const FRAME_W_MIN: u16 = 24;
const FRAME_W_MAX: u16 = 36;

/// Where the frames are drawn from.
#[derive(Clone, Copy)]
pub(crate) struct FramesView<'a> {
    pub selected: Option<&'a Target>,
    /// Studio currently focused in the main pane (draws `▸` when not selected).
    pub active_studio: Option<&'a crate::studio::StudioKey>,
    /// Index of the first frame drawn.
    pub scroll: usize,
    /// Height per frame (`FRAME_H`, `FRAME_H_MID`, or `FRAME_H_MIN`).
    pub frame_h: u16,
    pub mobile: bool,
    /// This core's name — the local studios' machine label.
    pub local: Option<&'a str>,
    /// Studios span machines: the titles carry `· <machine>`.
    pub multi: bool,
    /// Each frame's own machine line and stage — `machine` keyed.
    pub status: &'a Status<'a>,
    /// Suppress selection border/background while a modal covers it.
    pub suppress_highlights: bool,
}

/// What the frames drew.
#[derive(Default)]
pub(crate) struct FramesRender {
    /// Each drawn frame's rect and identity.
    pub hits: Vec<(Rect, Target)>,
    /// Frames sit before the window.
    pub more_left: bool,
    /// Frames sit after it.
    pub more_right: bool,
    /// Screen rows the frames took.
    pub height: u16,
    /// The number of frames drawn.
    pub drawn: usize,
}

/// How many frames fit a row, and how wide each is.
pub(crate) fn layout(width: u16, mobile: bool) -> (usize, u16) {
    let avail = width.saturating_sub(MARGIN * 2).max(1);
    let mut per_row = if mobile {
        if width < 40 { 1 } else { 2 }
    } else {
        4
    };
    while per_row > 1 {
        let gaps = GAP * (per_row as u16 - 1);
        let fw = avail.saturating_sub(gaps) / per_row as u16;
        if fw >= FRAME_W_MIN {
            return (per_row, fw.min(FRAME_W_MAX));
        }
        per_row -= 1;
    }
    (1, avail.min(FRAME_W_MAX))
}

/// Draw the frames that fit in `area`, starting at the window's first
/// frame.
pub(crate) fn render(
    buf: &mut Buffer,
    area: Rect,
    frames: &[Frame<'_>],
    view: FramesView<'_>,
    palette: &Palette,
) -> FramesRender {
    let frame_h = view.frame_h.max(FRAME_H_MIN);
    let (per_row, fw) = layout(area.width, view.mobile);
    let mut out = FramesRender::default();
    let rows_fit = u16::from(area.height >= frame_h);
    if rows_fit == 0 || fw == 0 {
        return out;
    }
    let start = view.scroll.min(frames.len().saturating_sub(1));
    let mut index = start;
    'rows: for row in 0..rows_fit {
        let y = area.y + row * frame_h;
        for col in 0..per_row {
            let x = area.x + MARGIN + (col as u16) * (fw + GAP);
            if index >= frames.len() || x + fw > area.x + area.width {
                break 'rows;
            }
            let rect = Rect::new(x, y, fw, frame_h);
            let frame = &frames[index];
            let selected = !view.suppress_highlights && view.selected == Some(&frame.target());
            match frame {
                Frame::Studio(studio) => {
                    let label =
                        studio_label(&studio.studio.id, studio.machine, view.local, view.multi);
                    let is_active = view.active_studio == Some(&studio.key());
                    draw_studio(
                        buf,
                        rect,
                        *studio,
                        &label,
                        selected,
                        is_active,
                        view.status,
                        palette,
                    )
                }
                Frame::New => draw_new(buf, rect, selected, palette),
            }
            out.hits.push((rect, frame.target()));
            out.drawn += 1;
            index += 1;
        }
    }
    out.height = rows_fit * frame_h;
    let elev = palette.elevation();
    let rail = elev.gilt_style(elev.surface);
    for dy in 0..out.height {
        let ry = area.y + dy;
        if !view.suppress_highlights {
            put_text(
                buf,
                area.x,
                ry,
                1,
                "▌",
                Style::default().fg(palette.accent).bg(elev.surface),
            );
        } else {
            put_text(buf, area.x, ry, 1, "║", rail);
        }
        if area.width >= 2 {
            put_text(buf, area.x + area.width - 1, ry, 1, "║", rail);
        }
    }
    out.more_left = start > 0;
    out.more_right = start + out.drawn < frames.len();
    let caret = Style::default().fg(palette.accent).bg(elev.surface);
    if out.more_left && area.width >= 4 {
        put_text(buf, area.x + 1, area.y, 1, "‹", caret);
    }
    if out.more_right && area.width >= 4 {
        put_text(buf, area.x + area.width - 2, area.y, 1, "›", caret);
    }
    out
}

/// Compute the updated `frame_scroll` keeping `hollywood.selected`
/// visible in either the 1-row uneven-width chip strip (`frames_h == 1`)
/// or the multi-row/frame grid (`frames_h >= 3`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn reveal_frame_scroll(
    frame: Rect,
    machines: &[MachineSummary],
    status: &Status<'_>,
    hollywood: &Hollywood,
    active_studio: Option<&crate::studio::StudioKey>,
    local: Option<&str>,
    opts: LayerOpts,
    palette: &Palette,
) -> usize {
    let all_frames = rows::frames(machines);
    let Some(sel) = hollywood.selected.as_ref() else {
        return hollywood.frame_scroll;
    };
    let Some(frame_idx) = all_frames.iter().position(|f| &f.target() == sel) else {
        return hollywood.frame_scroll;
    };
    let (waits, band_rows) =
        rows::band_rows_with_echoes(machines, status, opts.agent, &hollywood.reply_echoes);
    let p = plan::plan(frame, &waits, &band_rows, hollywood, opts);
    let studios_n = all_frames
        .iter()
        .filter(|f| matches!(f, Frame::Studio(_)))
        .count();
    if p.frames_h <= 1 {
        if matches!(sel, Target::NewStudio) {
            return hollywood.frame_scroll.min(studios_n.saturating_sub(1));
        }
        return chips::strip_reveal_scroll(
            frame.width,
            &all_frames,
            &waits,
            status,
            active_studio,
            local,
            spans_machines(machines),
            hollywood.frame_scroll,
            frame_idx,
            palette,
        );
    }
    let (per_row, _) = layout(frame.width, opts.mobile);
    let per_row = per_row.max(1);
    let row = frame_idx / per_row;
    let start_row = hollywood.frame_scroll / per_row;
    if row != start_row {
        row * per_row
    } else {
        hollywood.frame_scroll
    }
}
