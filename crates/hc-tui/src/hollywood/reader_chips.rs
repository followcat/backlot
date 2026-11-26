//! Reader action strip chips (`[t]`, `[o]`, `[p]`, `[v]`, `[y]`,
//! `[a]`, `[x]`), pending lock strip, and 3-tier adaptive fitting
//! recording exact mouse hit-rects.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::reader_meta::{ReaderKind, ReaderOAction};
use super::view::CardView;
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::{display_width, truncate_end};

#[derive(Clone, Copy)]
struct ChipSpec {
    key: char,
    full: &'static str,
    short: &'static str,
    primary: bool,
}

fn chip(key: char, full: &'static str, short: &'static str, primary: bool) -> ChipSpec {
    ChipSpec {
        key,
        full,
        short,
        primary,
    }
}

fn available_chips(view: &CardView) -> Vec<ChipSpec> {
    if view.meta.kind == ReaderKind::Design {
        let mut chips = Vec::with_capacity(4);
        if !view.meta.design_finalized() {
            chips.push(chip('r', "[r] reply", "[r]", true));
        }
        if view.meta.design_awaiting_user() {
            chips.push(chip('a', "[a] sign off", "[a]", true));
        }
        if view.meta.sheet_studio_key().is_some() {
            chips.push(chip('v', "[v] sheet", "[v]", false));
        }
        chips.push(chip('y', "[y] copy", "[y]", true));
        return chips;
    }

    if view.card.is_some() {
        let mut chips = vec![
            chip('a', "[a] apply", "[a]", true),
            chip('x', "[x] cancel", "[x]", true),
        ];
        if view.meta.sheet_studio_key().is_some() {
            chips.push(chip('v', "[v] sheet", "[v]", false));
        }
        chips.push(chip('y', "[y] copy", "[y]", true));
        return chips;
    }

    let mut chips = Vec::with_capacity(5);
    if view.meta.can_tell() {
        chips.push(chip('t', "[t] tell", "[t]", true));
    }
    if let Some(o_act) = view.meta.o_action() {
        let full = match o_act {
            ReaderOAction::Focus => "[o] focus",
            ReaderOAction::Reopen => "[o] reopen",
        };
        chips.push(chip('o', full, "[o]", true));
    }
    if view.meta.can_peek() {
        chips.push(chip('p', "[p] peek", "[p]", false));
    }
    if view.meta.sheet_studio_key().is_some() {
        chips.push(chip('v', "[v] sheet", "[v]", false));
    }
    chips.push(chip('y', "[y] copy", "[y]", true));
    chips
}

pub(super) fn draw_action_strip(
    buf: &mut Buffer,
    row: Rect,
    view: &CardView,
    nav_total: usize,
    pending: Option<&str>,
    palette: &Palette,
    chips_out: &mut Vec<(Rect, char)>,
) {
    let area = row;
    let y = row.y;
    if area.width <= 2 {
        return;
    }
    let strip = Rect::new(area.x + 1, y, area.width - 2, 1);
    let bg = Style::default().bg(palette.surface0);
    buf.set_style(strip, bg);
    let avail = usize::from(area.width.saturating_sub(4));
    if avail == 0 {
        return;
    }

    let base_x = area.x + 2;
    let hint_fg = bg.fg(palette.subtext0);
    let key_fg = bg.fg(palette.text);

    if view.card.is_some()
        && let Some(lbl) = pending
    {
        let right = "  [y] copy · esc";
        let right_copy = "[y] copy";
        let copy_w = display_width(right_copy) as u16;
        let lock_text = format!("■ {lbl}… (a/x locked)");
        let right_w = display_width(right);
        if avail >= right_w + 6 {
            let lock_fitted = truncate_end(&lock_text, avail - right_w);
            let lw = display_width(&lock_fitted) as u16;
            put_text(buf, base_x, y, lw, &lock_fitted, bg.fg(palette.yellow));
            let copy_x = base_x + lw + 2;
            put_text(buf, copy_x, y, copy_w, right_copy, key_fg);
            chips_out.push((Rect::new(copy_x, y, copy_w, 1), 'y'));
            put_text(
                buf,
                copy_x + copy_w,
                y,
                display_width(" · esc") as u16,
                " · esc",
                hint_fg,
            );
        } else {
            let fitted = truncate_end(&lock_text, avail);
            put_text(
                buf,
                base_x,
                y,
                display_width(&fitted) as u16,
                &fitted,
                bg.fg(palette.yellow),
            );
        }
        return;
    }

    let specs = available_chips(view);
    let has_nav = nav_total > 1;
    let tail = if view.meta.kind == ReaderKind::Design {
        if has_nav {
            "[/] · esc back"
        } else {
            "esc back"
        }
    } else if has_nav {
        "←/→ · esc"
    } else {
        "esc"
    };

    let tier0: Vec<ChipSpec> = specs.clone();
    let tier1: Vec<ChipSpec> = specs.iter().copied().filter(|c| c.primary).collect();
    let tier2: Vec<ChipSpec> = if view.meta.kind == ReaderKind::Records {
        specs.iter().copied().filter(|c| c.key == 'y').collect()
    } else {
        tier1.clone()
    };

    let measure_tier = |list: &[ChipSpec], short: bool, gap: usize, tail_gap: usize, t: &str| {
        if list.is_empty() {
            return display_width(t);
        }
        let chips_w: usize = list
            .iter()
            .map(|c| display_width(if short { c.short } else { c.full }))
            .sum::<usize>()
            + gap * list.len().saturating_sub(1);
        chips_w + tail_gap + display_width(t)
    };

    let (chosen, short, gap, tail_gap, suffix) = if measure_tier(&tier0, false, 2, 4, tail) <= avail
    {
        (tier0, false, 2usize, 4usize, tail)
    } else if measure_tier(&tier1, false, 1, 2, tail) <= avail {
        (tier1, false, 1usize, 2usize, tail)
    } else {
        (tier2, true, 1usize, 2usize, "? keys")
    };

    let mut cx = base_x;
    let right_limit = base_x + avail as u16;
    for (idx, chip) in chosen.iter().enumerate() {
        if idx > 0 {
            cx = cx.saturating_add(gap as u16);
        }
        let label = if short { chip.short } else { chip.full };
        let w = display_width(label) as u16;
        if cx + w > right_limit {
            let rem = usize::from(right_limit.saturating_sub(cx));
            let clipped = truncate_end(label, rem);
            let cw = display_width(&clipped) as u16;
            if cw > 0 {
                put_text(buf, cx, y, cw, &clipped, key_fg);
                chips_out.push((Rect::new(cx, y, cw, 1), chip.key));
            }
            return;
        }
        put_text(buf, cx, y, w, label, key_fg);
        chips_out.push((Rect::new(cx, y, w, 1), chip.key));
        cx = cx.saturating_add(w);
    }

    cx = cx.saturating_add(tail_gap as u16);
    if cx < right_limit {
        let rem = usize::from(right_limit.saturating_sub(cx));
        let fitted = truncate_end(suffix, rem);
        let sw = display_width(&fitted) as u16;
        if sw > 0 {
            put_text(buf, cx, y, sw, &fitted, hint_fg);
        }
    }
}
