// Derived from herdr src/client/shell/overlays.rs
// (`render_navigator_overlay`) (Apache-2.0).
//! The "Go to" overlay's renderer: centered popup, search header, row
//! list with a scrollbar, selected-row footer and key help. Reports its
//! hit rectangles through `NavigatorRender` like the other overlays.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};

use super::{
    endpoints::endpoint_status_presentation,
    help::CursorState,
    navigator::{
        NavigatorFilter, NavigatorState, NavigatorTarget, navigator_rows, navigator_selected_index,
    },
    palette::Palette,
    render::{display_width, put_right_text, put_text},
    scroll::{self, ScrollMetrics},
    snapshot::{ClientEndpointId, ClientEndpointStatus, ClientShellEndpoint},
    status::{AgentStatus, panel_contrast_fg, status_color, status_icon, status_text},
    text_editor,
};

#[derive(Default)]
pub(crate) struct NavigatorRender {
    pub area: Rect,
    pub search: Rect,
    pub rows: Vec<(Rect, NavigatorTarget)>,
    pub scrollbar: Rect,
    pub metrics: Option<ScrollMetrics>,
    pub cursor: Option<CursorState>,
}

fn status_dot(status: AgentStatus) -> &'static str {
    status_icon(status, crate::config::StatusIndicatorStyle::Dots)
}

pub(crate) fn render_navigator_overlay(
    b: &mut Buffer,
    n: &NavigatorState,
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    p: &Palette,
) -> Option<NavigatorRender> {
    let a = b.area;
    let width = a.width.saturating_sub(4).min(116);
    let height = a.height.saturating_sub(2).min(42);
    if width < 4 || height < 9 {
        return None;
    }
    let q = Rect::new(
        a.x + (a.width - width) / 2,
        a.y + (a.height - height) / 2,
        width,
        height,
    )
    .intersection(a);
    let i = super::dialogs::panel(b, q, p.accent, p.panel_bg)?;
    put_text(
        b,
        q.x + 2,
        q.y,
        q.width.saturating_sub(4),
        " Go to ",
        Style::default().fg(p.accent).bg(p.panel_bg),
    );
    let rows = navigator_rows(endpoints, active_endpoint_id, n);
    let search = if n.search_focused {
        " / ".to_owned()
    } else if let Some(f) = n.filter {
        format!(
            " / {}",
            match f {
                NavigatorFilter::Blocked => "blocked",
                NavigatorFilter::Working => "working",
                NavigatorFilter::Idle => "idle",
                NavigatorFilter::Done => "done",
            }
        )
    } else if n.query.is_empty() {
        " / search agents and terminals".to_owned()
    } else {
        format!(" / {}", n.query)
    };
    let terminal_count = rows
        .iter()
        .filter(|row| matches!(row.target, NavigatorTarget::Pane { .. }))
        .count();
    let count = format!(
        "{terminal_count} {}",
        if terminal_count == 1 {
            "terminal"
        } else {
            "terminals"
        }
    );
    put_text(
        b,
        i.x,
        i.y,
        i.width.saturating_sub(display_width(&count) + 1),
        &search,
        Style::default()
            .fg(if n.search_focused { p.text } else { p.overlay0 })
            .bg(p.panel_bg),
    );
    let cursor = if n.search_focused {
        text_editor::render(
            b,
            Rect::new(
                i.x + 3,
                i.y,
                i.width.saturating_sub(4 + display_width(&count)),
                1,
            ),
            &n.query,
            Style::default().fg(p.text).bg(p.panel_bg),
        )
    } else {
        None
    };
    put_right_text(
        b,
        i,
        i.y,
        &count,
        Style::default().fg(p.overlay0).bg(p.panel_bg),
    );
    put_text(
        b,
        i.x,
        i.y + 1,
        i.width,
        &"─".repeat(i.width as usize),
        Style::default().fg(p.surface1).bg(p.panel_bg),
    );
    let body = Rect::new(i.x, i.y + 2, i.width, i.height.saturating_sub(5));
    let selected = navigator_selected_index(&rows, n).unwrap_or(0);
    let max = rows.len().saturating_sub(body.height as usize);
    let scroll = n
        .scroll
        .max(selected.saturating_sub(body.height.saturating_sub(1) as usize))
        .min(selected)
        .min(max);
    let metrics = ScrollMetrics {
        offset_from_bottom: max.saturating_sub(scroll),
        max_offset_from_bottom: max,
        viewport_rows: usize::from(body.height),
    };
    let scrollbar =
        (max > 0 && body.width > 1).then_some(Rect::new(body.right() - 1, body.y, 1, body.height));
    let row_width = body.width.saturating_sub(u16::from(scrollbar.is_some()));
    let mut row_hits = Vec::new();
    if rows.is_empty() {
        put_text(
            b,
            body.x,
            body.y,
            body.width,
            " No matching agents or terminals",
            Style::default().fg(p.overlay0).bg(p.panel_bg),
        );
    }
    for (ix, r) in rows
        .iter()
        .enumerate()
        .skip(scroll)
        .take(body.height as usize)
    {
        let rect = Rect::new(body.x, body.y + (ix - scroll) as u16, row_width, 1);
        row_hits.push((rect, r.target.clone()));
        let st = if r.stale {
            Style::default()
                .fg(p.overlay0)
                .bg(if ix == selected {
                    p.surface0
                } else {
                    p.panel_bg
                })
                .add_modifier(Modifier::DIM)
        } else if ix == selected {
            Style::default()
                .fg(panel_contrast_fg(p))
                .bg(p.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(if matches!(r.target, NavigatorTarget::Machine { .. }) {
                    p.subtext0
                } else {
                    p.text
                })
                .bg(p.panel_bg)
        };
        let is_pane = matches!(r.target, NavigatorTarget::Pane { .. });
        let connector = if !is_pane {
            ""
        } else if rows
            .get(ix + 1)
            .is_some_and(|next| matches!(next.target, NavigatorTarget::Pane { .. }))
        {
            "├─ "
        } else {
            "└─ "
        };
        let padding = u16::from(r.depth.saturating_sub(u8::from(is_pane))) * 2 + 1;
        let connector_x = rect.x + padding;
        let indent = format!("{:width$}{connector}", "", width = usize::from(padding));
        let current = if r.current { "◆ " } else { "" };
        let status = r.status.map(status_dot).unwrap_or_default();
        let status_separator = if status.is_empty() { "" } else { " " };
        let label = format!("{indent}{current}{status}{status_separator}{}", r.label);
        let st = if r.status.is_none() {
            st.add_modifier(Modifier::BOLD)
        } else {
            st
        };
        b.set_style(rect, st);
        let columns = if r.status.is_some() {
            if rect.width >= 64 {
                24
            } else if rect.width >= 36 {
                12
            } else {
                0
            }
        } else {
            0
        };
        put_text(
            b,
            rect.x,
            rect.y,
            rect.width.saturating_sub(columns),
            &label,
            st,
        );
        if is_pane {
            put_text(
                b,
                connector_x,
                rect.y,
                rect.right().saturating_sub(connector_x).min(2),
                connector,
                if r.stale || ix == selected {
                    st
                } else {
                    st.fg(p.overlay0)
                },
            );
        }
        if let Some(status) = r.status {
            let prefix = format!("{indent}{current}");
            let status_style = if r.stale || ix == selected {
                st
            } else {
                Style::default().fg(status_color(status, p)).bg(p.panel_bg)
            };
            put_text(
                b,
                rect.x.saturating_add(display_width(&prefix)),
                rect.y,
                display_width(status_dot(status)),
                status_dot(status),
                status_style,
            );
            let meta_style = if r.stale || ix == selected {
                st
            } else {
                st.fg(p.overlay0)
            };
            if columns > 0 {
                put_text(
                    b,
                    rect.right() - columns + 1,
                    rect.y,
                    11,
                    r.agent.as_deref().unwrap_or("terminal"),
                    meta_style,
                );
            }
            if columns == 24 {
                put_text(
                    b,
                    rect.right() - 11,
                    rect.y,
                    11,
                    if r.agent.is_some() {
                        status_text(status)
                    } else {
                        "shell"
                    },
                    meta_style,
                );
            }
        }
        // Right-hand decorations: a remote machine's status signal, or
        // a heading row's metadata tail.
        let machine_status = match &r.target {
            NavigatorTarget::Machine { endpoint_id } if !endpoint_id.is_local() => endpoints
                .iter()
                .find(|endpoint| &endpoint.endpoint_id == endpoint_id)
                .map(|endpoint| endpoint.status),
            _ => None,
        };
        if let Some(status) = machine_status {
            let (glyph, state, color) = endpoint_status_presentation(status, p);
            let signal = if status == ClientEndpointStatus::Online {
                glyph.to_owned()
            } else {
                format!("{glyph} {state}")
            };
            let signal_style = if ix == selected {
                st
            } else {
                Style::default()
                    .fg(color)
                    .bg(p.panel_bg)
                    .add_modifier(if r.stale {
                        Modifier::DIM
                    } else {
                        Modifier::empty()
                    })
            };
            put_right_text(b, rect, rect.y, &signal, signal_style);
        } else if r.status.is_none() && !r.meta.is_empty() {
            let label_width = display_width(&label).min(rect.width);
            let meta = Rect::new(
                rect.x.saturating_add(label_width).saturating_add(1),
                rect.y,
                rect.width.saturating_sub(label_width.saturating_add(1)),
                1,
            );
            put_right_text(b, meta, rect.y, &r.meta, st)
        }
    }
    if let Some(track) = scrollbar {
        scroll::render_scrollbar_buffer(b, metrics, track, p.overlay0, p.overlay1, "▐");
    }
    if let Some(r) = rows.get(selected) {
        put_text(
            b,
            i.x,
            i.bottom() - 3,
            i.width,
            &format!(" {}", r.detail),
            Style::default().fg(p.subtext0).bg(p.panel_bg),
        );
        put_text(
            b,
            i.x,
            i.bottom() - 2,
            i.width,
            &format!(" {}", r.meta),
            Style::default().fg(p.overlay0).bg(p.panel_bg),
        );
    }
    put_text(
        b,
        i.x,
        i.bottom() - 1,
        i.width,
        if n.search_focused {
            " search type · move ↑↓/ctrl+n/p · open enter · back esc"
        } else {
            " ↑↓/j/k rows · ←→ workspace · / search · a/b/w/i/d filter · enter open · esc close"
        },
        Style::default().fg(p.overlay0).bg(p.panel_bg),
    );
    Some(NavigatorRender {
        area: q,
        search: Rect::new(i.x, i.y, i.width, 1),
        rows: row_hits,
        scrollbar: scrollbar.unwrap_or_default(),
        metrics: Some(metrics),
        cursor,
    })
}
