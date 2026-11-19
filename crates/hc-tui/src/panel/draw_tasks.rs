//! The block's task painters behind `draw::render`: the one-line
//! task rows with their status glyph and dock state word, the
//! accepted fold and the `Space` drawer lines.

use backlot_model::state::TaskStatus;
use backlot_model::summary::TaskSummary;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::data::StudioRef;
use super::draw_blocks::draw_inner;
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::{display_width, truncate_end};
use crate::studio::glyphs::{self, PriorityMark};

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
