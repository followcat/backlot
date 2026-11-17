//! The band under the frames: the headline, one row per open card,
//! the `tell hollywood` input line, the no-agent setup line and the
//! key hints. Every cell names its own background — the band floats
//! over the panes.
//!
//! The rows are drawn at rects the height ladder chose in `draw`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::draw::{BtnKind, CardBtn, HollywoodRender};
use super::rows::{Row, SETUP_COMMAND, SETUP_LEAD};
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::truncate_end;

/// The hint line with an agent.
pub(crate) const HINTS: &str =
    "← → move   ⏎ open   n new   a apply   c cancel   i tell   esc close";
/// The hint line without one: the agent's keys are gone.
pub(crate) const HINTS_NO_AGENT: &str = "← → move   ⏎ open   n new   esc close";

/// The space the band keeps at each side.
pub(crate) const MARGIN: u16 = 2;

/// Draw one band row across `rect`.
pub(crate) fn draw_row(
    buf: &mut Buffer,
    rect: Rect,
    row: &Row<'_>,
    hollywood: &super::Hollywood,
    palette: &Palette,
    hits: &mut HollywoodRender,
) {
    match row {
        Row::StageDown => put_text(
            buf,
            rect.x + MARGIN,
            rect.y,
            rect.width.saturating_sub(MARGIN),
            " stage not running",
            Style::default()
                .fg(palette.overlay0)
                .bg(palette.surface_dim),
        ),
        Row::Headline(headline) => {
            put_text(
                buf,
                rect.x + MARGIN,
                rect.y,
                1,
                "✦",
                Style::default().fg(palette.accent).bg(palette.surface_dim),
            );
            let width = rect.width.saturating_sub(MARGIN * 2 + 2);
            put_text(
                buf,
                rect.x + MARGIN + 2,
                rect.y,
                width,
                &truncate_end(headline, width as usize),
                Style::default()
                    .fg(palette.subtext0)
                    .bg(palette.surface_dim)
                    .add_modifier(Modifier::ITALIC),
            );
        }
        Row::Card(card) => draw_card(buf, rect, card, palette, hits),
        Row::Input => draw_input(buf, rect, hollywood, palette, hits),
        Row::Setup => draw_setup(buf, rect, palette),
    }
    hits.rows.push((rect, row.target()));
}

/// The key hints, right-aligned in `rect`.
pub(crate) fn draw_hints(buf: &mut Buffer, rect: Rect, agent: bool, palette: &Palette) {
    let text = if agent { HINTS } else { HINTS_NO_AGENT };
    let width = text.chars().count() as u16;
    let x = (rect.x + rect.width.saturating_sub(MARGIN))
        .saturating_sub(width)
        .max(rect.x);
    put_text(
        buf,
        x,
        rect.y,
        width.min(rect.width),
        &truncate_end(text, rect.width as usize),
        Style::default()
            .fg(palette.overlay0)
            .bg(palette.surface_dim),
    );
}

/// `c<n>` in accent, `<studio> · <action>` in text, ` — <why>` dim,
/// and the right-aligned `apply cancel show` chips — each its own hit
/// rect.
fn draw_card(
    buf: &mut Buffer,
    rect: Rect,
    card: &backlot_model::summary::CardSummary,
    palette: &Palette,
    hits: &mut HollywoodRender,
) {
    const BUTTONS: [(&str, BtnKind); 3] = [
        ("apply", BtnKind::Apply),
        ("cancel", BtnKind::Cancel),
        ("show", BtnKind::Show),
    ];
    let base = Style::default().bg(palette.surface_dim);
    // Chips hug the right edge: ` apply `, ` cancel `, ` show `.
    let buttons_width: u16 = BUTTONS
        .iter()
        .map(|(label, _)| label.len() as u16 + 2)
        .sum();
    let right = rect.x + rect.width.saturating_sub(MARGIN);
    let text_width = right.saturating_sub(rect.x + MARGIN + buttons_width);
    let x = rect.x + MARGIN;
    let id = format!(" {} ", card.id);
    let id_w = id.chars().count() as u16;
    put_text(
        buf,
        x,
        rect.y,
        id_w.min(text_width),
        &id,
        base.fg(palette.accent).add_modifier(Modifier::BOLD),
    );
    let head = format!("{} · {}", card.studio, card.action);
    let head_w = head.chars().count() as u16;
    let room = text_width.saturating_sub(id_w);
    put_text(
        buf,
        x + id_w,
        rect.y,
        room,
        &truncate_end(&head, room as usize),
        base.fg(palette.text),
    );
    let why_x = x + id_w + head_w.min(room);
    let why_room = right.saturating_sub(why_x + buttons_width);
    if why_room > 3 {
        put_text(
            buf,
            why_x,
            rect.y,
            why_room,
            &truncate_end(&format!(" — {}", card.why), why_room as usize),
            base.fg(palette.overlay0),
        );
    }
    let mut bx = right - buttons_width;
    for (label, kind) in BUTTONS {
        let w = label.len() as u16 + 2;
        let chip = format!(" {label} ");
        let style = match kind {
            BtnKind::Apply => base
                .fg(palette.panel_bg)
                .bg(palette.accent)
                .add_modifier(Modifier::BOLD),
            BtnKind::Cancel | BtnKind::Show => base.fg(palette.text).bg(palette.surface0),
        };
        put_text(buf, bx, rect.y, w, &chip, style);
        hits.buttons.push((
            Rect::new(bx + 1, rect.y, w - 2, 1),
            CardBtn {
                card: card.id.clone(),
                kind,
            },
        ));
        bx += w;
    }
}

/// The `tell hollywood` line on `active_row_bg`: `›` in accent, then
/// the placeholder or the editor's text.
fn draw_input(
    buf: &mut Buffer,
    rect: Rect,
    hollywood: &super::Hollywood,
    palette: &Palette,
    hits: &mut HollywoodRender,
) {
    let field = Rect::new(
        rect.x + MARGIN,
        rect.y,
        rect.width.saturating_sub(MARGIN * 2),
        1,
    );
    if field.width < 4 {
        return;
    }
    buf.set_style(field, Style::default().bg(palette.active_row_bg));
    put_text(
        buf,
        field.x + 1,
        field.y,
        1,
        "›",
        Style::default()
            .fg(palette.accent)
            .bg(palette.active_row_bg)
            .add_modifier(Modifier::BOLD),
    );
    hits.input = field;
    let text = Rect::new(field.x + 3, field.y, field.width - 3, 1);
    if hollywood.editing {
        hits.cursor = crate::chrome::text_editor::render(
            buf,
            text,
            &hollywood.input,
            Style::default().fg(palette.text).bg(palette.active_row_bg),
        );
    } else if hollywood.input.as_str().is_empty() {
        put_text(
            buf,
            text.x,
            text.y,
            text.width,
            &truncate_end("tell hollywood what you need…", text.width as usize),
            Style::default()
                .fg(palette.overlay0)
                .bg(palette.active_row_bg)
                .add_modifier(Modifier::ITALIC),
        );
    } else {
        put_text(
            buf,
            text.x,
            text.y,
            text.width,
            &truncate_end(hollywood.input.as_str(), text.width as usize),
            Style::default().fg(palette.text).bg(palette.active_row_bg),
        );
    }
}

/// `✦ an agent can watch every studio and tell you what needs you:`
/// then `backlot hollywood setup …` — the lead in `overlay0`, the
/// command in `subtext0` (U7).
fn draw_setup(buf: &mut Buffer, rect: Rect, palette: &Palette) {
    let x = rect.x + MARGIN;
    let base = Style::default().bg(palette.surface_dim);
    let lead = format!("{SETUP_LEAD} ");
    let room = rect.width.saturating_sub(MARGIN);
    let lead_w = (lead.chars().count() as u16).min(room);
    put_text(
        buf,
        x,
        rect.y,
        lead_w,
        &truncate_end(&lead, lead_w as usize),
        base.fg(palette.overlay0),
    );
    let rest = room.saturating_sub(lead_w);
    if rest > 0 {
        put_text(
            buf,
            x + lead_w,
            rect.y,
            rest,
            &truncate_end(SETUP_COMMAND, rest as usize),
            base.fg(palette.subtext0),
        );
    }
}
