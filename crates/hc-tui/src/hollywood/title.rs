//! Title status and bucket-count composition (`draw_title`), framed by the
//! ornamental architrave width ladder in `architrave`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::architrave::{finish_title_frame, init_title_frame, width_of};
use super::draw::HollywoodRender;
use super::items::WaitBucket;
use super::rows;
use super::sheet::Wait;
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::panel::data::{MachineSummary, studio_count};

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
    let frame = init_title_frame(buf, area, y, palette, hits);
    let elev = palette.elevation();
    let base = frame.base;
    let x = frame.x;

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
        let right_start = frame.right_start(area, right_w);
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
    let right_start = frame.right_start(area, right_w);

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

    finish_title_frame(buf, area, y, &frame, beam_start, &chosen_right, right_w);
}
