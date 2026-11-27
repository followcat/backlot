//! Individual studio frame painter inside the Call Sheet's STUDIOS grid
//! at 7 rows (full), 5 rows (compact: drops role badge and next step),
//! or 3 rows (micro: border + state · round · done/total + border).

use backlot_model::summary::StudioSummary;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::rows::on_set;
use super::sheet::Conn;
use crate::chrome::palette::Palette;
use crate::chrome::render::{put_role_badged_text, put_text};
use crate::chrome::text::truncate_end;
use crate::panel::data::{StudioLabel, StudioRef};

fn on(fg: Color, bg: Color) -> Style {
    Style::default().fg(fg).bg(bg)
}

/// Draw one studio frame (`7`, `5`, or `3` rows tall).
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_studio(
    buf: &mut Buffer,
    rect: Rect,
    studio: StudioRef<'_>,
    label: &StudioLabel,
    selected: bool,
    active: bool,
    status: &super::sheet::Status<'_>,
    palette: &Palette,
) {
    if rect.width < 4 || rect.height < 3 {
        return;
    }
    let elev = palette.elevation();
    let border = if selected { palette.accent } else { elev.gilt };
    let bg = if selected {
        elev.raised_row
    } else {
        elev.stage_well
    };
    for y in rect.y + 1..rect.y + rect.height - 1 {
        for x in rect.x + 1..rect.x + rect.width - 1 {
            buf[(x, y)].set_symbol(" ");
            buf[(x, y)].set_style(on(border, bg));
        }
    }
    draw_chrome(buf, rect, label, selected, active, palette);

    let inner = rect.width.saturating_sub(4);
    draw_state_row(buf, rect, inner, studio, status, bg, palette);
    if rect.height >= 7 {
        draw_who_row(buf, rect, rect.y + 2, inner, studio.studio, bg, palette);
        draw_what_row(buf, rect, rect.y + 3, inner, studio.studio, bg, palette);
        if !studio.studio.needs_upgrade
            && let Some(step) = crate::panel::data::next_step(studio.studio)
        {
            let text = format!("next: {step}");
            put_text(
                buf,
                rect.x + 2,
                rect.y + 4,
                inner,
                &format!(" {}", truncate_end(&text, inner.saturating_sub(1) as usize)),
                on(elev.muted_surface, bg),
            );
        }
        draw_cast_row(buf, rect, rect.y + 5, studio.studio, bg, palette);
    } else if rect.height >= 5 {
        draw_what_row(buf, rect, rect.y + 2, inner, studio.studio, bg, palette);
        draw_cast_row(buf, rect, rect.y + 3, studio.studio, bg, palette);
    }
}

fn draw_chrome(
    buf: &mut Buffer,
    rect: Rect,
    label: &StudioLabel,
    selected: bool,
    active: bool,
    palette: &Palette,
) {
    let elev = palette.elevation();
    let (tl, h, tr, v, bl, br, edge) = if selected {
        (
            "╔",
            "═",
            "╗",
            "║",
            "╚",
            "╝",
            on(palette.accent, elev.surface),
        )
    } else {
        ("╭", "─", "╮", "│", "╰", "╯", elev.gilt_style(elev.surface))
    };
    put_text(buf, rect.x, rect.y, 1, tl, edge);
    put_text(buf, rect.x + 1, rect.y, 1, h, edge);
    let prefix = if active { "▸ " } else { "" };
    let budget = rect.width.saturating_sub(6 + prefix.chars().count() as u16) as usize;
    let id = label.fit(budget);
    let title = format!(" {prefix}{id} ");
    let title_style = on(
        if selected {
            palette.accent
        } else {
            palette.text
        },
        elev.surface,
    )
    .add_modifier(Modifier::BOLD);
    let title_w = title.chars().count() as u16;
    let used = 2 + title_w;
    put_text(buf, rect.x + 2, rect.y, title_w, &title, title_style);
    if active && used >= 5 {
        put_text(
            buf,
            rect.x + 3,
            rect.y,
            1,
            "▸",
            on(palette.accent, elev.surface).add_modifier(Modifier::BOLD),
        );
    }
    if used + 1 < rect.width {
        let fill = rect.width - 1 - used;
        put_text(
            buf,
            rect.x + used,
            rect.y,
            fill,
            &h.repeat(fill as usize),
            edge,
        );
    }
    put_text(buf, rect.x + rect.width - 1, rect.y, 1, tr, edge);
    for y in rect.y + 1..rect.y + rect.height - 1 {
        put_text(buf, rect.x, y, 1, v, edge);
        put_text(buf, rect.x + rect.width - 1, y, 1, v, edge);
    }
    let bottom = rect.y + rect.height - 1;
    put_text(buf, rect.x, bottom, 1, bl, edge);
    put_text(
        buf,
        rect.x + 1,
        bottom,
        rect.width - 2,
        &h.repeat((rect.width - 2) as usize),
        edge,
    );
    put_text(buf, rect.x + rect.width - 1, bottom, 1, br, edge);
}

fn draw_state_row(
    buf: &mut Buffer,
    rect: Rect,
    inner: u16,
    studio: StudioRef<'_>,
    status: &super::sheet::Status<'_>,
    bg: Color,
    palette: &Palette,
) {
    let (conn, stage) = status(studio.machine);
    let room = inner.saturating_sub(2) as usize;
    let (mark, word, colour) = studio_line(studio.studio, conn, stage, room, palette);
    let y = rect.y + 1;
    put_text(buf, rect.x + 2, y, 1, mark, on(colour, bg));
    put_text(
        buf,
        rect.x + 3,
        y,
        inner.saturating_sub(1),
        &format!(" {}", truncate_end(word, inner.saturating_sub(2) as usize)),
        on(colour, bg).add_modifier(Modifier::BOLD),
    );
    let rev = format!(
        "r{} · {}/{}",
        studio.studio.requirement_version, studio.studio.tasks_done, studio.studio.tasks_total
    );
    let mut right_w = rev.chars().count() as u16;
    let waiting = crate::studio::waiting_count(studio.studio);
    let chip = if waiting > 0 {
        format!(" !{waiting} ")
    } else {
        String::new()
    };
    right_w += chip.chars().count() as u16 + u16::from(!chip.is_empty());
    if inner > right_w + 8 {
        let mut rx = rect.x + rect.width - 2 - right_w;
        let rev_w = rev.chars().count() as u16;
        put_text(buf, rx, y, rev_w, &rev, on(palette.subtext0, bg));
        rx += rev_w;
        if !chip.is_empty() {
            rx += 1;
            let user = crate::studio::waiting_is_user(studio.studio);
            put_text(
                buf,
                rx,
                y,
                chip.chars().count() as u16,
                &chip,
                on(
                    crate::studio::glyphs::contrast_fg(palette),
                    if user { palette.red } else { palette.peach },
                )
                .add_modifier(Modifier::BOLD),
            );
        }
    }
}

fn draw_who_row(
    buf: &mut Buffer,
    rect: Rect,
    y: u16,
    inner: u16,
    studio: &StudioSummary,
    bg: Color,
    palette: &Palette,
) {
    let elev = palette.elevation();
    let who = on_set(studio)
        .map(crate::studio::glyphs::role_label)
        .unwrap_or_else(|| "—".to_string());
    put_role_badged_text(
        buf,
        rect.x + 2,
        y,
        inner,
        &format!(" {}", truncate_end(&who, inner.saturating_sub(1) as usize)),
        on(elev.muted_surface, bg),
        palette,
    );
}

fn draw_what_row(
    buf: &mut Buffer,
    rect: Rect,
    y: u16,
    inner: u16,
    studio: &StudioSummary,
    bg: Color,
    palette: &Palette,
) {
    let (what, colour) = match studio.error.as_deref() {
        Some(error) => (error, palette.red),
        None if studio.needs_upgrade => ("upgrade backlot", palette.peach),
        None => (
            on_set(studio)
                .and_then(|r| {
                    r.doing
                        .as_deref()
                        .or_else(|| r.running.as_ref().map(|run| run.cmd.as_str()))
                        .or_else(|| r.progress.as_ref().map(|p| p.text.as_str()))
                })
                .or(studio.next.as_deref())
                .unwrap_or("nothing on set"),
            palette.subtext0,
        ),
    };
    put_text(
        buf,
        rect.x + 2,
        y,
        inner,
        &format!(" {}", truncate_end(what, inner.saturating_sub(1) as usize)),
        on(colour, bg),
    );
}

fn draw_cast_row(
    buf: &mut Buffer,
    rect: Rect,
    y: u16,
    studio: &StudioSummary,
    bg: Color,
    palette: &Palette,
) {
    let elev = palette.elevation();
    let live_roles = studio
        .roles
        .iter()
        .filter(|role| role.state != "standby")
        .count();
    let cast_label = if studio.open_reviews > 0 {
        format!("◈ {} rev", studio.open_reviews)
    } else if studio.actors_cap > 0 {
        format!("actors {}/{}", studio.actors_live, studio.actors_cap)
    } else {
        format!("{live_roles} cast")
    };
    let label_w = cast_label.chars().count() as u16;
    let label_x = (rect.x + rect.width - 2).saturating_sub(label_w);
    if label_x > rect.x + 2 {
        put_text(
            buf,
            label_x,
            y,
            label_w,
            &cast_label,
            on(
                if studio.open_reviews > 0 {
                    palette.blue
                } else {
                    elev.muted_surface
                },
                bg,
            ),
        );
    }
    let mut x = rect.x + 2;
    for role in &studio.roles {
        if role.state == "standby" {
            continue;
        }
        if x + 1 >= label_x {
            break;
        }
        let mark = crate::studio::glyphs::Person::of(role);
        put_text(buf, x, y, 1, mark.still(), on(mark.color(palette), bg));
        x += 2;
    }
}

/// The frame's state line, machine-aware (A8.2): `× error`, then
/// `× offline`, `◌ stage stopped`, `✓ ready`, `✓ done`, `● live` —
/// `(mark, word, colour)`. `room` is the width the word gets; the
/// stopped-stage line names its `s start` key only when it fits.
pub(crate) fn studio_line<'a>(
    studio: &'a StudioSummary,
    conn: Conn,
    stage: bool,
    room: usize,
    palette: &Palette,
) -> (&'static str, &'a str, Color) {
    if studio.error.is_some() {
        return ("×", "error", palette.red);
    }
    if studio.needs_upgrade {
        return ("!", "upgrade", palette.peach);
    }
    if conn == Conn::Offline {
        return ("×", "offline", palette.red);
    }
    if !stage {
        let word = if room >= "stage stopped · s start".len() {
            "stage stopped · s start"
        } else {
            "stage stopped"
        };
        return ("◌", word, palette.peach);
    }
    if studio.ready_to_finish {
        return ("✓", "ready", palette.teal);
    }
    match studio.state.as_str() {
        "finished" => ("✓", "done", palette.overlay1),
        "live" => ("●", "live", palette.green),
        other => (
            "·",
            if other.is_empty() { "down" } else { other },
            palette.overlay0,
        ),
    }
}
