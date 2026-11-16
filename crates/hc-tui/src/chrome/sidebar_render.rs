// Derived from herdr src/client/shell/sidebar.rs (Apache-2.0).

use std::collections::{HashMap, HashSet};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Paragraph, Widget},
};

use super::palette::Palette;
use super::render::{
    ShellHitMap, ShellRenderState, WORKSPACE_HEADER_ROWS, WorkspaceHit, put_right_text,
    put_segment, put_text, put_version_badge,
};
use super::snapshot::{ClientEndpointId, ClientShellSnapshot, ClientShellWorkspace};
use super::spans::{expanded_sidebar_sections, resolved_token_spans, sidebar_section_divider_rect};
use super::status::{AgentStatus, status_color, status_icon, status_priority, status_text};
use super::tokens::{self, ResolvedToken};
use super::workspace::{
    WorkspaceEntry, workspace_active_background, workspace_selection_background,
};
use crate::config::{SpacesSidebarConfig, StatusIndicatorStyle};

pub(crate) fn render_sidebar(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: &ClientShellSnapshot,
    config: &super::ClientShellConfig,
    state: &mut ShellRenderState<'_>,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    super::render::render_sidebar_background(buffer, area, palette);
    hits.sidebar_divider = if area.is_empty() {
        Rect::default()
    } else {
        Rect::new(area.right().saturating_sub(1), area.y, 1, area.height)
    };
    let (workspace_area, detail_area) =
        expanded_sidebar_sections(area, state.sidebar_section_split);
    hits.sidebar_section_divider = sidebar_section_divider_rect(area, state.sidebar_section_split);
    put_text(
        buffer,
        workspace_area.x,
        workspace_area.y,
        workspace_area.width,
        " spaces",
        Style::default()
            .fg(palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );
    // The version badge shares the header row, right-aligned short of
    // the divider column and never touching the `spaces` label.
    let badge_row = Rect::new(
        workspace_area.x,
        workspace_area.y,
        workspace_area.width.saturating_sub(2),
        1,
    );
    hits.update_badge = put_version_badge(
        buffer,
        badge_row,
        workspace_area.x.saturating_add(8),
        state.update_latest,
        palette,
    );

    let entries = workspace_entries(snapshot, state.collapsed_groups);
    let body = Rect::new(
        workspace_area.x,
        workspace_area.y.saturating_add(WORKSPACE_HEADER_ROWS),
        workspace_area.width,
        workspace_area
            .height
            .saturating_sub(WORKSPACE_HEADER_ROWS + 1),
    );
    hits.workspace_body = body;
    let row_heights = entries
        .iter()
        .map(|entry| {
            snapshot
                .workspaces
                .get(entry.index)
                .map(|workspace| {
                    workspace_rows(
                        workspace,
                        displayed_workspace_status(snapshot, workspace, state.collapsed_groups),
                        entry.indented,
                        &config.spaces,
                    )
                    .len()
                    .max(1)
                    .min(u16::MAX as usize) as u16
                })
                .unwrap_or(1)
        })
        .collect::<Vec<_>>();
    let gaps = entries
        .iter()
        .enumerate()
        .map(|(index, _)| {
            entries
                .get(index + 1)
                .map_or(0, |next| u16::from(!next.indented) * config.spaces.row_gap)
        })
        .collect::<Vec<_>>();
    let mut metrics = super::scroll::list_scroll_metrics(
        &row_heights,
        &gaps,
        body.height,
        *state.workspace_scroll,
    );
    if !body.is_empty()
        && std::mem::take(state.reveal_focused_workspace)
        && let Some(target) = entries
            .iter()
            .position(|entry| snapshot.workspaces[entry.index].focused)
    {
        *state.workspace_scroll = super::scroll::list_scroll_start_to_reveal(
            &row_heights,
            &gaps,
            body.height,
            *state.workspace_scroll,
            target,
        );
        metrics = super::scroll::list_scroll_metrics(
            &row_heights,
            &gaps,
            body.height,
            *state.workspace_scroll,
        );
    }
    hits.workspace_max_scroll = metrics.max_offset_from_bottom;
    hits.workspace_scroll_metrics = Some(metrics);
    *state.workspace_scroll = metrics
        .max_offset_from_bottom
        .saturating_sub(metrics.offset_from_bottom);
    let show_scrollbar = metrics.max_offset_from_bottom > 0 && body.width > 1;
    let content_width = body.width.saturating_sub(u16::from(show_scrollbar));
    let mut y = body.y;
    for (entry_position, entry) in entries.iter().enumerate().skip(*state.workspace_scroll) {
        let Some(workspace) = snapshot.workspaces.get(entry.index) else {
            continue;
        };
        let status = displayed_workspace_status(snapshot, workspace, state.collapsed_groups);
        let rows = workspace_rows(workspace, status, entry.indented, &config.spaces);
        let row_height = (rows.len().max(1).min(u16::MAX as usize) as u16).min(body.height);
        if y.saturating_add(row_height) > body.bottom() {
            break;
        }
        let rect = Rect::new(body.x, y, content_width, row_height);
        let selected = state.selected_workspace_id.is_some_and(|target| {
            target.matches(state.active_endpoint_id, &workspace.workspace_id)
        });
        let dragged = state.dragged_workspace_id == Some(workspace.workspace_id.as_str());
        if selected {
            buffer.set_style(rect, Style::default().bg(palette.selection_bg));
        } else if dragged {
            buffer.set_style(rect, Style::default().bg(palette.surface1));
        } else if workspace.focused {
            buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
        }
        render_workspace_rows(
            buffer,
            rect,
            status,
            config.status_indicators,
            entry,
            rows,
            workspace.focused,
            selected,
            state.selected_workspace_id.is_some(),
            dragged,
            palette,
        );
        let group_toggle = render_parent_group_toggle(
            buffer,
            rect,
            snapshot,
            entry.index,
            state.collapsed_groups,
            palette,
        );
        hits.workspaces.push(WorkspaceHit {
            rect,
            endpoint_id: ClientEndpointId::Local,
            workspace_id: workspace.workspace_id.clone(),

            group_toggle,
        });
        let gap = entries
            .get(entry_position + 1)
            .map_or(0, |next| u16::from(!next.indented) * config.spaces.row_gap);
        y = y.saturating_add(row_height + gap);
    }

    if show_scrollbar {
        let track = Rect::new(body.right().saturating_sub(1), body.y, 1, body.height);
        hits.workspace_scrollbar = track;
        super::scroll::render_list_scrollbar(buffer, track, metrics, palette);
    }

    if let Some(row) = state.workspace_drop_indicator_row.filter(|row| {
        *row >= workspace_area.y.saturating_add(1)
            && *row < workspace_area.bottom().saturating_sub(1)
    }) {
        put_text(
            buffer,
            body.x,
            row,
            body.width,
            &"─".repeat(body.width as usize),
            Style::default().fg(palette.accent),
        );
    }

    let footer_y = workspace_area.bottom().saturating_sub(1);
    if config.mouse_capture {
        hits.new_workspace = Rect::new(
            workspace_area.x,
            footer_y,
            5.min(workspace_area.width),
            u16::from(workspace_area.height > 0),
        );
        put_text(
            buffer,
            workspace_area.x,
            footer_y,
            workspace_area.width,
            " new",
            Style::default().fg(palette.overlay0),
        );
        let launcher_width = 6.min(workspace_area.width);
        hits.global_launcher = Rect::new(
            workspace_area.right().saturating_sub(launcher_width),
            footer_y,
            launcher_width,
            1,
        );
        put_right_text(
            buffer,
            workspace_area,
            footer_y,
            "menu",
            Style::default().fg(palette.overlay0),
        );
    }

    super::agents::render_agent_panel(
        buffer,
        detail_area,
        snapshot,
        config,
        state.agent_scroll,
        hits,
    );

    hits.sidebar_toggle = Rect::new(
        area.right().saturating_sub(2),
        area.bottom().saturating_sub(1),
        u16::from(area.width > 1),
        u16::from(area.height > 0),
    );
    put_text(
        buffer,
        hits.sidebar_toggle.x,
        hits.sidebar_toggle.y,
        hits.sidebar_toggle.width,
        "«",
        Style::default().fg(palette.overlay0),
    );
}

#[path = "sidebar_rows.rs"]
mod sidebar_rows;
pub(crate) use sidebar_rows::*;
