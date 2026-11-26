//! A cast person's rows: the first line keeps its glyph, name, task and
//! runner; the rest wraps over the lines it needs — up to four rows per
//! person — carrying the full state text and, on a line of its own, the
//! full model with effort. When the cast cannot show every role in full,
//! only the focused role keeps its height and the others fall back to
//! the two-row form.

use backlot_model::summary::RoleSummary;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::CastView;
use crate::chrome::palette::Palette;
use crate::chrome::render::{put_role_badged_text, put_text};
use crate::chrome::text::{display_width, truncate_end, wrap_text};
use crate::studio::glyphs::{self, Person, RoleBadge};

/// The text column — after the selection bar and the mark.
const TEXT_X: u16 = 3;
/// The most rows one person may take, first line included.
pub(super) const MAX_LINES: u16 = 4;
/// The wrapped lines under the first one.
const MAX_BODY: usize = MAX_LINES as usize - 1;

/// One wrapped line under a person's first row.
enum BodyLine {
    State(String),
    Model(String),
}

impl BodyLine {
    fn text(&self) -> &str {
        match self {
            BodyLine::State(text) | BodyLine::Model(text) => text,
        }
    }
}

/// Whether the role's `failed` turn is the one shown — no attention
/// and no open question outranks it.
fn failed_only(role: &RoleSummary) -> bool {
    let info_only = matches!(
        Person::attention_class(role),
        Some(backlot_model::attention::AttentionClass::Info)
    );
    role.failed.is_some() && !(role.attention.is_some() && !info_only) && role.asks.is_none()
}

/// Whether the role shows an alert that takes the model's line away.
fn alert(role: &RoleSummary) -> bool {
    let info_only = matches!(
        Person::attention_class(role),
        Some(backlot_model::attention::AttentionClass::Info)
    );
    let attention_shown = role.attention.is_some() && !info_only;
    attention_shown || role.asks.is_some() || failed_only(role)
}

/// The role's one shown state, in full words — the terminal title stands
/// in for a bare `working`/`waiting`/`ready` word when it has one.
fn state_text(role: &RoleSummary, view: &CastView<'_>) -> String {
    if role.state == "standby" {
        return crate::studio::standby_hint(&role.role).to_string();
    }
    if failed_only(role) {
        return "last turn failed".to_string();
    }
    let text = Person::state_text(role);
    let bare = matches!(text.as_str(), "working" | "waiting" | "ready");
    if bare
        && let Some(title) = role
            .terminal
            .as_deref()
            .and_then(|terminal| view.titles.get(terminal))
            .and_then(|title| title.clone())
    {
        return title;
    }
    text
}

/// The model line: the full model with its effort, or the short form
/// only when the full one cannot fit a line of its own.
fn model_text(role: &RoleSummary, width: usize) -> Option<String> {
    if alert(role) {
        return None;
    }
    let model = role.model.as_deref().filter(|model| !model.is_empty())?;
    let full = match role.effort.as_deref().filter(|effort| !effort.is_empty()) {
        Some(effort) => format!("{model} {effort}"),
        None => model.to_string(),
    };
    if display_width(&full) <= width {
        Some(full)
    } else {
        let short = glyphs::short_model(model);
        (display_width(&short) <= width).then_some(short)
    }
}

/// The wrapped lines under the first row, capped at [`MAX_BODY`].
fn body(role: &RoleSummary, view: &CastView<'_>, width: usize) -> Vec<BodyLine> {
    let mut lines: Vec<BodyLine> = wrap_text(&state_text(role, view), width)
        .into_iter()
        .map(BodyLine::State)
        .collect();
    if let Some(model) = model_text(role, width) {
        lines.push(BodyLine::Model(model));
    }
    if lines.len() > MAX_BODY {
        lines.truncate(MAX_BODY);
        if let Some(last) = lines.last_mut() {
            let text = truncate_end(last.text(), width);
            match last {
                BodyLine::State(found) | BodyLine::Model(found) => *found = text,
            }
        }
    }
    lines
}

/// A person's height at `width` when every role may show in full.
pub(super) fn full_height(role: &RoleSummary, view: &CastView<'_>, width: u16) -> u16 {
    1 + body(role, view, width.saturating_sub(TEXT_X) as usize).len() as u16
}

/// The fallback height when the roles do not all fit.
pub(super) const COMPACT_LINES: u16 = 2;

/// One person: the first line always; the rest either wrapped (`full`)
/// or the old two-row form. Focus paints the active background across
/// every row and a quiet `▎` bar so only the active pane holds an accent ring.
pub(super) fn draw(
    buffer: &mut Buffer,
    rect: Rect,
    role: &RoleSummary,
    focused: bool,
    full: bool,
    view: &CastView<'_>,
    palette: &Palette,
) {
    if rect.height == 0 {
        return;
    }
    if focused {
        buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
    }
    let dim = matches!(role.state.as_str(), "closed" | "standby");
    let mark = Person::of(role);
    for y in rect.y..rect.y.saturating_add(rect.height) {
        if focused {
            put_text(
                buffer,
                rect.x,
                y,
                1,
                "▎",
                Style::default()
                    .fg(palette.overlay1)
                    .bg(palette.active_row_bg),
            );
        }
    }
    put_text(
        buffer,
        rect.x + 1,
        rect.y,
        1,
        mark.glyph(view.spin),
        mark.style(palette),
    );
    let text_fg = |main| {
        if dim { palette.overlay0 } else { main }
    };

    // Row 1: the role badge (the actor keeps its candidate name), the
    // task and the runner right-aligned.
    let mut label = match RoleBadge::of_role(&role.role) {
        Some(RoleBadge::Actor) => format!("{} {}", RoleBadge::Actor.glyph(), role.name),
        Some(badge) => badge.glyph().to_string(),
        None => role.name.clone(),
    };
    if let Some(task) = &role.task {
        label.push(' ');
        label.push_str(task);
    }
    let runner = role.runner.as_deref().unwrap_or_default();
    let runner_width = display_width(runner).min(rect.width.saturating_sub(5) as usize) as u16;
    let name_width = rect
        .width
        .saturating_sub(3 + runner_width + u16::from(runner_width > 0));
    put_role_badged_text(
        buffer,
        rect.x + 3,
        rect.y,
        name_width,
        &truncate_end(&label, name_width as usize),
        Style::default().fg(text_fg(palette.text)),
        palette,
    );
    if runner_width > 0 {
        put_text(
            buffer,
            rect.right().saturating_sub(runner_width),
            rect.y,
            runner_width,
            runner,
            Style::default().fg(palette.overlay0),
        );
    }
    if rect.height < 2 {
        return;
    }

    let width = rect.width.saturating_sub(TEXT_X);
    if !full {
        draw_compact_row(buffer, rect, role, view, width, mark, palette);
        return;
    }
    let body = body(role, view, width as usize);
    for (index, line) in body.iter().enumerate() {
        let y = rect.y + 1 + index as u16;
        if y >= rect.bottom() {
            break;
        }
        let style = match line {
            BodyLine::Model(_) => Style::default().fg(palette.overlay0),
            BodyLine::State(_) => state_style(role, mark, dim, palette),
        };
        put_text(
            buffer,
            rect.x + TEXT_X,
            y,
            width,
            &truncate_end(line.text(), width as usize),
            style,
        );
    }
}

/// Today's two-row form: the state text left, the short model at the
/// right edge.
fn draw_compact_row(
    buffer: &mut Buffer,
    rect: Rect,
    role: &RoleSummary,
    view: &CastView<'_>,
    width: u16,
    mark: Person,
    palette: &Palette,
) {
    let dim = matches!(role.state.as_str(), "closed" | "standby");
    let model = if alert(role) {
        None
    } else {
        role.model
            .as_deref()
            .map(glyphs::short_model)
            .filter(|model| !model.is_empty())
    };
    let model_width = model
        .as_deref()
        .map(display_width)
        .unwrap_or(0)
        .min(width.saturating_sub(2) as usize) as u16;
    let left_width = width.saturating_sub(model_width + u16::from(model_width > 0));
    put_text(
        buffer,
        rect.x + TEXT_X,
        rect.y + 1,
        left_width,
        &truncate_end(&state_text(role, view), left_width as usize),
        state_style(role, mark, dim, palette),
    );
    if let Some(model) = model {
        put_text(
            buffer,
            rect.right().saturating_sub(model_width),
            rect.y + 1,
            model_width,
            &model,
            Style::default().fg(palette.overlay0),
        );
    }
}

fn state_style(role: &RoleSummary, mark: Person, dim: bool, palette: &Palette) -> Style {
    if failed_only(role) {
        Person::Failed.style(palette)
    } else if alert(role) {
        mark.style(palette)
    } else if dim {
        Style::default().fg(palette.overlay0)
    } else {
        Style::default().fg(palette.subtext0)
    }
}
