//! The band's frames: one film frame per studio and the trailing
//! new-studio frame. Pure drawing — it reads the document through
//! `Frame` and writes cells plus the hit rects the mouse needs.
//!
//! Widths follow the design: four frames across when they fit,
//! `(width - 4 - 3·gap) / 4` clamped to 24..36; two per row on a
//! narrow screen, one under 40 columns. Frames that do not fit scroll
//! sideways, and a `‹` or `›` at the band edge says so.

use backlot_model::summary::StudioSummary;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::Target;
use super::new_frame::draw_new;
use super::rows::{Frame, on_set};
use super::sheet::{Conn, Status};
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::truncate_end;
use crate::panel::data::{StudioLabel, StudioRef, studio_label};

/// A frame's height, borders included.
pub(crate) const FRAME_H: u16 = 7;
/// The air between two frames.
pub(crate) const GAP: u16 = 2;
/// The band's left and right margins.
pub(crate) const MARGIN: u16 = 2;
/// Frame width bounds.
const FRAME_W_MIN: u16 = 24;
const FRAME_W_MAX: u16 = 36;

/// Where the frames are drawn from.
#[derive(Clone, Copy)]
pub(crate) struct FramesView<'a> {
    pub selected: Option<&'a Target>,
    /// Index of the first frame drawn.
    pub scroll: usize,
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

/// A style on a surface of its own — every cell the band writes names
/// its background, or the panes would show through it.
fn on(fg: Color, bg: Color) -> Style {
    Style::default().fg(fg).bg(bg)
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
    let (per_row, fw) = layout(area.width, view.mobile);
    let mut out = FramesRender::default();
    let rows_fit = if view.mobile {
        area.height / FRAME_H
    } else {
        u16::from(area.height >= FRAME_H)
    };
    if rows_fit == 0 || fw == 0 {
        return out;
    }
    let start = view.scroll.min(frames.len().saturating_sub(1));
    let mut index = start;
    'rows: for row in 0..rows_fit {
        let y = area.y + row * FRAME_H;
        for col in 0..per_row {
            let x = area.x + MARGIN + (col as u16) * (fw + GAP);
            if index >= frames.len() || x + fw > area.x + area.width {
                break 'rows;
            }
            let rect = Rect::new(x, y, fw, FRAME_H);
            let frame = &frames[index];
            let selected = !view.suppress_highlights && view.selected == Some(&frame.target());
            match frame {
                Frame::Studio(studio) => {
                    let label =
                        studio_label(&studio.studio.id, studio.machine, view.local, view.multi);
                    draw_studio(buf, rect, *studio, &label, selected, view.status, palette)
                }
                Frame::New => draw_new(buf, rect, selected, palette),
            }
            out.hits.push((rect, frame.target()));
            out.drawn += 1;
            index += 1;
        }
    }
    out.height = rows_fit * FRAME_H;
    out.more_left = start > 0;
    out.more_right = start + out.drawn < frames.len();
    // `‹` / `›` in the margins, on the frames' top row: the band edge,
    // in accent.
    let caret = Style::default().fg(palette.accent).bg(palette.surface_dim);
    if out.more_left && area.width > 0 {
        put_text(buf, area.x, area.y, 1, "‹", caret);
    }
    if out.more_right && area.width > 1 {
        put_text(buf, area.x + area.width - 1, area.y, 1, "›", caret);
    }
    out
}

/// A studio's frame: `╭─ <id[ · machine]> ─…╮`, its state line —
/// its own machine's line and stage — the role on set, what it is
/// doing, and the cast line. The label's machine part is what
/// narrows first.
fn draw_studio(
    buf: &mut Buffer,
    rect: Rect,
    studio: StudioRef<'_>,
    label: &StudioLabel,
    selected: bool,
    status: &Status<'_>,
    palette: &Palette,
) {
    if rect.width < 4 || rect.height < 3 {
        return;
    }
    let border = if selected {
        palette.accent
    } else {
        palette.surface1
    };
    let bg = if selected {
        palette.selection_bg
    } else {
        palette.panel_bg
    };
    // The inside first, so nothing behind the frame shows through.
    for y in rect.y + 1..rect.y + rect.height - 1 {
        for x in rect.x + 1..rect.x + rect.width - 1 {
            buf[(x, y)].set_symbol(" ");
            buf[(x, y)].set_style(on(border, bg));
        }
    }
    // `╭─ <label> ─…╮` — the label in the top edge, bold, accent when
    // the frame is selected; a machine name that does not fit is cut
    // before the studio id is.
    let edge = on(border, palette.surface_dim);
    put_text(buf, rect.x, rect.y, 2, "╭─", edge);
    let id = label.fit(rect.width.saturating_sub(6) as usize);
    let title = format!(" {id} ");
    let title_style = on(
        if selected {
            palette.accent
        } else {
            palette.text
        },
        palette.surface_dim,
    )
    .add_modifier(Modifier::BOLD);
    let used = 2 + title.chars().count() as u16;
    put_text(
        buf,
        rect.x + 2,
        rect.y,
        title.chars().count() as u16,
        &title,
        title_style,
    );
    if used + 1 < rect.width {
        let fill = rect.width - 1 - used;
        put_text(
            buf,
            rect.x + used,
            rect.y,
            fill,
            &"─".repeat(fill as usize),
            edge,
        );
    }
    put_text(buf, rect.x + rect.width - 1, rect.y, 1, "╮", edge);
    // The sides and the bottom.
    for y in rect.y + 1..rect.y + rect.height - 1 {
        put_text(buf, rect.x, y, 1, "│", on(border, palette.surface_dim));
        put_text(
            buf,
            rect.x + rect.width - 1,
            y,
            1,
            "│",
            on(border, palette.surface_dim),
        );
    }
    let bottom = rect.y + rect.height - 1;
    put_text(buf, rect.x, bottom, 1, "╰", edge);
    put_text(
        buf,
        rect.x + 1,
        bottom,
        rect.width - 2,
        &"─".repeat((rect.width - 2) as usize),
        edge,
    );
    put_text(buf, rect.x + rect.width - 1, bottom, 1, "╯", edge);

    let inner = rect.width.saturating_sub(4);
    // Row 1: the state line — this frame's own machine line and
    // stage — and right-aligned `r<ver> · <done>/<total>` plus the
    // waiting chip (red when a prompt is the user's).
    let (conn, stage) = status(studio.machine);
    let room = inner.saturating_sub(2) as usize;
    let (mark, word, colour) = studio_line(studio.studio, conn, stage, room, palette);
    let y = rect.y + 1;
    put_text(buf, rect.x + 2, y, 1, mark, on(colour, bg));
    put_text(
        buf,
        rect.x + 3,
        y,
        inner.saturating_sub(1),
        &format!(" {}", truncate_end(word, inner.saturating_sub(2) as usize)),
        on(colour, bg).add_modifier(Modifier::BOLD),
    );
    let rev = format!(
        "r{} · {}/{}",
        studio.studio.requirement_version, studio.studio.tasks_done, studio.studio.tasks_total
    );
    let mut right_w = rev.chars().count() as u16;
    let chip = if studio.studio.waiting > 0 {
        format!(" !{} ", studio.studio.waiting)
    } else {
        String::new()
    };
    // One unchipped space between the counts and the badge.
    right_w += chip.chars().count() as u16 + u16::from(!chip.is_empty());
    if inner > right_w + 8 {
        let mut rx = rect.x + rect.width - 2 - right_w;
        put_text(buf, rx, y, rev.chars().count() as u16, &rev, {
            on(palette.subtext0, bg)
        });
        rx += rev.chars().count() as u16;
        if !chip.is_empty() {
            rx += 1;
            let user = studio.studio.roles.iter().any(|r| r.attention_user);
            put_text(
                buf,
                rx,
                y,
                chip.chars().count() as u16,
                &chip,
                on(
                    palette.panel_bg,
                    if user { palette.red } else { palette.peach },
                )
                .add_modifier(Modifier::BOLD),
            );
        }
    }
    // Row 2: the role on set now.
    let who = on_set(studio.studio)
        .map(|r| r.name.as_str())
        .unwrap_or("—");
    put_text(
        buf,
        rect.x + 2,
        rect.y + 2,
        inner,
        &format!(" {}", truncate_end(who, inner.saturating_sub(1) as usize)),
        on(palette.overlay1, bg),
    );
    // Row 3: what it is doing — or the ledger's error, in red.
    let (what, colour) = match studio.studio.error.as_deref() {
        Some(error) => (error, palette.red),
        None => (
            on_set(studio.studio)
                .and_then(|r| {
                    r.doing
                        .as_deref()
                        .or_else(|| r.running.as_ref().map(|run| run.cmd.as_str()))
                        .or_else(|| r.progress.as_ref().map(|p| p.text.as_str()))
                })
                .or(studio.studio.next.as_deref())
                .unwrap_or("nothing on set"),
            palette.subtext0,
        ),
    };
    put_text(
        buf,
        rect.x + 2,
        rect.y + 3,
        inner,
        &format!(" {}", truncate_end(what, inner.saturating_sub(1) as usize)),
        on(colour, bg),
    );
    // Row 4: the studio's own next step — the one thing to do next
    // when its state names it. Everything else stays blank.
    if let Some(step) = crate::panel::data::next_step(studio.studio) {
        let text = format!("next: {step}");
        put_text(
            buf,
            rect.x + 2,
            rect.y + 4,
            inner,
            &format!(" {}", truncate_end(&text, inner.saturating_sub(1) as usize)),
            on(palette.overlay1, bg),
        );
    }
    // Row 5: the cast, one glyph each; `◈ <open> rev` in blue when
    // reviews wait, else `<N> cast`.
    let cast_label = if studio.studio.open_reviews > 0 {
        format!("◈ {} rev", studio.studio.open_reviews)
    } else {
        format!("{} cast", studio.studio.roles.len())
    };
    let label_w = cast_label.chars().count() as u16;
    let label_x = (rect.x + rect.width - 2).saturating_sub(label_w);
    if label_x > rect.x + 2 {
        put_text(
            buf,
            label_x,
            rect.y + 5,
            label_w,
            &cast_label,
            on(
                if studio.studio.open_reviews > 0 {
                    palette.blue
                } else {
                    palette.overlay0
                },
                bg,
            ),
        );
    }
    let mut x = rect.x + 2;
    for role in &studio.studio.roles {
        if x + 1 >= label_x {
            break;
        }
        let mark = crate::studio::glyphs::Person::of(role);
        put_text(
            buf,
            x,
            rect.y + 5,
            1,
            mark.still(),
            on(mark.color(palette), bg),
        );
        x += 2;
    }
}

/// The frame's state line, machine-aware (A8.2): `× error`, then
/// `× offline`, `◌ stage stopped`, `✓ ready`, `✓ done`, `● live` —
/// `(mark, word, colour)`. `room` is the width the word gets; the
/// stopped-stage line names its `s start` key only when it fits.
pub(crate) fn studio_line<'a>(
    studio: &'a StudioSummary,
    conn: Conn,
    stage: bool,
    room: usize,
    palette: &Palette,
) -> (&'static str, &'a str, Color) {
    if studio.error.is_some() {
        return ("×", "error", palette.red);
    }
    if conn == Conn::Offline {
        return ("×", "offline", palette.red);
    }
    if !stage {
        let word = if room >= "stage stopped · s start".len() {
            "stage stopped · s start"
        } else {
            "stage stopped"
        };
        return ("◌", word, palette.peach);
    }
    if studio.ready_to_finish {
        return ("✓", "ready", palette.teal);
    }
    match studio.state.as_str() {
        "finished" => ("✓", "done", palette.overlay1),
        "live" => ("●", "live", palette.green),
        other => (
            "·",
            if other.is_empty() { "down" } else { other },
            palette.overlay0,
        ),
    }
}
