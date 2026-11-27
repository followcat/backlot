//! Reader action strip chips (`[t]`, `[o]`, `[p]`, `[v]`, `[y]`,
//! `[a]`, `[x]`), pending lock strip, and 3-tier adaptive fitting
//! recording exact mouse hit-rects.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::reader_chrome::babylon_rail_style;
use super::reader_meta::{ReaderKind, ReaderOAction};
use super::view::CardView;
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::{display_width, truncate_end};

#[derive(Clone, Copy)]
struct ChipSpec {
    key: char,
    label: &'static str,
    primary: bool,
}

fn chip(key: char, label: &'static str, primary: bool) -> ChipSpec {
    ChipSpec {
        key,
        label,
        primary,
    }
}

fn available_chips(view: &CardView) -> Vec<ChipSpec> {
    if view.meta.kind == ReaderKind::Design {
        let mut chips = Vec::with_capacity(4);
        if !view.meta.design_finalized() {
            chips.push(chip('r', "reply", true));
        }
        if view.meta.design_awaiting_user() {
            chips.push(chip('a', "sign off", true));
        }
        if view.meta.sheet_studio_key().is_some() {
            chips.push(chip('v', "sheet", false));
        }
        chips.push(chip('y', "copy", true));
        return chips;
    }

    if view.card.is_some() {
        let mut chips = vec![chip('a', "apply", true), chip('x', "cancel", true)];
        if view.meta.sheet_studio_key().is_some() {
            chips.push(chip('v', "sheet", false));
        }
        chips.push(chip('y', "copy", true));
        return chips;
    }

    let mut chips = Vec::with_capacity(5);
    if view.meta.can_tell() {
        chips.push(chip('t', "tell", true));
    }
    if let Some(o_act) = view.meta.o_action() {
        let label = match o_act {
            ReaderOAction::Focus => "focus",
            ReaderOAction::Reopen => "reopen",
        };
        chips.push(chip('o', label, true));
    }
    if view.meta.can_peek() {
        chips.push(chip('p', "peek", false));
    }
    if view.meta.sheet_studio_key().is_some() {
        chips.push(chip('v', "sheet", false));
    }
    chips.push(chip('y', "copy", true));
    chips
}

#[derive(Clone, Copy)]
struct ChipStyle {
    key: Style,
    label: Style,
    short: bool,
    plain: bool,
}

fn chip_text(c: ChipSpec, short: bool, plain: bool) -> String {
    match (plain, short) {
        (true, true) => format!("[{}]", c.key),
        (true, false) => format!("[{}] {}", c.key, c.label),
        (false, true) => c.key.to_string(),
        (false, false) => format!(" {} {} ", c.key, c.label),
    }
}

fn put_chip(buf: &mut Buffer, x: u16, y: u16, max_w: usize, c: ChipSpec, st: ChipStyle) -> u16 {
    let full = chip_text(c, st.short, st.plain);
    let fw = display_width(&full);
    if st.plain || st.short || fw > max_w {
        let fitted = truncate_end(&full, max_w);
        let cw = display_width(&fitted) as u16;
        if cw > 0 {
            put_text(buf, x, y, cw, &fitted, st.key);
        }
        return cw;
    }
    put_text(buf, x, y, 1, " ", st.label);
    put_text(buf, x + 1, y, 1, &c.key.to_string(), st.key);
    let tail = format!(" {} ", c.label);
    let tw = display_width(&tail) as u16;
    put_text(buf, x + 2, y, tw, &tail, st.label);
    2 + tw
}

pub(super) fn draw_action_strip(
    buf: &mut Buffer,
    row: Rect,
    view: &CardView,
    nav_total: usize,
    pending: Option<&str>,
    theme: (&Palette, bool),
    chips_out: &mut Vec<(Rect, char)>,
) {
    let (palette, plain) = theme;
    let area = row;
    let y = row.y;
    let stepped = !plain && area.width >= 64;
    if area.width <= 2 {
        return;
    }
    let strip = if stepped {
        let rail = babylon_rail_style(palette);
        put_text(buf, area.x, y, 4, "║  │", rail);
        put_text(buf, area.right() - 4, y, 4, "│  ║", rail);
        Rect::new(area.x + 4, y, area.width - 8, 1)
    } else {
        Rect::new(area.x + 1, y, area.width - 2, 1)
    };
    let bg = Style::default().bg(palette.surface0);
    buf.set_style(strip, bg);
    let inset: u16 = if stepped { 5 } else { 2 };
    let avail = usize::from(area.width.saturating_sub(2 * inset));
    if avail == 0 {
        return;
    }

    let base_x = area.x + inset;
    let hint_fg = bg.fg(palette.subtext0);
    let key_fg = if plain {
        bg.fg(palette.text)
    } else if palette.elevation().is_terminal_16 {
        bg.fg(palette.subtext0)
    } else {
        bg.fg(palette.accent)
    };

    if view.card.is_some()
        && let Some(lbl) = pending
    {
        let right = if plain {
            "  [y] copy · esc"
        } else {
            "   y copy  · esc"
        };
        let lock_text = format!("■ {lbl}… (a/x locked)");
        let right_w = display_width(right);
        if avail >= right_w + 6 {
            let lock_fitted = truncate_end(&lock_text, avail - right_w);
            let lw = display_width(&lock_fitted) as u16;
            put_text(buf, base_x, y, lw, &lock_fitted, bg.fg(palette.yellow));
            let copy_x = base_x + lw + 2;
            let cw = put_chip(
                buf,
                copy_x,
                y,
                avail.saturating_sub(usize::from(lw) + 2),
                chip('y', "copy", true),
                ChipStyle {
                    key: key_fg,
                    label: hint_fg,
                    short: false,
                    plain,
                },
            );
            chips_out.push((Rect::new(copy_x, y, cw, 1), 'y'));
            put_text(
                buf,
                copy_x + cw,
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
            .map(|c| display_width(&chip_text(*c, short, plain)))
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
    for (idx, c) in chosen.iter().enumerate() {
        if idx > 0 {
            cx = cx.saturating_add(gap as u16);
        }
        let chip_st = ChipStyle {
            key: key_fg,
            label: hint_fg,
            short,
            plain,
        };
        let label = chip_text(*c, short, plain);
        let w = display_width(&label) as u16;
        if cx + w > right_limit {
            let rem = usize::from(right_limit.saturating_sub(cx));
            let cw = put_chip(buf, cx, y, rem, *c, chip_st);
            if cw > 0 {
                chips_out.push((Rect::new(cx, y, cw, 1), c.key));
            }
            return;
        }
        let cw = put_chip(buf, cx, y, usize::from(w), *c, chip_st);
        chips_out.push((Rect::new(cx, y, cw, 1), c.key));
        cx = cx.saturating_add(cw);
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
