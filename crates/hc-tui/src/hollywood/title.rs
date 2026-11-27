//! The band's top title architrave and bottom marquee sill:
//! - `╔═╡ ‹✦› H O L L Y W O O D ‹✦› ╞═ … ╞═╗` (`>= 120` cols),
//!   `╔═╡ ✦ H O L L Y W O O D ╞═ … ╞═╗` (`100..=119` cols),
//!   `╔╡✦ HOLLYWOOD╞═ … ╞╗` (`< 100` cols), on `elev.stage_well`;
//! - crimson tally `◉ !B` replacing green `●` whenever blocking > 0;
//! - bottom marquee sill `╚══▪══ … ╡ ✦ ╞ … ══▪══╝` on `elev.stage_well`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::Target;
use super::draw::HollywoodRender;
use super::items::WaitBucket;
use super::rows;
use super::sheet::Wait;
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::display_width;
use crate::panel::data::{MachineSummary, studio_count};

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

fn width_of(text: &str) -> u16 {
    display_width(text) as u16
}

#[derive(Clone, Copy)]
enum RightTier {
    Full,
    Compact,
    Ultra,
}

fn build_right_segs(
    studios: usize,
    waits: &[Wait<'_>],
    title_scroll: Option<(usize, usize)>,
    tier: RightTier,
    palette: &Palette,
) -> Vec<(String, Style)> {
    let elev = palette.elevation();
    let base = Style::default().bg(elev.stage_well);
    let muted = base.fg(elev.muted_surface);
    let (mut b, mut a, mut c, mut r) = (0usize, 0usize, 0usize, 0usize);
    for w in waits {
        match WaitBucket::of(w) {
            WaitBucket::Blocking => b += 1,
            WaitBucket::Asks => a += 1,
            WaitBucket::Cards => c += 1,
            WaitBucket::Ready => r += 1,
        }
    }
    let mut segs = Vec::new();
    let has_buckets = b + a + c + r > 0 || title_scroll.is_some();
    let include_studios = !matches!(tier, RightTier::Ultra) || !has_buckets;
    if include_studios {
        let studio_text = match tier {
            RightTier::Full => format!("{studios} studio{}", if studios == 1 { "" } else { "s" }),
            RightTier::Compact | RightTier::Ultra => format!("{studios}"),
        };
        segs.push((studio_text, muted));
    }
    let sep = if matches!(tier, RightTier::Ultra) {
        " "
    } else {
        " · "
    };
    let mut push_bucket = |tok: String, fg: Color| {
        if !segs.is_empty() {
            segs.push((sep.to_string(), muted));
        }
        segs.push((tok, base.fg(fg).add_modifier(Modifier::BOLD)));
    };
    if b > 0 {
        push_bucket(format!("!{b}"), palette.red);
    }
    if a > 0 {
        push_bucket(format!("?{a}"), palette.peach);
    }
    if c > 0 {
        push_bucket(format!("{c}c"), palette.peach);
    }
    if r > 0 {
        let tok = match tier {
            RightTier::Full => format!("✓{r} ready"),
            RightTier::Compact | RightTier::Ultra => format!("✓{r}"),
        };
        push_bucket(tok, palette.teal);
    }
    if let Some((top, bot)) = title_scroll {
        if !segs.is_empty() {
            segs.push((" · ".to_string(), muted));
        }
        segs.push((
            format!("{top}–{bot}/{}", waits.len()),
            muted.add_modifier(Modifier::BOLD),
        ));
    }
    segs
}

fn segs_width(segs: &[(String, Style)]) -> u16 {
    segs.iter().map(|(s, _)| width_of(s)).sum()
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

/// The title row: brass marquee architrave on `elev.stage_well` + agent status
/// (crimson tally `◉ !B` while blocking > 0) + right-aligned bucket counts.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_title(
    buf: &mut Buffer,
    area: Rect,
    y: u16,
    machines: &[MachineSummary],
    waits: &[Wait<'_>],
    agent: bool,
    activity: rows::HollywoodActivity,
    spin: usize,
    title_scroll: Option<(usize, usize)>,
    palette: &Palette,
    hits: &mut HollywoodRender,
) {
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
    let right_cap_w = width_of(right_cap);
    let clasp_w = 2u16; // `"╡ "`

    let studios = studio_count(machines);
    let full_right = build_right_segs(studios, waits, title_scroll, RightTier::Full, palette);
    let compact_right = build_right_segs(studios, waits, title_scroll, RightTier::Compact, palette);
    let ultra_right = build_right_segs(studios, waits, title_scroll, RightTier::Ultra, palette);

    let blocking = waits
        .iter()
        .filter(|w| WaitBucket::of(w) == WaitBucket::Blocking)
        .count();
    let summary = rows::local_summary(machines);
    let hw = summary.and_then(|s| s.hollywood.as_ref());
    let state = hw.map(|h| h.state.as_str());
    let (agent_line, short_line) = if let Some(hw) = hw
        && let Some(over) =
            super::handoff_ui::title_override(hw, activity, crate::update_state::now_ms())
    {
        over
    } else if agent {
        let state = state.unwrap_or("down");
        let suffix = match activity {
            rows::HollywoodActivity::Working => " · thinking…",
            rows::HollywoodActivity::Queued => " · queued…",
            rows::HollywoodActivity::Idle => "",
        };
        let short = match activity {
            rows::HollywoodActivity::Working => "thinking…".to_string(),
            rows::HollywoodActivity::Queued => "queued…".to_string(),
            rows::HollywoodActivity::Idle => state.to_string(),
        };
        let details = summary
            .and_then(|s| s.hollywood.as_ref())
            .map(|h| {
                [h.runner.as_deref(), h.model.as_deref(), h.effort.as_deref()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" / ")
            })
            .unwrap_or_default();
        if details.is_empty() {
            (format!("{state}{suffix}"), short)
        } else {
            (format!("{state} · {details}{suffix}"), short)
        }
    } else {
        ("no agent".to_string(), String::new())
    };
    let tally_prefix = if agent && blocking > 0 {
        format!("!{blocking} · ")
    } else {
        String::new()
    };
    let tally_w = width_of(&tally_prefix);

    let pick_for = |right_w: u16| -> Option<String> {
        let right_total = if right_w > 0 {
            clasp_w + right_w + right_cap_w
        } else {
            right_cap_w
        };
        let right_start = (area.x + area.width).saturating_sub(right_total);
        // Reserve `glyph (1) + ' ' (1)` before text and ` ' '` after text before beam/clasp.
        let room = right_start.saturating_sub(x + 3);
        let full_w = tally_w + width_of(&agent_line);
        let short_w = tally_w + width_of(&short_line);
        if full_w <= room {
            Some(agent_line.clone())
        } else if !short_line.is_empty() && short_w <= room {
            Some(short_line.clone())
        } else {
            None
        }
    };

    let (chosen_right, text) = {
        let full_w = segs_width(&full_right);
        let compact_w = segs_width(&compact_right);
        let ultra_w = segs_width(&ultra_right);
        if let Some(t) = pick_for(full_w) {
            (full_right, t)
        } else if let Some(t) = pick_for(compact_w) {
            (compact_right, t)
        } else if area.width < 100 && pick_for(ultra_w).is_some() {
            let t = pick_for(ultra_w).unwrap_or_default();
            (ultra_right, t)
        } else if let Some(t) = pick_for(0) {
            (Vec::new(), t)
        } else {
            (compact_right, String::new())
        }
    };

    let (glyph, colour, bold_glyph) = if agent {
        if let Some(hw) = hw
            && super::handoff_ui::handoff_phase(hw) == Some("needs_approval")
        {
            ("⚠", palette.red, true)
        } else if let Some(hw) = hw
            && super::handoff_ui::is_quota_or_error(hw)
            && !super::handoff_ui::is_handoff_active(hw)
        {
            ("◉", palette.red, true)
        } else {
            match activity {
                rows::HollywoodActivity::Working => (
                    crate::studio::glyphs::Person::Working.glyph(spin),
                    palette.accent,
                    false,
                ),
                rows::HollywoodActivity::Queued => (
                    crate::studio::glyphs::Person::Working.glyph(spin),
                    palette.peach,
                    false,
                ),
                rows::HollywoodActivity::Idle if blocking > 0 => ("◉", palette.red, true),
                rows::HollywoodActivity::Idle => (
                    "●",
                    rows::state_color(state.unwrap_or("down"), palette),
                    false,
                ),
            }
        }
    } else {
        ("·", elev.muted_surface, false)
    };
    let right_w = segs_width(&chosen_right);
    let right_total = if right_w > 0 {
        clasp_w + right_w + right_cap_w
    } else {
        right_cap_w
    };
    let right_start = (area.x + area.width).saturating_sub(right_total);

    if x + 1 < right_start {
        let mut gstyle = base.fg(colour);
        if bold_glyph {
            gstyle = gstyle.add_modifier(Modifier::BOLD);
        }
        put_text(buf, x, y, 1, glyph, gstyle);
    }

    let mut beam_start = x + 2;
    let text_x = x + 2;
    let text_room = right_start.saturating_sub(text_x + 1);
    if !text.is_empty() && text_room > 0 {
        let mut tx = text_x;
        if !tally_prefix.is_empty() && tally_w <= text_room {
            let b_part = format!("!{blocking}");
            let bw = width_of(&b_part);
            put_text(
                buf,
                tx,
                y,
                bw,
                &b_part,
                base.fg(palette.red).add_modifier(Modifier::BOLD),
            );
            tx += bw;
            let sep_w = width_of(" · ");
            put_text(buf, tx, y, sep_w, " · ", base.fg(palette.subtext0));
            tx += sep_w;
        }
        let rem = right_start.saturating_sub(tx + 1);
        let tw = width_of(&text).min(rem);
        put_text(
            buf,
            tx,
            y,
            tw,
            &text,
            base.fg(if agent {
                palette.subtext0
            } else {
                elev.muted_surface
            }),
        );
        beam_start = tx + tw + 1;
    }

    if beam_start < right_start {
        let fill = right_start - beam_start;
        put_text(buf, beam_start, y, fill, &"═".repeat(fill as usize), brass);
    }

    if right_w > 0 && right_start >= x {
        put_text(buf, right_start, y, clasp_w, "╡ ", brass);
        let mut rx = right_start + clasp_w;
        for (seg, style) in chosen_right {
            let sw = width_of(&seg);
            put_text(buf, rx, y, sw, &seg, style);
            rx += sw;
        }
        put_text(buf, rx, y, right_cap_w, right_cap, brass);
    } else if area.width >= right_cap_w {
        let cap_x = (area.x + area.width).saturating_sub(right_cap_w);
        put_text(buf, cap_x, y, right_cap_w, right_cap, brass);
    }
}
