//! Waiting-item detail line painter: inline finish confirmation,
//! multi-line design decision prompts, card reasons, and prompt tails.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::MARGIN;
use super::sheet::Wait;
use super::{Confirm, Hollywood};
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::truncate_end;

/// Draw left rail (`▌` if focused/selected else `║`) and right gilt rail (`║`).
fn draw_detail_rails(
    buf: &mut Buffer,
    rect: Rect,
    y: u16,
    colour: ratatui::style::Color,
    bg: ratatui::style::Color,
    active_rail: bool,
    palette: &Palette,
) {
    let elev = palette.elevation();
    let rail = elev.gilt_style(elev.surface);
    if active_rail {
        put_text(
            buf,
            rect.x,
            y,
            1,
            "▌",
            Style::default().fg(palette.accent).bg(bg),
        );
    } else {
        put_text(buf, rect.x, y, 1, "║", rail);
    }
    if rect.width >= 2 {
        put_text(buf, rect.x + rect.width - 1, y, 1, "║", rail);
    }
    put_text(
        buf,
        rect.x + MARGIN,
        y,
        1,
        "▎",
        Style::default().fg(colour).bg(bg),
    );
}

/// A waiting row's second line: indented directly under the head verb,
/// keeping the col-2 severity bar and `└─` tree branch.
pub(super) fn draw_detail(
    buf: &mut Buffer,
    rect: Rect,
    wait: &Wait<'_>,
    hollywood: &Hollywood,
    selected: bool,
    zone_focused: bool,
    palette: &Palette,
) {
    let elev = palette.elevation();
    let bg = if selected {
        elev.raised_row
    } else {
        elev.surface
    };
    buf.set_style(rect, Style::default().bg(bg));
    let colour = super::items::glyph_colour(wait, palette);
    let active_rail = zone_focused || selected;
    draw_detail_rails(buf, rect, rect.y, colour, bg, active_rail, palette);

    let branch_x = rect.x + MARGIN + 4;
    let show_branch = rect.width >= MARGIN * 2 + 10;
    let x = branch_x + if show_branch { 3 } else { 0 };
    let width = rect
        .width
        .saturating_sub(MARGIN * 2 + 4 + if show_branch { 3 } else { 0 });

    if let Some(Confirm::Finish(key)) = hollywood.confirm.as_ref()
        && wait.target() == super::Target::Ready(key.clone())
    {
        if show_branch {
            put_text(buf, branch_x, rect.y, 2, "└─", elev.gilt_style(bg));
        }
        let pending =
            matches!(wait, Wait::Ready(item) if crate::panel::data::report_pending(item.studio));
        let text = if pending {
            "finish studio (report pending)?  y yes   n no"
        } else {
            "finish studio?  y yes   n no"
        };
        put_text(
            buf,
            x,
            rect.y,
            width,
            &truncate_end(text, width as usize),
            Style::default()
                .fg(palette.peach)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        );
        return;
    }
    if let Wait::Decision(item) = wait {
        let lines = super::decision::detail_lines(item);
        let take = lines.len().min(rect.height as usize);
        for (idx, line) in lines.into_iter().take(take).enumerate() {
            let y = rect.y + idx as u16;
            if idx > 0 {
                draw_detail_rails(buf, rect, y, colour, bg, active_rail, palette);
            }
            if show_branch {
                let b = if idx + 1 == take { "└─" } else { "├─" };
                put_text(buf, branch_x, y, 2, b, elev.gilt_style(bg));
            }
            put_text(
                buf,
                x,
                y,
                width,
                &truncate_end(&line, width as usize),
                Style::default().fg(palette.subtext0).bg(bg),
            );
        }
        return;
    }
    if let Some(text) = super::items::detail_text(wait) {
        if show_branch {
            put_text(buf, branch_x, rect.y, 2, "└─", elev.gilt_style(bg));
        }
        put_text(
            buf,
            x,
            rect.y,
            width,
            &truncate_end(&text, width as usize),
            Style::default().fg(palette.subtext0).bg(bg),
        );
        return;
    }
    let (text, colour) = match wait {
        Wait::Prompt(item) => (
            item.role
                .attention_detail
                .as_deref()
                .unwrap_or("unbound prompt — peek screen")
                .to_string(),
            palette.subtext0,
        ),
        Wait::Card(card) => (format!("why: {}", card.why), palette.subtext0),
        Wait::Outward(item) => {
            let commits = if item.card.commits_total > 0 {
                format!(
                    " · {} commit{}",
                    item.card.commits_total,
                    if item.card.commits_total == 1 {
                        ""
                    } else {
                        "s"
                    }
                )
            } else {
                String::new()
            };
            let remote = if item.card.remote_url.is_empty() {
                String::new()
            } else {
                format!(" · {}", item.card.remote_url)
            };
            (
                format!(
                    "{}{} · why: {}{}",
                    item.card.rule, commits, item.card.why, remote
                ),
                palette.subtext0,
            )
        }
        Wait::Ready(item) if crate::panel::data::report_pending(item.studio) => {
            ("report pending".to_string(), elev.muted_surface)
        }
        _ => return,
    };
    if show_branch {
        put_text(buf, branch_x, rect.y, 2, "└─", elev.gilt_style(bg));
    }
    put_text(
        buf,
        x,
        rect.y,
        width,
        &truncate_end(&text, width as usize),
        Style::default().fg(colour).bg(bg),
    );
}
