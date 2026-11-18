//! The block painters behind `draw::render`: the bordered studio
//! frame pieces, one-line people with their badges and cue lines,
//! task rows, the accepted fold, the `Space` drawer lines, the
//! stacked single-line block and the collapsed strip's rows.
use backlot_model::state::TaskStatus;
use backlot_model::summary::{RoleSummary, TaskSummary};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::data::{StudioLabel, StudioRef};
use super::draw::PanelRender;
use super::mark_counts;
use super::rows::{Owed, PersonLine};
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::{display_width, truncate_end};
use crate::studio;
use crate::studio::glyphs::{self, Health, Person, PriorityMark};

/// The frame's colour: accent only while the panel owns the keys AND
/// the block owns the selected row — every block is `surface1`
/// otherwise, focused or not (A2.3, A7.2).
pub(super) fn frame_style(selected: bool, focused: bool, palette: &Palette) -> Style {
    Style::default().fg(if focused && selected {
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
    let waiting = studio.studio.waiting;
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
    let interior = rect.width.saturating_sub(3); // `╭─ ` … `╮`
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
    // `!<n>` is its own colour — red while a person waits on the
    // user, peach for director-owned waits (S2 §2).
    if waiting > 0 {
        let mark_w = display_width(&format!("!{waiting}")) as u16;
        let user = studio.studio.roles.iter().any(|r| r.attention_user);
        put_text(
            buf,
            right.saturating_sub(1 + mark_w),
            rect.y,
            mark_w,
            &format!("!{waiting}"),
            Style::default().fg(if user { palette.red } else { palette.peach }),
        );
    }
    // `▸ ` before the label while its terminal is the focused pane.
    let id_end = tail_x.saturating_sub(1);
    let id_width = id_end.saturating_sub(rect.x + 2 + marker_w);
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
    let id_text = label.fit(id_width as usize);
    let drawn = display_width(&id_text) as u16;
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
    let fill_start = rect.x + 2 + marker_w + drawn + u16::from(drawn > 0);
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
/// variants — `◌ stage not running`, `× host unreachable`, `! error`.
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

/// The state word's style: `USER` is a red badge, `dir` a peach tag
/// on `surface0` (D9) — the rest are the mark's bare colour.
fn word_style(mark: Person, palette: &Palette) -> Style {
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
    let closed = role.state == "closed";
    put_text(
        buf,
        rect.x + 2,
        rect.y,
        1,
        mark.glyph(spin),
        mark.style(palette),
    );
    // `name · task ↑` left — the state word keeps the right edge.
    let state = mark.word();
    let badge = matches!(mark, Person::UserPrompt | Person::DirPrompt);
    let state_width = display_width(state) as u16 + u16::from(badge) * 2;
    let name_width = rect.width.saturating_sub(4 + state_width + 2);
    let flag = role.priority.and_then(PriorityMark::of);
    let flag_width = flag.map_or(0, |f| display_width(f.glyph()) + 1) as u16;
    let mut name = role.name.clone();
    if let Some(task) = &role.task {
        name.push_str(" · ");
        name.push_str(task);
    }
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

/// A person's extra line at the name column. The cue is the
/// attention's trimmed detail — `! env -i cargo…  [y/d/p]` for a
/// user-owned prompt, `! prompt · director reviewing` for a
/// director-owned one (D9), `! <attention>` otherwise. `Failed`
/// shows the last turn's error; `ClosedHint`/`Confirm` are the
/// reopen affordances.
pub(super) fn draw_person_line(
    buf: &mut Buffer,
    rect: Rect,
    role: &RoleSummary,
    kind: &PersonLine,
    frame: Style,
    palette: &Palette,
) {
    draw_inner(buf, rect, frame);
    let (text, style) = match kind {
        PersonLine::Cue => {
            let detail = role
                .attention_detail
                .as_deref()
                .or(role.attention.as_deref())
                .unwrap_or_default();
            if Person::of(role) == Person::DirPrompt {
                (
                    "! prompt · director reviewing".to_string(),
                    Style::default().fg(palette.overlay0),
                )
            } else if role.attention_user {
                (
                    format!("! {detail} [y/d/p]"),
                    Person::UserPrompt.style(palette),
                )
            } else {
                (format!("! {detail}"), Person::of(role).style(palette))
            }
        }
        PersonLine::Failed => (
            role.failed
                .as_deref()
                .map(|f| format!("! last turn failed: {f}"))
                .unwrap_or_default(),
            Person::Failed.style(palette),
        ),
        PersonLine::ClosedHint => (
            "closed — press o to reopen".to_string(),
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

/// A task row: `│ · task t2 (waits t1)           dep │` — the
/// glyph is the status', the state word the dock's own (`dep`,
/// `uncast`, `cap`, `review`, `done`). Behind tasks carry
/// ` v<pin>→v<cur>` after the id (S2 §2).
pub(super) fn draw_task(
    buf: &mut Buffer,
    rect: Rect,
    studio: StudioRef<'_>,
    task: &TaskSummary,
    frame: Style,
    palette: &Palette,
) {
    draw_inner(buf, rect, frame);
    let mark = glyphs::Task::of(task.status);
    put_text(
        buf,
        rect.x + 2,
        rect.y,
        1,
        mark.glyph(),
        Style::default().fg(mark.color(palette)),
    );
    let word = task_word(task);
    let word_w = display_width(word) as u16;
    let flag = PriorityMark::of(task.priority);
    let flag_w = flag.map_or(0, |f| display_width(f.glyph()) + 1) as u16;
    let mut text = format!("task {}", task.id);
    if task.behind {
        text = format!(
            "{text} v{}→v{}",
            task.requirement_version, studio.studio.requirement_version
        );
    }
    if !task.waits_for.is_empty() {
        text = format!("{text} (waits {})", task.waits_for.join(", "));
    } else if let Some(name) = &task.actor_name {
        text = format!("{text} · {name}");
    }
    let text_w = rect.width.saturating_sub(5 + word_w + 1 + flag_w);
    let shown = truncate_end(&text, text_w as usize);
    let shown_w = display_width(&shown) as u16;
    put_text(
        buf,
        rect.x + 4,
        rect.y,
        text_w,
        &shown,
        Style::default().fg(palette.text),
    );
    if let Some(flag) = flag {
        put_text(
            buf,
            rect.x + 4 + shown_w + 1,
            rect.y,
            1,
            flag.glyph(),
            Style::default().fg(flag.color(palette)),
        );
    }
    put_text(
        buf,
        rect.x + rect.width.saturating_sub(1 + word_w),
        rect.y,
        word_w,
        word,
        Style::default().fg(mark.color(palette)),
    );
}

/// The task row's right-edge state word — the dock's own short words
/// (S2 §2), not the ledger's enum names.
fn task_word(task: &TaskSummary) -> &'static str {
    match task.status {
        TaskStatus::Created if !task.waits_for.is_empty() => "dep",
        TaskStatus::Created => "uncast",
        TaskStatus::Running => "run",
        TaskStatus::Delivered => "review",
        TaskStatus::Paused => "paused",
        TaskStatus::Blocked => "blocked",
        TaskStatus::AtCap => "cap",
        TaskStatus::Accepted | TaskStatus::PartiallyAccepted => "done",
        TaskStatus::Abandoned | TaskStatus::TakenOver => "dead",
    }
}

/// The `✓ <n> accepted` fold line — `▾` open, `▸` shut.
pub(super) fn draw_fold(
    buf: &mut Buffer,
    rect: Rect,
    studio: StudioRef<'_>,
    open: bool,
    frame: Style,
    palette: &Palette,
) {
    draw_inner(buf, rect, frame);
    let n = studio
        .studio
        .tasks
        .iter()
        .filter(|t| {
            matches!(
                t.status,
                TaskStatus::Accepted | TaskStatus::PartiallyAccepted
            )
        })
        .count();
    let text = format!("{} ✓ {n} accepted", if open { "▾" } else { "▸" });
    put_text(
        buf,
        rect.x + 2,
        rect.y,
        rect.width.saturating_sub(3),
        &text,
        Style::default().fg(palette.teal),
    );
}

/// A `Space` drawer's content line — the `┆` guide in `overlay0`
/// over a `surface_dim` field, indented under the owner's name
/// column (S2 §3).
pub(super) fn draw_drawer(
    buf: &mut Buffer,
    rect: Rect,
    text: &str,
    frame: Style,
    palette: &Palette,
) {
    draw_inner(buf, rect, frame);
    let inner = Rect {
        x: rect.x + 1,
        width: rect.width.saturating_sub(2),
        ..rect
    };
    buf.set_style(inner, Style::default().bg(palette.surface_dim));
    put_text(
        buf,
        rect.x + 2,
        rect.y,
        1,
        "┆",
        Style::default()
            .fg(palette.overlay0)
            .bg(palette.surface_dim),
    );
    put_text(
        buf,
        rect.x + 4,
        rect.y,
        rect.width.saturating_sub(5),
        &truncate_end(text, rect.width.saturating_sub(5) as usize),
        Style::default()
            .fg(palette.subtext0)
            .bg(palette.surface_dim),
    );
}

/// Right-aligned `(<text>, <colour>)` parts — `count` marks the
/// label's right end; returns where the tail started.
fn put_parts_right(buf: &mut Buffer, rect: Rect, parts: &[(String, Color)]) -> u16 {
    let glyphs: u16 = parts
        .iter()
        .map(|(part, _)| display_width(part) as u16)
        .sum();
    let spaces = parts.len().saturating_sub(1) as u16;
    let mut x = rect.x + rect.width.saturating_sub(glyphs + spaces);
    for (part, color) in parts {
        let w = display_width(part) as u16;
        put_text(buf, x, rect.y, w, part, Style::default().fg(*color));
        x = x.saturating_add(w + 1);
    }
    rect.x + rect.width.saturating_sub(glyphs + spaces)
}

/// A stacked block's single line: `╶ <label> ⠿2 ○3 ×1 !<n>` — each
/// mark count in its mark's colour, the attention count in the
/// blocked colour.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_stacked(
    buf: &mut Buffer,
    rect: Rect,
    studio: StudioRef<'_>,
    label: &StudioLabel,
    spin: usize,
    palette: &Palette,
    hits: &mut PanelRender,
) {
    put_text(
        buf,
        rect.x,
        rect.y,
        1,
        "╶",
        Style::default().fg(palette.overlay0),
    );
    let mut parts = Vec::new();
    for (mark, n) in mark_counts(studio.studio) {
        hits.working_drawn |= mark == Person::Working;
        parts.push((format!("{}{}", mark.glyph(spin), n), mark.color(palette)));
    }
    let attention = studio::attention_count(studio.studio);
    if attention > 0 {
        parts.push((format!("!{attention}"), palette.red));
    }
    let tail = put_parts_right(buf, rect, &parts);
    let gap = u16::from(!parts.is_empty());
    let id_end = tail.saturating_sub(gap);
    if id_end > rect.x + 2 {
        let id_width = id_end - (rect.x + 2);
        put_text(
            buf,
            rect.x + 2,
            rect.y,
            id_width,
            &label.fit(id_width as usize),
            Style::default().fg(palette.subtext0),
        );
    }
}

/// The collapsed strip's studio line: `● acme·Loc  1/3` — the
/// health glyph, a short `id·<machine>` label and `<done>/<total>`
/// or `!<n>` right-aligned (A4.3). Never blank: a studio with no
/// tasks and nothing waiting still shows its health and name.
pub(super) fn draw_strip(
    buf: &mut Buffer,
    rect: Rect,
    studio: StudioRef<'_>,
    health: Health,
    local: Option<&str>,
    palette: &Palette,
) {
    put_text(
        buf,
        rect.x,
        rect.y,
        1,
        health.glyph(),
        Style::default().fg(health.color(palette)),
    );
    let waiting = studio.studio.waiting;
    let mut parts = Vec::new();
    if waiting > 0 {
        let user = studio.studio.roles.iter().any(|r| r.attention_user);
        parts.push((
            format!("!{waiting}"),
            if user { palette.red } else { palette.peach },
        ));
    }
    if studio.studio.tasks_total > 0 {
        parts.push((
            format!("{}/{}", studio.studio.tasks_done, studio.studio.tasks_total),
            palette.overlay0,
        ));
    }
    let tail = put_parts_right(buf, rect, &parts);
    let gap = u16::from(!parts.is_empty());
    // `<short_id>·<machine 2-3>` — the machine keeps its three
    // chars, the id truncates first (A4.3).
    let machine = studio.machine.or(local).unwrap_or("local");
    let short_m: String = machine.chars().take(3).collect();
    let label_w = tail.saturating_sub(rect.x + 2 + gap);
    let id_room = (label_w as usize).saturating_sub(1 + short_m.chars().count());
    let id = truncate_end(&studio.studio.id, id_room.max(1));
    let label = format!("{id}·{short_m}");
    if label_w > 0 {
        put_text(
            buf,
            rect.x + 2,
            rect.y,
            label_w,
            &truncate_end(&label, label_w as usize),
            Style::default().fg(palette.text),
        );
    }
}

/// A strip's active-or-blocked role line: `× dir      USER` or
/// `◐ act-a    t1 ↑` — the mark keeps its colour, the name
/// truncates, the task id and its priority flag or the `USER` word
/// hold the right edge (A4.3).
pub(super) fn draw_strip_role(buf: &mut Buffer, rect: Rect, role: &RoleSummary, palette: &Palette) {
    let mark = Person::of(role);
    put_text(
        buf,
        rect.x + 1,
        rect.y,
        1,
        mark.glyph(0),
        mark.style(palette),
    );
    let flag = role.priority.and_then(PriorityMark::of);
    let mut right = String::new();
    if mark == Person::UserPrompt {
        right = "USER".to_string();
    } else if let Some(task) = &role.task {
        right = task.clone();
        if let Some(flag) = flag {
            right.push(' ');
            right.push_str(flag.glyph());
        }
    } else if mark == Person::DirPrompt {
        right = "dir".to_string();
    }
    let right_w = display_width(&right) as u16;
    if right_w > 0 {
        put_text(
            buf,
            rect.x + rect.width.saturating_sub(right_w),
            rect.y,
            right_w,
            &right,
            Style::default().fg(if mark == Person::UserPrompt {
                palette.red
            } else {
                palette.subtext0
            }),
        );
    }
    let name_w = rect
        .width
        .saturating_sub(3 + right_w + u16::from(right_w > 0));
    put_text(
        buf,
        rect.x + 3,
        rect.y,
        name_w,
        &truncate_end(&role.name, name_w as usize),
        Style::default().fg(palette.subtext0),
    );
}
