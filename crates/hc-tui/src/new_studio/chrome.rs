//! Frame, greenlight sheet border (`╔══╡ ✦ NEW STUDIO · [n/4] <STEP> ╞══ esc … ══╗`),
//! step strip (`✓ 1 … ▸ ╡ 2 … ╞`), and footer hints for the New Studio wizard.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::model::Step;
use super::{NewStudio, NsHit, NsHits};
use crate::chrome::palette::Palette;
use crate::chrome::render::{put_right_text, put_text};
use crate::chrome::text::{display_width, fit_hints, truncate_end};

/// Frame content size `(cw, ch)` centered in `area`, or `None` when too small.
pub(super) fn frame_content_size(area: Rect) -> Option<(u16, u16)> {
    let w = area.width.saturating_sub(2).min(96);
    let h = (area.height.saturating_mul(5) / 6)
        .min(area.height.saturating_sub(2))
        .min(30);
    if w < 24 || h < 10 {
        return None;
    }
    Some((w.saturating_sub(2), h.saturating_sub(2)))
}

#[derive(Clone, Copy)]
pub(super) struct ChromeStyle {
    pub(super) shadow: bool,
    pub(super) deco: bool,
}

/// Draw the outer frame, title, step strip, and footer hint row; returns the
/// step content rectangle inside the frame.
pub(super) fn draw_chrome(
    b: &mut Buffer,
    band: Rect,
    d: &NewStudio,
    p: &Palette,
    hints: &mut NsHits,
    hint_tiers: &[&str],
    style: ChromeStyle,
) -> Option<Rect> {
    let (cw, ch) = frame_content_size(band)?;
    let chrome = if style.deco {
        crate::chrome::popup_frame::PopupChrome::Deco
    } else {
        crate::chrome::popup_frame::PopupChrome::Standard
    };
    let geom = crate::chrome::popup_frame::open_centered(
        b,
        band,
        cw,
        ch,
        24,
        10,
        p.accent,
        chrome,
        p,
        style.shadow,
    )?;
    let inner = geom.inner;
    let bg = crate::chrome::popup_frame::overlay_bg(p);
    hints.area = geom.outer;

    let esc = match d.step {
        Step::MachineRepo => "esc cancel",
        _ => "esc step back",
    };

    if style.deco {
        draw_deco_border_header(b, geom.border, d, esc, p);
        draw_deco_step_strip(b, inner, inner.y, d.step, p, hints, bg);
        draw_deco_divider(b, geom.border, inner.y + 1, p, bg);
    } else {
        let title_w = inner.width.saturating_sub(display_width(esc) as u16 + 1);
        let title = format!(" New studio · [{}/4] {} ", d.step.index(), d.step.label());
        put_text(
            b,
            inner.x,
            inner.y,
            title_w,
            &truncate_end(&title, title_w as usize),
            Style::default()
                .fg(p.accent)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        );
        put_right_text(
            b,
            inner,
            inner.y,
            esc,
            Style::default().fg(p.overlay0).bg(bg),
        );
        draw_standard_step_strip(b, inner, inner.y + 1, d.step, p, hints, bg);
    }

    let hint_y = inner.bottom().saturating_sub(1);
    let hint = fit_hints(hint_tiers, usize::from(inner.width));
    put_text(
        b,
        inner.x,
        hint_y,
        inner.width,
        hint,
        Style::default().fg(p.overlay0).bg(bg),
    );
    Some(Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(3),
    ))
}

/// Top border: `╔══╡ ✦ NEW STUDIO · [n/4] <STEP> ╞══ esc … ══╗`.
fn draw_deco_border_header(b: &mut Buffer, border: Rect, d: &NewStudio, esc: &str, p: &Palette) {
    if border.width < 14 {
        return;
    }
    let bg = crate::chrome::popup_frame::overlay_bg(p);
    let elev = p.elevation();
    let border_style = Style::default()
        .fg(p.accent)
        .bg(bg)
        .remove_modifier(Modifier::DIM);
    let star_style = elev
        .gilt_style(bg)
        .add_modifier(Modifier::BOLD)
        .remove_modifier(Modifier::DIM);
    let text_style = Style::default()
        .fg(p.text)
        .bg(bg)
        .add_modifier(Modifier::BOLD)
        .remove_modifier(Modifier::DIM);
    let esc_style = Style::default()
        .fg(p.overlay0)
        .bg(bg)
        .remove_modifier(Modifier::DIM);

    let step_part = format!("[{}/4] {}", d.step.index(), d.step.label());
    let full_prefix = "NEW STUDIO · ";
    let esc_w = display_width(esc) as u16;
    // Overhead without prefix: `╔══╡ ` (5) + `✦ ` (2) + ` ╞` (2) + `═╗` (2) = 11.
    // Right `esc` plaque adds ` ══ ` + esc + ` ══` -> needs esc_w + 7 inside `╞...╗`.
    let need_with_full =
        5 + 2 + display_width(full_prefix) as u16 + display_width(&step_part) as u16 + 2;
    let show_esc = border.width >= need_with_full + esc_w + 6;
    let show_prefix = border.width >= need_with_full + if show_esc { esc_w + 6 } else { 2 };

    let right_reserve = if show_esc { esc_w + 6 } else { 2 };
    let max_title = border.width.saturating_sub(7 + right_reserve);

    let mut x = border.x;
    put_text(b, x, border.y, 5, "╔══╡ ", border_style);
    x += 5;
    put_text(b, x, border.y, 2, "✦ ", star_style);
    x += 2;
    let body_raw = if show_prefix {
        format!("{full_prefix}{step_part}")
    } else {
        step_part
    };
    let body = truncate_end(&body_raw, max_title as usize);
    let body_w = display_width(&body) as u16;
    put_text(b, x, border.y, body_w, &body, text_style);
    x += body_w;
    put_text(b, x, border.y, 2, " ╞", border_style);
    x += 2;

    if show_esc {
        let esc_start = border.right().saturating_sub(4 + esc_w);
        let fill = esc_start.saturating_sub(1).saturating_sub(x);
        if fill > 0 {
            put_text(
                b,
                x,
                border.y,
                fill,
                &"═".repeat(fill as usize),
                border_style,
            );
        }
        put_text(b, esc_start - 1, border.y, 1, " ", Style::default().bg(bg));
        put_text(b, esc_start, border.y, esc_w, esc, esc_style);
        put_text(b, esc_start + esc_w, border.y, 4, " ══╗", border_style);
    } else {
        let fill = border.right().saturating_sub(1).saturating_sub(x);
        if fill > 0 {
            put_text(
                b,
                x,
                border.y,
                fill,
                &"═".repeat(fill as usize),
                border_style,
            );
        }
        put_text(b, border.right() - 1, border.y, 1, "╗", border_style);
    }
}

fn draw_deco_divider(b: &mut Buffer, border: Rect, y: u16, p: &Palette, bg: ratatui::style::Color) {
    if border.width < 3 || y <= border.y || y >= border.bottom().saturating_sub(1) {
        return;
    }
    let elev = p.elevation();
    let edge = elev.gilt_style(bg).remove_modifier(Modifier::DIM);
    let rule = Style::default()
        .fg(elev.border_muted)
        .bg(bg)
        .remove_modifier(Modifier::DIM);
    put_text(b, border.x, y, 1, "╟", edge);
    let inner_w = border.width - 2;
    put_text(
        b,
        border.x + 1,
        y,
        inner_w,
        &"─".repeat(inner_w as usize),
        rule,
    );
    put_text(b, border.right() - 1, y, 1, "╢", edge);
}

fn draw_deco_step_strip(
    b: &mut Buffer,
    inner: Rect,
    y: u16,
    current: Step,
    p: &Palette,
    hints: &mut NsHits,
    bg: ratatui::style::Color,
) {
    let steps = [
        Step::MachineRepo,
        Step::IdTitle,
        Step::Cast,
        Step::CheckStart,
    ];
    let item_width = |step: Step| -> u16 {
        let idx = step.index();
        if step == current {
            display_width(&format!("╡ {idx} {} ╞", step.label()))
        } else if idx < current.index() {
            display_width(&format!("✓ {idx} {}", step.label()))
        } else {
            display_width(&format!("{idx} {}", step.label()))
        }
    };
    let items_sum: u16 = steps.iter().map(|&s| item_width(s)).sum();
    let sep = if items_sum + 9 <= inner.width {
        " ▸ "
    } else {
        let compact = format!(" {}/4 {} ", current.index(), current.label());
        put_text(
            b,
            inner.x,
            y,
            inner.width,
            &truncate_end(&compact, inner.width as usize),
            Style::default()
                .fg(crate::chrome::dialogs::contrast(p))
                .bg(p.accent)
                .add_modifier(Modifier::BOLD),
        );
        return;
    };

    let elev = p.elevation();
    let sep_style = elev.gilt_style(bg);
    let sep_w = display_width(sep) as u16;
    let active_style = Style::default()
        .fg(crate::chrome::dialogs::contrast(p))
        .bg(p.accent)
        .add_modifier(Modifier::BOLD);
    let done_style = Style::default().fg(p.subtext0).bg(bg);
    let check_style = Style::default()
        .fg(p.teal)
        .bg(bg)
        .add_modifier(Modifier::BOLD);
    let future_style = Style::default().fg(p.overlay0).bg(bg);

    let mut x = inner.x;
    for (idx, step) in steps.into_iter().enumerate() {
        if idx > 0 {
            put_text(
                b,
                x,
                y,
                sep_w.min(inner.right().saturating_sub(x)),
                sep,
                sep_style,
            );
            x += sep_w;
        }
        let n = step.index();
        let w = item_width(step);
        let avail = w.min(inner.right().saturating_sub(x));
        if step == current {
            let text = format!("╡ {n} {} ╞", step.label());
            put_text(b, x, y, avail, &text, active_style);
        } else if n < current.index() {
            put_text(b, x, y, avail.min(1), "✓", check_style);
            if avail > 1 {
                let tail = format!(" {n} {}", step.label());
                put_text(b, x + 1, y, avail - 1, &tail, done_style);
            }
            hints.push(Rect::new(x, y, avail, 1), NsHit::StepTab(step));
        } else {
            let text = format!("{n} {}", step.label());
            put_text(b, x, y, avail, &text, future_style);
        }
        x += w;
    }
}

fn draw_standard_step_strip(
    b: &mut Buffer,
    inner: Rect,
    y: u16,
    current: Step,
    p: &Palette,
    hints: &mut NsHits,
    bg: ratatui::style::Color,
) {
    let steps = [
        Step::MachineRepo,
        Step::IdTitle,
        Step::Cast,
        Step::CheckStart,
    ];
    let tab_w: u16 = steps
        .iter()
        .map(|step| display_width(&format!(" {} {} ", step.index(), step.label())) as u16 + 1)
        .sum();
    if tab_w <= inner.width + 1 {
        let mut x = inner.x;
        for step in steps {
            let label = format!(" {} {} ", step.index(), step.label());
            let w = display_width(&label) as u16;
            let style = if step == current {
                Style::default()
                    .fg(crate::chrome::dialogs::contrast(p))
                    .bg(p.accent)
                    .add_modifier(Modifier::BOLD)
            } else if step.index() < current.index() {
                Style::default().fg(p.subtext0).bg(bg)
            } else {
                Style::default().fg(p.overlay0).bg(bg)
            };
            put_text(
                b,
                x,
                y,
                w.min(inner.right().saturating_sub(x)),
                &label,
                style,
            );
            if step.index() < current.index() {
                hints.push(Rect::new(x, y, w, 1), NsHit::StepTab(step));
            }
            x += w + 1;
        }
    } else {
        let compact = format!(" {}/4 {} ", current.index(), current.label());
        put_text(
            b,
            inner.x,
            y,
            inner.width,
            &truncate_end(&compact, inner.width as usize),
            Style::default()
                .fg(crate::chrome::dialogs::contrast(p))
                .bg(p.accent)
                .add_modifier(Modifier::BOLD),
        );
    }
}
