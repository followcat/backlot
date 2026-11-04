//! Pure ratatui drawing for the sidebar model. Two layouts: expanded
//! (80+ columns) reserves a real pane area beside the list; compact
//! collapses to a single status line. Nothing mutates model state
//! here, and every string arrived pre-sanitized from the model.

use super::{Row, RowId, Sidebar, Tone, ViewMode};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, List, ListItem, ListState, Paragraph, StatefulWidget, Widget,
};
use unicode_width::UnicodeWidthStr;

/// Below this width the sidebar collapses unless the user forced it.
pub const EXPAND_MIN: u16 = 80;

/// How the screen splits for the current layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Place {
    /// Expanded: sidebar on the left, pane area beside it.
    Expanded {
        /// Sidebar column.
        sidebar: Rect,
        /// Reserved pane area.
        pane: Rect,
    },
    /// Compact: one status line on top, pane under it.
    Compact {
        /// The status line.
        line: Rect,
        /// Reserved pane area.
        pane: Rect,
    },
}

/// Split `area` for the sidebar's effective layout. Expanded keeps a
/// usable pane (a quarter of the width, bounded 24..40 columns); the
/// pane area is never narrower than a cell on any sane width.
pub fn place(sidebar: &Sidebar, area: Rect) -> Place {
    if sidebar.expanded(area.width) {
        let want = (area.width / 4).clamp(24, 40);
        let sw = want.min(area.width.saturating_sub(4));
        let [sidebar, pane] =
            Layout::horizontal([Constraint::Length(sw), Constraint::Min(0)]).areas(area);
        Place::Expanded { sidebar, pane }
    } else {
        let [line, pane] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
        Place::Compact { line, pane }
    }
}

fn tone_style(tone: Tone) -> Style {
    match tone {
        Tone::Heading => Style::default().add_modifier(Modifier::BOLD),
        Tone::Offline => Style::default().fg(Color::DarkGray),
        Tone::Blocked => Style::default()
            .fg(Color::LightYellow)
            .add_modifier(Modifier::BOLD),
        Tone::Error => Style::default().fg(Color::LightRed),
        Tone::Lost => Style::default().fg(Color::Red),
        Tone::Working => Style::default().fg(Color::Cyan),
        Tone::Done => Style::default().fg(Color::Green),
        Tone::Quiet => Style::default(),
        Tone::Muted => Style::default().fg(Color::DarkGray),
    }
}

/// One row as a `Line`: selection marker + indent + label + note on
/// the left, the badge right-aligned. Whatever overflows is truncated
/// by display width on grapheme boundaries; the badge is kept last.
fn row_line(row: &Row, selected: bool, width: usize) -> Line<'static> {
    if width == 0 {
        return Line::default();
    }
    let dim = Style::default().fg(Color::DarkGray);
    let (text_style, badge_style) = if row.dim {
        (dim, dim)
    } else {
        let text = match row.tone {
            Tone::Heading => Style::default().add_modifier(Modifier::BOLD),
            _ => Style::default(),
        };
        (text, tone_style(row.tone))
    };
    let marker = if selected { ">" } else { " " };
    let indent = "  ".repeat(row.depth as usize);
    let badge_w = UnicodeWidthStr::width(row.badge.as_str());
    let left = format!("{marker}{indent}{}", row.text);
    let left_w = UnicodeWidthStr::width(left.as_str());
    // The note gets only what the label leaves; the badge is reserved.
    let note_room = width.saturating_sub(left_w + badge_w + 1);
    let note = if note_room >= 2 {
        super::text::fit(&format!(" {}", row.note), note_room)
    } else {
        String::new()
    };
    let note_w = UnicodeWidthStr::width(note.as_str());

    if left_w + note_w + badge_w < width {
        let pad = " ".repeat(width - left_w - note_w - badge_w);
        Line::from(vec![
            Span::styled(left, text_style),
            Span::styled(note, dim),
            Span::raw(pad),
            Span::styled(row.badge.clone(), badge_style),
        ])
    } else {
        // Still over: identity and badge share a bounded budget — the
        // left text is reserved up to 12 cells (name included), so the
        // badge keeps at least the rest. When a terminal's address
        // itself cannot fit what the badge leaves, it keeps its END
        // (`…/t7`, `…/t8` under one long machine alias stay
        // distinguishable); the badge leads with the word, so its
        // clipped tail only loses detail.
        let is_term = matches!(row.id, RowId::Terminal(_));
        let head = format!("{marker}{indent}");
        let head_w = UnicodeWidthStr::width(head.as_str());
        let addr = row.text.split(' ').next().unwrap_or(&row.text);
        let id_w = head_w + UnicodeWidthStr::width(addr);
        let reserved = left_w.min(12).min(width.saturating_sub(1));
        let badge_fit = super::text::fit(&row.badge, width.saturating_sub(reserved + 1));
        let badge_fit_w = UnicodeWidthStr::width(badge_fit.as_str());
        let avail = width.saturating_sub(badge_fit_w + 1);
        let left_fit = if is_term && avail <= id_w {
            if avail <= head_w {
                super::text::fit(&left, avail)
            } else if avail == id_w {
                // Exact fit: drop the name rather than let a trailing
                // `…` eat the address.
                format!("{head}{addr}")
            } else {
                format!("{head}{}", super::text::fit_tail(addr, avail - head_w))
            }
        } else {
            super::text::fit(&left, avail)
        };
        let left_fit_w = UnicodeWidthStr::width(left_fit.as_str());
        let pad = " ".repeat(width.saturating_sub(left_fit_w + badge_fit_w));
        Line::from(vec![
            Span::styled(left_fit, text_style),
            Span::raw(pad),
            Span::styled(badge_fit, badge_style),
        ])
    }
}

fn draw_expanded(sb: &Sidebar, area: Rect, buf: &mut Buffer) {
    let block = Block::default().borders(Borders::RIGHT).title(" hc-tui ");
    let inner = block.inner(area);
    block.render(area, buf);
    if inner.is_empty() {
        return;
    }

    // Header: view tabs, then counters — dropped first at tiny heights.
    let header_rows = if inner.height >= 4 { 2 } else { 0 };
    if header_rows > 0 {
        let tabs = Line::from(vec![
            Span::styled(
                " spaces ",
                if sb.mode() == ViewMode::Spaces {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                },
            ),
            Span::raw(" "),
            Span::styled(
                " agents ",
                if sb.mode() == ViewMode::Agents {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                },
            ),
        ]);
        let c = sb.counters();
        let counters = Line::from(Span::styled(
            format!(
                "{} turn · {} work · {} ready",
                c.your_turn, c.working, c.ready
            ),
            Style::default().fg(Color::DarkGray),
        ));
        Paragraph::new(vec![tabs, counters])
            .render(Rect::new(inner.x, inner.y, inner.width, header_rows), buf);
    }

    let list_area = Rect::new(
        inner.x,
        inner.y + header_rows,
        inner.width,
        inner.height - header_rows,
    );
    if list_area.is_empty() {
        return;
    }
    let w = list_area.width as usize;
    let selected = sb.selected();
    let items: Vec<ListItem> = sb
        .rows()
        .iter()
        .map(|r| ListItem::new(row_line(r, selected == Some(&r.id), w)))
        .collect();
    let mut state = ListState::default().with_selected(sb.selected_index());
    let list = List::new(items).highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    StatefulWidget::render(list, list_area, buf, &mut state);
}

fn draw_compact(sb: &Sidebar, area: Rect, buf: &mut Buffer) {
    if area.is_empty() {
        return;
    }
    let c = sb.counters();
    let mode = match sb.mode() {
        ViewMode::Spaces => "spaces",
        ViewMode::Agents => "agents",
    };
    let offline = match sb.summary().offline {
        0 => String::new(),
        n => format!(" · {n} offline"),
    };
    let line = format!(
        " {mode} │ {} turn · {} work · {} ready{offline} ",
        c.your_turn, c.working, c.ready
    );
    Paragraph::new(super::text::fit(&line, area.width as usize)).render(area, buf);
}

/// The pane placeholder, filled by the M2 runtime task.
fn draw_pane(area: Rect, buf: &mut Buffer) {
    if area.is_empty() {
        return;
    }
    Block::bordered().title(" terminal ").render(area, buf);
}

/// Draw the sidebar and the reserved pane area into `buf`.
pub fn draw(sidebar: &Sidebar, area: Rect, buf: &mut Buffer) {
    match place(sidebar, area) {
        Place::Expanded {
            sidebar: side,
            pane,
        } => {
            draw_expanded(sidebar, side, buf);
            draw_pane(pane, buf);
        }
        Place::Compact { line, pane } => {
            draw_compact(sidebar, line, buf);
            draw_pane(pane, buf);
        }
    }
}
