//! The one-row studio chip strip the frames fold into when height
//! runs out: `  studios  [1 acme-shop·Local ● !1]  [+ new n]` —
//! each chip its frame's identity, digit-jump key and health mark.
//! The selected chip paints `selection_bg`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::Target;
use super::rows::Frame;
use super::sheet::{Conn, Status};
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::display_width;
use crate::panel::data::studio_label;
use crate::studio::glyphs::Health;

/// What the strip drew: the chip rects and whether it scrolled.
#[derive(Default)]
pub(crate) struct ChipsRender {
    pub hits: Vec<(Rect, Target)>,
    pub more_left: bool,
    pub more_right: bool,
}

/// Draw the strip into `rect` (one row). `scroll` is the first
/// frame index drawn.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render(
    buf: &mut Buffer,
    rect: Rect,
    frames: &[Frame<'_>],
    status: &Status<'_>,
    selected: Option<&Target>,
    scroll: usize,
    local: Option<&str>,
    multi: bool,
    palette: &Palette,
) -> ChipsRender {
    let mut out = ChipsRender::default();
    if rect.height == 0 || rect.width < 8 {
        return out;
    }
    let base = Style::default().bg(palette.surface_dim);
    let mut x = rect.x + 1;
    put_text(buf, x, rect.y, 9, " studios ", base.fg(palette.overlay0));
    x += 10;
    let start = scroll.min(frames.len().saturating_sub(1));
    let mut drawn = 0usize;
    for (index, frame) in frames.iter().enumerate().skip(start) {
        // The chip's segments: `[`, digit, ` `, label, ` `, mark,
        // ` !N?` , `]` — each measured so a chip that does not fit
        // whole sets `more_right` and stops the row.
        let target = frame.target();
        let sel = selected == Some(&target);
        let bg = if sel {
            palette.selection_bg
        } else {
            palette.surface0
        };
        let digit = if index < 9 {
            format!("{}", index + 1)
        } else {
            "·".to_string()
        };
        let segs: Vec<(String, Color)> = match frame {
            Frame::Studio(studio) => {
                let label = studio_label(&studio.studio.id, studio.machine, local, multi);
                let (conn, stage) = status(studio.machine);
                let health = match (conn, stage) {
                    (Conn::Offline, _) => Health::Offline,
                    (_, false) => Health::StageDown,
                    _ => Health::Live,
                };
                let mut segs = vec![
                    ("[".into(), palette.overlay0),
                    (digit, palette.accent),
                    (" ".into(), palette.text),
                    (label.fit(18), palette.text),
                    (" ".into(), palette.text),
                    (health.glyph().to_string(), health.color(palette)),
                ];
                if studio.studio.waiting > 0 {
                    let user = studio.studio.roles.iter().any(|r| r.attention_user);
                    segs.push((
                        format!(" !{}", studio.studio.waiting),
                        if user { palette.red } else { palette.peach },
                    ));
                }
                segs.push(("]".into(), palette.overlay0));
                segs
            }
            Frame::New => vec![
                ("[".into(), palette.overlay0),
                ("+ new n".into(), palette.accent),
                ("]".into(), palette.overlay0),
            ],
        };
        let w: u16 = segs
            .iter()
            .map(|(s, _)| display_width(s) as u16)
            .sum::<u16>()
            + 1;
        if x + w > rect.x + rect.width {
            out.more_right = true;
            break;
        }
        let mut cx = x + 1;
        for (text, colour) in &segs {
            let w = display_width(text) as u16;
            let style = Style::default().fg(*colour).bg(bg).add_modifier(if sel {
                Modifier::BOLD
            } else {
                Modifier::empty()
            });
            put_text(buf, cx, rect.y, w, text, style);
            cx += w;
        }
        out.hits.push((Rect::new(x, rect.y, w, 1), target));
        x += w + 1;
        drawn += 1;
    }
    out.more_left = start > 0;
    out.more_right = out.more_right || start + drawn < frames.len();
    if out.more_left {
        put_text(buf, rect.x, rect.y, 1, "‹", base.fg(palette.accent));
    }
    if out.more_right && rect.width > 1 {
        put_text(
            buf,
            rect.x + rect.width - 1,
            rect.y,
            1,
            "›",
            base.fg(palette.accent),
        );
    }
    out
}
