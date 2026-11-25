//! The block painters behind `draw::render`: the bordered studio
//! frame pieces, one-line people with their badges and cue lines,
//! task rows, the accepted fold, the `Space` drawer lines, the
//! stacked single-line block and the collapsed strip's rows.
use backlot_model::summary::{OutwardCardSummary, RoleSummary};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::data::{StudioLabel, StudioRef};
use super::rows::{Owed, PersonLine};
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::{display_width, truncate_end};
use crate::studio::glyphs::{self, Health, Person, PriorityMark};

/// The frame's colour: `accent` for the selected studio (whether or
/// not the lot owns the keys), `surface1` for every other block.
pub(super) fn frame_style(selected: bool, palette: &Palette) -> Style {
    Style::default().fg(if selected {
        palette.accent
    } else {
        palette.surface1
    })
}

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
    frame: Style,
    palette: &Palette,
) {
    let right = rect.x + rect.width;
    put_text(buf, rect.x, rect.y, rect.width.min(1), "╭", frame);
    put_text(buf, right.saturating_sub(1), rect.y, 1, "╮", frame);
    // The right-hand tail: `● r1·1/4 !1` when the whole label fits
    // beside it, else `●!1` — the progress part drops before the
    // waiting count does.
    let waiting = crate::studio::waiting_count(studio.studio);
    let progress = format!(
        "r{}·{}/{}",
        studio.studio.requirement_version, studio.studio.tasks_done, studio.studio.tasks_total
    );
    let full_tail = if waiting > 0 {
        format!("{} {progress} !{waiting}", health.glyph())
    } else {
        format!("{} {progress}", health.glyph())
    };
    let marker_w = u16::from(focused_terminal) * 2;
    let chip_pad = u16::from(selected);
    let interior = rect.width.saturating_sub(4 + chip_pad);
    let full_fits = display_width(&label.full) as u16 + display_width(&full_tail) as u16
        <= interior.saturating_sub(marker_w);
    let tail = if full_fits || waiting == 0 {
        full_tail
    } else {
        format!("{}!{waiting}", health.glyph())
    };
    let tail_w = display_width(&tail) as u16;
    let tail_x = right.saturating_sub(1 + tail_w);
    if tail_w > 0 {
        put_text(
            buf,
            tail_x,
            rect.y,
            tail_w,
            &tail,
            Style::default().fg(health.color(palette)),
        );
    }
    // `!<n>` is its own colour — red while a person or outward card waits on
    // the user, peach for director-owned waits (S2 §2).
    if waiting > 0 {
        let mark_w = display_width(&format!("!{waiting}")) as u16;
        let user = crate::studio::waiting_is_user(studio.studio);
        put_text(
            buf,
            right.saturating_sub(1 + mark_w),
            rect.y,
            mark_w,
            &format!("!{waiting}"),
            Style::default().fg(if user { palette.red } else { palette.peach }),
        );
    }
    // R14: an experimental studio's `exp` badge rides after the
    // label — the label gets the full budget and a dock too narrow
    // for both drops the badge first. Selected blocks wrap `[▸ ]label`
    // in a bold padded `accent` chip (` ▸ label `).
    let id_end = tail_x.saturating_sub(1);
    let id_width = id_end.saturating_sub(rect.x + 2 + marker_w + chip_pad);
    let id_text = label.fit(id_width as usize);
    let drawn = display_width(&id_text) as u16;
    if selected && id_width > 0 {
        let chip_style = Style::default()
            .fg(glyphs::contrast_fg(palette))
            .bg(palette.accent)
            .add_modifier(Modifier::BOLD);
        put_text(buf, rect.x + 1, rect.y, 1, " ", chip_style);
        if focused_terminal {
            put_text(
                buf,
                rect.x + 2,
                rect.y,
                2,
                &format!("{} ", glyphs::FOCUSED_STUDIO),
                chip_style,
            );
        }
        put_text(
            buf,
            rect.x + 2 + marker_w,
            rect.y,
            id_width,
            &id_text,
            chip_style,
        );
        if drawn > 0 {
            put_text(
                buf,
                rect.x + 2 + marker_w + drawn,
                rect.y,
                1,
                " ",
                chip_style,
            );
        }
    } else {
        if focused_terminal && id_end > rect.x + 2 {
            put_text(
                buf,
                rect.x + 2,
                rect.y,
                1,
                glyphs::FOCUSED_STUDIO,
                Style::default()
                    .fg(palette.text)
                    .add_modifier(Modifier::BOLD),
            );
        }
        if id_width > 0 {
            put_text(
                buf,
                rect.x + 2 + marker_w,
                rect.y,
                id_width,
                &id_text,
                Style::default().fg(if focused_terminal {
                    palette.text
                } else {
                    palette.subtext0
                }),
            );
        }
    }
    let badge = studio.studio.mode == backlot_model::event::StudioMode::Experimental;
    let mut fill_start = rect.x + 2 + marker_w + drawn + u16::from(drawn > 0);
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
            &"─".repeat((id_end - fill_start) as usize),
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
        Owed::StageDown => ("◌", Style::default().fg(palette.peach)),
        Owed::Offline => ("×", Style::default().fg(palette.red)),
        Owed::Error | Owed::Confirm => ("!", Style::default().fg(palette.peach)),
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

/// The state word's style: `USER` is a red badge, `dir` a peach tag
/// on `surface0` (D9) — the rest are the mark's bare colour.
pub(super) fn word_style(mark: Person, palette: &Palette) -> Style {
    match mark {
        Person::UserPrompt => Style::default()
            .fg(glyphs::contrast_fg(palette))
            .bg(palette.red)
            .add_modifier(Modifier::BOLD),
        Person::DirPrompt => Style::default().fg(palette.peach).bg(palette.surface0),
        _ => mark.style(palette),
    }
}

/// One person inside a block: `│ <mark> <name> · <task> <flag>   <state> │`
/// — no model, no runner, no `doing` line. The mark, the state word
/// and the priority flag all read the shared vocabulary.
pub(super) fn draw_person(
    buf: &mut Buffer,
    rect: Rect,
    role: &RoleSummary,
    frame: Style,
    spin: usize,
    palette: &Palette,
) {
    draw_inner(buf, rect, frame);
    let mark = Person::of(role);
    let closed = matches!(role.state.as_str(), "closed" | "standby");
    put_text(
        buf,
        rect.x + 2,
        rect.y,
        1,
        mark.glyph(spin),
        mark.style(palette),
    );
    // `name · task ↑` left — the state word keeps the right edge,
    // with the dim runner tag before it and the short model before
    // that only in cells the whole name leaves spare (L4).
    let state = mark.word();
    let badge = matches!(mark, Person::UserPrompt | Person::DirPrompt);
    let state_width = display_width(state) as u16 + u16::from(badge) * 2;
    let flag = role.priority.and_then(PriorityMark::of);
    let flag_width = flag.map_or(0, |f| display_width(f.glyph()) + 1) as u16;
    let mut name = role.name.clone();
    if let Some(task) = &role.task {
        name.push_str(" · ");
        name.push_str(task);
    }
    let name_need = display_width(&name) as u16 + flag_width;
    let tail = super::badge::tail(role, name_need, state_width, rect.width);
    let name_width = tail.name_width;
    let name_text = truncate_end(&name, name_width.saturating_sub(flag_width) as usize);
    let drawn_name_width = display_width(&name_text) as u16;
    put_text(
        buf,
        rect.x + 4,
        rect.y,
        name_width.saturating_sub(flag_width),
        &name_text,
        Style::default().fg(if closed {
            palette.overlay0
        } else {
            palette.text
        }),
    );
    if let Some(flag) = flag {
        put_text(
            buf,
            rect.x + 4 + drawn_name_width + 1,
            rect.y,
            1,
            flag.glyph(),
            Style::default().fg(flag.color(palette)),
        );
    }
    // The dim tail: the runner tag nearest the state, the short model
    // left of it. Their cells were reserved before the name was cut,
    // so neither ever reaches under the state word.
    let mut end = rect
        .x
        .saturating_add(rect.width.saturating_sub(1 + state_width))
        .saturating_sub(1);
    if let Some(runner) = &tail.runner {
        let width = tail.runner_width();
        let x = end.saturating_sub(width);
        put_text(
            buf,
            x,
            rect.y,
            width,
            runner,
            Style::default().fg(palette.overlay0),
        );
        end = x.saturating_sub(1);
    }
    if let Some(model) = &tail.model {
        let width = tail.model_width();
        put_text(
            buf,
            end.saturating_sub(width),
            rect.y,
            width,
            model,
            Style::default().fg(palette.overlay0),
        );
    }
    let (word, word_x) = if badge {
        (
            format!(" {state} "),
            rect.x + rect.width.saturating_sub(1 + state_width),
        )
    } else {
        (
            state.to_string(),
            rect.x + rect.width.saturating_sub(1 + display_width(state) as u16),
        )
    };
    put_text(
        buf,
        word_x,
        rect.y,
        state_width,
        &word,
        word_style(mark, palette),
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
    selected: bool,
    composing: bool,
    cue: Option<(&str, ratatui::style::Color)>,
    spin: usize,
    frame: Style,
    palette: &Palette,
) -> Option<crate::chrome::help::CursorState> {
    draw_inner(buf, rect, frame);
    let mark = Style::default().fg(if composing {
        palette.accent
    } else {
        palette.overlay0
    });
    put_text(buf, rect.x + 2, rect.y, 1, "›", mark);
    let badge = cue
        .filter(|_| rect.width >= 20)
        .map(|(word, fg)| (format!("{} {word}", Person::Working.glyph(spin)), fg));
    let badge_w = badge.as_ref().map_or(0, |(b, _)| display_width(b) as u16);
    let width = if badge_w > 0 {
        rect.width.saturating_sub(5 + badge_w + 1)
    } else {
        rect.width.saturating_sub(4)
    };
    let text = Rect::new(rect.x + 4, rect.y, width, 1);
    let cursor = if composing {
        crate::chrome::text_editor::render(buf, text, draft, Style::default().fg(palette.text))
    } else if draft.as_str().is_empty() {
        put_text(
            buf,
            text.x,
            text.y,
            width,
            &truncate_end("tell producer · / actions", width as usize),
            Style::default().fg(palette.overlay0),
        );
        None
    } else {
        put_text(
            buf,
            text.x,
            text.y,
            width,
            &truncate_end(draft.as_str(), width as usize),
            Style::default().fg(if selected {
                palette.subtext0
            } else {
                palette.overlay0
            }),
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
            Style::default().fg(fg),
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
        put_text(
            buf,
            rect.x + 4,
            rect.y,
            width,
            &truncate_end(&line.text, width as usize),
            style,
        );
    }
}

/// A person's extra line at the name column: the one shown state's
/// detail. A user-owned prompt keeps `[y/d/p]`, a director's prompt
/// reads `! prompt · director reviewing`, and a needs-you attention,
/// an unanswered question or a stuck episode reads its plain words.
/// `Failed` shows the last turn's error.
pub(super) fn draw_person_line(
    buf: &mut Buffer,
    rect: Rect,
    role: &RoleSummary,
    kind: &PersonLine,
    now_ms: u64,
    frame: Style,
    palette: &Palette,
) {
    draw_inner(buf, rect, frame);
    let (text, style) = match kind {
        PersonLine::Cue => {
            let mark = Person::of(role);
            match mark {
                Person::DirPrompt => (
                    "! prompt · director reviewing".to_string(),
                    Style::default().fg(palette.overlay0),
                ),
                // A runner that cannot answer its own prompt sends
                // nothing for `y`/`d`, so the cue offers Enter instead.
                Person::UserPrompt if !role.can_answer_prompt => {
                    let detail = role
                        .attention_detail
                        .as_deref()
                        .or(role.attention.as_deref())
                        .unwrap_or_default();
                    (
                        format!("! {detail} · answer in its terminal (Enter)"),
                        Person::UserPrompt.style(palette),
                    )
                }
                Person::UserPrompt => {
                    let detail = role
                        .attention_detail
                        .as_deref()
                        .or(role.attention.as_deref())
                        .unwrap_or_default();
                    (
                        format!("! {detail} [y/d/p]"),
                        Person::UserPrompt.style(palette),
                    )
                }
                _ => (Person::state_text(role), mark.style(palette)),
            }
        }
        PersonLine::Failed => (
            role.failed
                .as_deref()
                .map(|f| format!("! last turn failed: {f}"))
                .unwrap_or_default(),
            Person::Failed.style(palette),
        ),
        PersonLine::Working => (
            super::working_line::fit_working_line(
                role,
                now_ms,
                rect.width.saturating_sub(5) as usize,
            ),
            Style::default().fg(palette.subtext0),
        ),
        PersonLine::Standby => (
            crate::studio::standby_hint(&role.role).to_string(),
            Style::default().fg(palette.overlay0),
        ),
    };
    put_text(
        buf,
        rect.x + 4,
        rect.y,
        rect.width.saturating_sub(5),
        &truncate_end(&text, rect.width.saturating_sub(5) as usize),
        style,
    );
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
