//! The lifecycle dialogs' chrome (W4b): restart's two choices and the
//! recast picker's step list. The archived list lives in `archived.rs`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Clear, Widget};

use super::super::dialogs::{panel, row};
use super::super::palette::Palette;
use super::super::render::put_text;
use super::super::text::truncate_end;
use super::{DialogHits, cancel_button, centered, dialog_width, dim_line, send_button, title_line};
use crate::new_studio::Probe;
use crate::studio_dialog::{CastEditDialog, CastEditStep, RecastDialog, RecastStep, RestartDialog};

/// The restart dialog — both choices always, the unavailable one dim.
pub(super) fn render_restart(
    b: &mut Buffer,
    band: Rect,
    d: &RestartDialog,
    palette: &Palette,
) -> Option<DialogHits> {
    let width = dialog_width(band.width);
    let q = centered(band, width, 9)?;
    Clear.render(q, b);
    let i = panel(b, q, palette.accent, palette.panel_bg)?;
    title_line(b, i, &d.title, palette);
    dim_line(
        b,
        i.x,
        i.y + 1,
        i.width,
        " Reload the seat in place; the stage replaces its terminal.",
        palette,
    );
    for (n, choice) in d.choices().iter().enumerate() {
        let y = i.y + 2 + n as u16;
        let selected = n == d.selected;
        let style = match (&choice.dim, selected) {
            (Some(_), _) => Style::default().fg(palette.overlay0).bg(palette.panel_bg),
            (None, true) => Style::default()
                .fg(palette.text)
                .bg(palette.surface0)
                .add_modifier(Modifier::BOLD),
            (None, false) => Style::default().fg(palette.subtext0).bg(palette.panel_bg),
        };
        if selected {
            b.set_style(Rect::new(i.x, y, i.width, 1), style);
        }
        let mark = if selected { "▸ " } else { "  " };
        let label = match &choice.dim {
            Some(reason) => format!("{mark}{}  ({reason})", choice.label),
            None => format!("{mark}{}", choice.label),
        };
        put_text(
            b,
            i.x,
            y,
            i.width,
            &truncate_end(&label, i.width as usize),
            style,
        );
    }
    let rs = row(i, &[13, 12], 2, i.height.saturating_sub(2));
    let [ok, cancel] = rs.as_slice() else {
        return None;
    };
    send_button(b, *ok, " ↵ restart ", true, palette);
    cancel_button(b, *cancel, palette);
    Some(DialogHits {
        area: q,
        send: *ok,
        cancel: *cancel,
        body: Rect::default(),
        summary: Rect::default(),
        cursor: None,
    })
}

/// The recast picker — one step at a time, the summary before it runs.
pub(super) fn render_recast(
    b: &mut Buffer,
    band: Rect,
    d: &RecastDialog,
    palette: &Palette,
) -> Option<DialogHits> {
    let width = dialog_width(band.width);
    let q = centered(band, width, 16)?;
    Clear.render(q, b);
    let i = panel(b, q, palette.accent, palette.panel_bg)?;
    title_line(b, i, &d.title, palette);
    let step_label = match d.step {
        RecastStep::Runner => "1/3 runner",
        RecastStep::Model => "2/3 model",
        RecastStep::Effort => "3/3 effort",
        RecastStep::Apply => "apply",
    };
    dim_line(b, i.x, i.y + 1, i.width, &format!(" {step_label}"), palette);
    let list_y = i.y + 2;
    let list_rows = i.height.saturating_sub(7) as usize;
    match d.step {
        RecastStep::Runner => match &d.runners {
            Probe::Pending => dim_line(
                b,
                i.x,
                list_y,
                i.width,
                " loading the machine's runners…",
                palette,
            ),
            Probe::Failed(error) => dim_line(b, i.x, list_y, i.width, error, palette),
            Probe::Done(_) => {
                let found = d.found();
                if found.is_empty() {
                    dim_line(
                        b,
                        i.x,
                        list_y,
                        i.width,
                        " no runner on this machine can seat a role",
                        palette,
                    );
                }
                for (n, runner) in found.iter().take(list_rows).enumerate() {
                    let y = list_y + n as u16;
                    let selected = n == d.runner_sel;
                    let text = match &runner.version {
                        Some(version) => format!("{}  {version}", runner.runner),
                        None => runner.runner.clone(),
                    };
                    list_line(b, i, y, selected, &text, palette);
                }
            }
        },
        RecastStep::Model => {
            let models = d.models();
            for (n, model) in models.iter().take(list_rows).enumerate() {
                let y = list_y + n as u16;
                let selected = n == d.model_sel;
                list_line(b, i, y, selected, model, palette);
            }
            if models.is_empty() {
                dim_line(
                    b,
                    i.x,
                    list_y,
                    i.width,
                    " this runner lists no model",
                    palette,
                );
            }
        }
        RecastStep::Effort => {
            let efforts = d.efforts();
            for (n, effort) in efforts.iter().take(list_rows).enumerate() {
                let y = list_y + n as u16;
                let selected = n == d.effort_sel;
                list_line(b, i, y, selected, &effort.label, palette);
            }
        }
        RecastStep::Apply => {
            dim_line(
                b,
                i.x,
                list_y,
                i.width,
                " Enter applies the new identity.",
                palette,
            );
        }
    }
    let summary_y = i.bottom().saturating_sub(3);
    put_text(
        b,
        i.x,
        summary_y,
        i.width,
        &truncate_end(&format!(" {}", d.summary()), i.width as usize),
        Style::default()
            .fg(palette.accent)
            .bg(palette.panel_bg)
            .add_modifier(Modifier::BOLD),
    );
    if let Some(error) = &d.error {
        dim_line(
            b,
            i.x,
            summary_y + 1,
            i.width,
            &format!(" ! {error}"),
            palette,
        );
    }
    let rs = row(i, &[12, 12], 2, i.height.saturating_sub(1));
    let [ok, cancel] = rs.as_slice() else {
        return None;
    };
    let label = if d.step == RecastStep::Apply {
        " ↵ apply "
    } else {
        " ↵ next "
    };
    send_button(b, *ok, label, true, palette);
    cancel_button(b, *cancel, palette);
    Some(DialogHits {
        area: q,
        send: *ok,
        cancel: *cancel,
        body: Rect::default(),
        summary: Rect::default(),
        cursor: None,
    })
}

/// One picker list row — the selection background and `▸`.
fn list_line(b: &mut Buffer, i: Rect, y: u16, selected: bool, text: &str, palette: &Palette) {
    let style = if selected {
        Style::default()
            .fg(palette.text)
            .bg(palette.surface0)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(palette.subtext0).bg(palette.panel_bg)
    };
    if selected {
        b.set_style(Rect::new(i.x, y, i.width, 1), style);
    }
    let mark = if selected { "▸ " } else { "  " };
    put_text(
        b,
        i.x,
        y,
        i.width,
        &truncate_end(&format!("{mark}{text}"), i.width as usize),
        style,
    );
}

/// The `Edit cast…` dialog: while fetching or checking it says what it
/// is doing; on a diff it shows the changes and offers apply; a refusal
/// shows its reason and offers only cancel.
pub(super) fn render_cast_edit(
    b: &mut Buffer,
    band: Rect,
    d: &CastEditDialog,
    palette: &Palette,
) -> Option<DialogHits> {
    let width = dialog_width(band.width);
    let q = centered(band, width, 16)?;
    Clear.render(q, b);
    let i = panel(b, q, palette.accent, palette.panel_bg)?;
    title_line(b, i, &d.title, palette);
    let step = match d.step {
        CastEditStep::Loading => "reading the cast…",
        CastEditStep::Editing => "open in your editor — save and close to check it",
        CastEditStep::Checking => "checking the change…",
        CastEditStep::Diff => "the change",
    };
    dim_line(b, i.x, i.y + 1, i.width, &format!(" {step}"), palette);
    // The body: an error wins, else the diff. Wrapped to the panel width
    // and clipped to the rows the panel leaves above the buttons.
    let inner = i.width.saturating_sub(2) as usize;
    let body = match &d.error {
        Some(err) => err.clone(),
        None if d.step == CastEditStep::Diff && d.diff.trim().is_empty() => {
            "nothing changes".to_owned()
        }
        None => d.diff.clone(),
    };
    let mut lines: Vec<String> = Vec::new();
    for raw in body.lines() {
        if raw.is_empty() {
            lines.push(String::new());
            continue;
        }
        // A diff line already fits a full-width panel on a normal
        // terminal; an over-long one is truncated, never wrapped, so a
        // change stays on its own row.
        lines.push(truncate_end(raw, inner));
    }
    let top = i.y + 2;
    let room = i.height.saturating_sub(5) as usize;
    for (n, line) in lines.iter().take(room).enumerate() {
        put_text(
            b,
            i.x,
            top + n as u16,
            i.width,
            &format!(" {line}"),
            Style::default().fg(palette.text).bg(palette.panel_bg),
        );
    }
    let rs = row(i, &[19, 13], 2, i.height.saturating_sub(2));
    let [send, cancel] = rs.as_slice() else {
        return None;
    };
    let apply = d.step == CastEditStep::Diff && d.ready && !d.busy;
    send_button(b, *send, " ↵ / y apply ", apply, palette);
    cancel_button(b, *cancel, palette);
    Some(DialogHits {
        area: q,
        send: *send,
        cancel: *cancel,
        body: Rect::default(),
        summary: Rect::default(),
        cursor: None,
    })
}
