//! The block painters behind `draw::render`: the bordered studio
//! frame pieces, one-line people with their badges and cue lines,
//! task rows, the accepted fold, the `Space` drawer lines, the
//! stacked single-line block and the collapsed strip's rows.
use backlot_model::summary::{OutwardCardSummary, RoleSummary};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::data::{StudioLabel, StudioRef};
use super::draw_deco_blocks::{
    BlockWeight, draw_entered_tag, entered_width, finished_top_tail, paint_finished_tail,
};
use super::rows::Owed;
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::{display_width, truncate_end};
use crate::studio::glyphs::{self, Health, Person};

/// The `│` edges of a block's interior row.
pub(super) fn draw_inner(buf: &mut Buffer, rect: Rect, edge: Style) {
    put_text(buf, rect.x, rect.y, 1, "│", edge);
    if rect.width > 1 {
        put_text(buf, rect.x + rect.width - 1, rect.y, 1, "│", edge);
    }
}

/// `╭─ ▸ <label> ── <health> r<ver>·<done>/<total> !<n> ─╮` — the
/// studio header in the top border (S2 §2): the `▸` marks the studio
/// whose terminal is focused, the health glyph names machine/stage/
/// finish state, the counters the requirement progress, `!` the
/// waiting count — red when a person waits on the user, peach
/// otherwise. A narrow dock drops the `r<ver>·d/t` part first, then
/// squeezes `!` against the health glyph: `●!1`.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_top(
    buf: &mut Buffer,
    rect: Rect,
    studio: StudioRef<'_>,
    label: &StudioLabel,
    health: Health,
    selected: bool,
    focused_terminal: bool,
    weight: BlockWeight,
    plain_chrome: bool,
    frame: Style,
    palette: &Palette,
) {
    let entered = weight == BlockWeight::Entered;
    let right = rect.x + rect.width;
    let (left_corner, fill_glyph, right_corner) = weight.top_corners();
    put_text(buf, rect.x, rect.y, rect.width.min(1), left_corner, frame);
    put_text(buf, right.saturating_sub(1), rect.y, 1, right_corner, frame);
    let edge_pad = weight != BlockWeight::Resting && rect.width >= 36;
    if edge_pad {
        put_text(buf, rect.x + 1, rect.y, 1, fill_glyph, frame);
        put_text(buf, right - 2, rect.y, 1, fill_glyph, frame);
    }
    // The right-hand tail: `● r1·1/4 !1` when the whole label fits
    // beside it, else `●!1` — the progress part drops before the
    // waiting count does.
    let waiting = crate::studio::waiting_count(studio.studio);
    let marker_w = u16::from(focused_terminal) * 2;
    let has_pill = selected || focused_terminal;
    let chip_pad = u16::from(has_pill);
    let side_pads = u16::from(edge_pad) * 2;
    let interior = rect.width.saturating_sub(4 + chip_pad + side_pads);
    let entered_w = entered_width(entered, plain_chrome);
    let (tail, finished_deco) = if let Some((fin_tail, deco_c)) = finished_top_tail(
        studio,
        rect.width,
        interior.saturating_sub(marker_w),
        display_width(&label.full) as u16,
        plain_chrome,
    ) {
        (fin_tail, Some(deco_c))
    } else {
        let mut progress = format!(
            "r{}·{}/{}",
            studio.studio.requirement_version, studio.studio.tasks_done, studio.studio.tasks_total
        );
        // The studio-wide actor cap beside the progress: `actors 2/4` once
        // the document carries one — older documents read 0 and show none.
        if studio.studio.actors_live > 0 && studio.studio.actors_cap > 0 {
            progress.push_str(&format!(
                " · {}/{}",
                studio.studio.actors_live, studio.studio.actors_cap
            ));
        }
        let full_tail = if waiting > 0 {
            format!("{} {progress} !{waiting}", health.glyph())
        } else {
            format!("{} {progress}", health.glyph())
        };
        let full_fits =
            display_width(&label.full) as u16 + display_width(&full_tail) as u16 + entered_w
                <= interior.saturating_sub(marker_w);
        let tail = if full_fits {
            full_tail
        } else if waiting == 0 {
            if entered {
                health.glyph().to_string()
            } else {
                full_tail
            }
        } else {
            format!("{}!{waiting}", health.glyph())
        };
        (tail, None)
    };
    let tail_w = display_width(&tail) as u16;
    let tail_right_pad = 1 + u16::from(edge_pad);
    let tail_x = right.saturating_sub(tail_right_pad + tail_w);
    if tail_w > 0 {
        if let Some(deco_c) = finished_deco {
            paint_finished_tail(
                buf,
                tail_x,
                rect.y,
                &tail,
                deco_c,
                health.color(palette),
                palette,
            );
        } else {
            put_text(
                buf,
                tail_x,
                rect.y,
                tail_w,
                &tail,
                Style::default().fg(health.color(palette)),
            );
            // `!<n>` is its own colour — red while a person or outward card waits on
            // the user, peach for director-owned waits (S2 §2).
            if waiting > 0 {
                let mark_w = display_width(&format!("!{waiting}")) as u16;
                let user = crate::studio::waiting_is_user(studio.studio);
                put_text(
                    buf,
                    right.saturating_sub(tail_right_pad + mark_w),
                    rect.y,
                    mark_w,
                    &format!("!{waiting}"),
                    Style::default().fg(if user { palette.red } else { palette.peach }),
                );
            }
        }
    }
    // R14: an experimental studio's `exp` badge rides after the
    // label — the label gets the full budget and a dock too narrow
    // for both drops the badge first. Selected blocks wrap `[▸ ]label`
    // in a bold padded `accent` chip; an unselected current block (`▸`)
    // sits in a subtle raised capsule (`elevation.raised_row`, or bold+reverse
    // on 16-color terminals); other blocks draw bold `palette.text`.
    let id_end = tail_x.saturating_sub(1);
    let left_pad = u16::from(edge_pad);
    let plain_pad = u16::from(!has_pill && !plain_chrome && !edge_pad);
    let pill_x = rect.x + 1 + left_pad;
    let id_start = rect.x + 2 + left_pad + plain_pad + marker_w;
    let id_width = id_end.saturating_sub(id_start + chip_pad);
    let id_text = label.fit(id_width as usize);
    let drawn = display_width(&id_text) as u16;
    if has_pill && id_width > 0 {
        let pill_style = if selected {
            Style::default()
                .fg(glyphs::contrast_fg(palette))
                .bg(palette.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            glyphs::studio_capsule_style(palette)
        };
        put_text(buf, pill_x, rect.y, 1, " ", pill_style);
        if focused_terminal {
            put_text(
                buf,
                pill_x + 1,
                rect.y,
                2,
                &format!("{} ", glyphs::FOCUSED_STUDIO),
                pill_style,
            );
        }
        put_text(buf, id_start, rect.y, id_width, &id_text, pill_style);
        if drawn > 0 {
            put_text(buf, id_start + drawn, rect.y, 1, " ", pill_style);
        }
    } else if id_width > 0 {
        if !plain_chrome && !edge_pad && rect.width > 3 {
            put_text(buf, rect.x + 1, rect.y, 1, fill_glyph, frame);
        }
        put_text(
            buf,
            id_start,
            rect.y,
            id_width,
            &id_text,
            Style::default()
                .fg(palette.text)
                .add_modifier(Modifier::BOLD),
        );
    }
    let badge = studio.studio.mode == backlot_model::event::StudioMode::Experimental;
    let mut fill_start = id_start + drawn + u16::from(drawn > 0);
    if entered {
        fill_start = draw_entered_tag(
            buf,
            fill_start,
            rect.y,
            id_end,
            fill_glyph,
            plain_chrome,
            frame,
            palette,
        );
    }
    if badge && fill_start + 3 < id_end {
        put_text(
            buf,
            fill_start,
            rect.y,
            3,
            "exp",
            Style::default()
                .fg(palette.peach)
                .add_modifier(Modifier::BOLD),
        );
        fill_start += 4;
    }
    if id_end > fill_start {
        put_text(
            buf,
            fill_start,
            rect.y,
            id_end - fill_start,
            &fill_glyph.repeat((id_end - fill_start) as usize),
            frame,
        );
    }
}

/// The owed-step subheader: `→ <label> · <name>` and its state
/// variants — `◌ stage stopped · s start`, `× host unreachable`,
/// `! error`.
pub(super) fn draw_owed(
    buf: &mut Buffer,
    rect: Rect,
    kind: Owed,
    text: &str,
    frame: Style,
    palette: &Palette,
) {
    draw_inner(buf, rect, frame);
    let (mark, style) = match kind {
        Owed::Step | Owed::Finish => ("→", Style::default().fg(palette.subtext0)),
        Owed::Next => ("·", Style::default().fg(palette.subtext0)),
        Owed::ReportPending => ("·", Style::default().fg(palette.overlay0)),
        Owed::StageDown | Owed::ServerStopped => ("◌", Style::default().fg(palette.peach)),
        Owed::Offline => ("×", Style::default().fg(palette.red)),
        Owed::Error | Owed::Upgrade | Owed::Confirm => ("!", Style::default().fg(palette.peach)),
    };
    let width = rect.width.saturating_sub(3);
    put_text(
        buf,
        rect.x + 2,
        rect.y,
        width,
        &truncate_end(&format!("{mark} {text}"), width as usize),
        style,
    );
}

/// The studio's usage line (P4a): `∑ 12.4k/3.1k tok · $0.08` —
/// dim, drawn only while `usage` is `Some` so a usage-less studio
/// never shows it. Truncates at the block's edge like `draw_owed`.
pub(super) fn draw_usage(
    buf: &mut Buffer,
    rect: Rect,
    studio: StudioRef<'_>,
    frame: Style,
    palette: &Palette,
) {
    draw_inner(buf, rect, frame);
    let Some(usage) = &studio.studio.usage else {
        return;
    };
    let width = rect.width.saturating_sub(3);
    put_text(
        buf,
        rect.x + 2,
        rect.y,
        width,
        &truncate_end(&format!("∑ {}", usage.compact_dock()), width as usize),
        Style::default().fg(palette.overlay0),
    );
}

/// One person inside a block: strict single-line row shared with Cast —
/// `│ <mark> <role_badge>[ <flag>]  <state>[ <age>][ · <detail…>] │`.
pub(super) fn draw_person(
    buf: &mut Buffer,
    rect: Rect,
    role: &RoleSummary,
    frame: Style,
    spin: usize,
    now_ms: u64,
    palette: &Palette,
) {
    draw_inner(buf, rect, frame);
    crate::studio::role_row::render_role_row(
        buf,
        rect,
        rect.x + 2,
        rect.x + 4,
        rect.right().saturating_sub(1),
        role,
        spin,
        now_ms,
        None,
        palette,
    );
}

/// The block's `›` line — the studio's own input surface inside its
/// frame. `draft` is that studio's parked buffer (empty when it never
/// typed); `composing` is the explicit `panel.editing` latch — the
/// row shows the editor's viewport and returns the hardware cursor
/// only then. Resting draws the placeholder — or the parked draft —
/// dim, so the line reads as an affordance, not a cursor.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_input(
    buf: &mut Buffer,
    rect: Rect,
    draft: &crate::chrome::text_editor::TextEditor,
    target_label: &str,
    selected: bool,
    composing: bool,
    cue: Option<(&str, ratatui::style::Color)>,
    spin: usize,
    frame: Style,
    palette: &Palette,
) -> Option<crate::chrome::help::CursorState> {
    let elev = palette.elevation();
    let active_field = selected || composing;
    let field_bg = if active_field {
        elev.raised_field
    } else {
        elev.surface
    };
    let ph_fg = if active_field {
        elev.muted_raised
    } else {
        elev.muted_surface
    };
    draw_inner(buf, rect, frame);
    if rect.width > 2 {
        let inner = Rect::new(rect.x + 1, rect.y, rect.width.saturating_sub(2), 1);
        buf.set_style(inner, Style::default().bg(field_bg));
        let (bar, bar_fg) = if selected || composing {
            (glyphs::SELECTION_BAR, palette.accent)
        } else {
            ("▎", elev.border_muted)
        };
        put_text(
            buf,
            rect.x + 1,
            rect.y,
            1,
            bar,
            Style::default().fg(bar_fg).bg(field_bg),
        );
    }
    let mark = if composing {
        Style::default()
            .fg(palette.accent)
            .bg(field_bg)
            .add_modifier(Modifier::BOLD)
    } else if selected {
        Style::default().fg(palette.accent).bg(field_bg)
    } else {
        Style::default().fg(ph_fg).bg(field_bg)
    };
    put_text(buf, rect.x + 2, rect.y, 1, "›", mark);
    let badge = cue
        .filter(|_| rect.width >= 20)
        .map(|(word, fg)| (format!("{} {word}", Person::Working.glyph(spin)), fg));
    let badge_w = badge.as_ref().map_or(0, |(b, _)| display_width(b) as u16);
    let width = if badge_w > 0 {
        rect.width.saturating_sub(5 + badge_w + 1)
    } else {
        rect.width.saturating_sub(5)
    };
    let text = Rect::new(rect.x + 4, rect.y, width, 1);
    let cursor = if composing {
        let cursor = crate::chrome::text_editor::render(
            buf,
            text,
            draft,
            Style::default().fg(palette.text).bg(field_bg),
        );
        if draft.as_str().is_empty() && width > 0 {
            let ph = format!("tell {target_label} · / actions");
            put_text(
                buf,
                text.x,
                text.y,
                width,
                &truncate_end(&ph, width as usize),
                Style::default()
                    .fg(ph_fg)
                    .bg(field_bg)
                    .add_modifier(Modifier::ITALIC),
            );
        }
        cursor
    } else if draft.as_str().is_empty() {
        let ph = format!("tell {target_label} · / actions");
        put_text(
            buf,
            text.x,
            text.y,
            width,
            &truncate_end(&ph, width as usize),
            Style::default()
                .fg(ph_fg)
                .bg(field_bg)
                .add_modifier(Modifier::ITALIC),
        );
        None
    } else {
        put_text(
            buf,
            text.x,
            text.y,
            width,
            &truncate_end(draft.as_str(), width as usize),
            Style::default()
                .fg(if selected { palette.subtext0 } else { ph_fg })
                .bg(field_bg),
        );
        None
    };
    if let Some((badge, fg)) = badge {
        put_text(
            buf,
            rect.x + rect.width.saturating_sub(1 + badge_w),
            rect.y,
            badge_w,
            &badge,
            Style::default().fg(fg).bg(field_bg),
        );
    }
    cursor
}

/// A finished studio's resting row: `│ ↺ reopen producer [o]    │`
/// — the row where a live studio's `›` line sits (R3-N7).
pub(super) fn draw_reopen(buf: &mut Buffer, rect: Rect, frame: Style, palette: &Palette) {
    draw_inner(buf, rect, frame);
    let width = rect.width.saturating_sub(3);
    put_text(
        buf,
        rect.x + 2,
        rect.y,
        width,
        &truncate_end("↺ reopen producer [o]", width as usize),
        Style::default().fg(palette.teal),
    );
}

/// One user-facing exchange turn inside a block: `│ › <text> │` for
/// the user (`subtext0`) and `│ ✦ <text> │` for `producer` /
/// `hollywood` / reports (`accent` glyph, `text` snippet), matching
/// the Call Sheet's talk rows.
pub(super) fn draw_talk(
    buf: &mut Buffer,
    rect: Rect,
    line: &backlot_model::summary::StudioExchange,
    selected: bool,
    frame: Style,
    palette: &Palette,
) {
    draw_inner(buf, rect, frame);
    let from_user = line.who == "user";
    let (glyph, glyph_fg, text_fg) = if from_user {
        (
            "›",
            palette.subtext0,
            if selected {
                palette.text
            } else {
                palette.subtext0
            },
        )
    } else {
        ("✦", palette.accent, palette.text)
    };
    put_text(
        buf,
        rect.x + 2,
        rect.y,
        1,
        glyph,
        Style::default().fg(glyph_fg),
    );
    let width = rect.width.saturating_sub(5);
    if width > 0 {
        let mut style = Style::default().fg(text_fg);
        if selected {
            style = style.add_modifier(Modifier::BOLD);
        }
        let display = match line.dropped_reason.as_deref() {
            Some(reason) => format!("{} (dropped: {reason})", line.text),
            None => line.text.clone(),
        };
        put_text(
            buf,
            rect.x + 4,
            rect.y,
            width,
            &truncate_end(&display, width as usize),
            style,
        );
    }
}

/// One open outward push/PR card inside an expanded block:
/// `│ ◆ c1 git push origin HEAD:main      a/y x/d ⏎ │` (shortening hints first).
pub(super) fn draw_outward_card(
    buf: &mut Buffer,
    rect: Rect,
    card: &OutwardCardSummary,
    frame: Style,
    palette: &Palette,
) {
    draw_inner(buf, rect, frame);
    if rect.width < 6 {
        return;
    }
    put_text(
        buf,
        rect.x + 2,
        rect.y,
        1,
        "◆",
        Style::default().fg(palette.accent),
    );
    let inner = rect.width.saturating_sub(5) as usize;
    let hint = "a/y x/d ⏎";
    let hint_w = display_width(hint);
    let label = format!("{} {}", card.id, card.command);
    if inner > display_width(&label) + hint_w {
        let label_w = inner - hint_w - 1;
        put_text(
            buf,
            rect.x + 4,
            rect.y,
            label_w as u16,
            &truncate_end(&label, label_w),
            Style::default().fg(palette.text),
        );
        put_text(
            buf,
            rect.x + 4 + (inner - hint_w) as u16,
            rect.y,
            hint_w as u16,
            hint,
            Style::default().fg(palette.overlay0),
        );
    } else {
        put_text(
            buf,
            rect.x + 4,
            rect.y,
            inner as u16,
            &truncate_end(&label, inner),
            Style::default().fg(palette.text),
        );
    }
}
