// Derived from herdr src/client/shell/endpoint_sidebar.rs (Apache-2.0).

use std::collections::HashSet;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};

use super::endpoints::collapsed_groups_for_endpoint;
use super::render::{
    MachineHit, ShellHitMap, ShellRenderState, WORKSPACE_HEADER_ROWS, WorkspaceHit, display_width,
    put_right_text, put_text,
};
use super::sidebar_render::{
    displayed_workspace_status, render_parent_group_toggle, render_workspace_rows,
    workspace_entries_for_sidebar, workspace_rows,
};
use super::snapshot::{ClientEndpointId, ClientEndpointStatus, ClientShellSnapshot};
use super::spans::{expanded_sidebar_sections, sidebar_section_divider_rect};
use super::studio_rows::endpoint_is_machine;
use super::studio_tree::SidebarRow;
use super::workspace::WorkspaceEntry;

enum Row {
    Endpoint(usize),
    Workspace {
        endpoint: usize,
        entry: WorkspaceEntry,
    },
    /// W1: an extra studio row under a machine — a ledger studio with
    /// no space, the `finished (n)` group, or an `unreachable` line.
    Studio {
        endpoint: usize,
        row: crate::chrome::studio_tree::Row,
    },
}

impl Row {
    /// W1b: the flat-cursor identity this drawn row stands for, when it
    /// is selectable — the same `SidebarRow` the tree's cursor walks.
    fn selectable(&self, endpoints: &[super::snapshot::ClientShellEndpoint]) -> Option<SidebarRow> {
        match self {
            Row::Endpoint(index) => Some(SidebarRow::Machine(
                endpoints.get(*index)?.endpoint_id.clone(),
            )),
            Row::Workspace { endpoint, entry } => {
                let endpoint = endpoints.get(*endpoint)?;
                let workspace_id = endpoint
                    .snapshot
                    .as_deref()?
                    .workspaces
                    .get(entry.index)?
                    .workspace_id
                    .clone();
                Some(SidebarRow::Workspace {
                    endpoint: endpoint.endpoint_id.clone(),
                    workspace_id,
                })
            }
            Row::Studio { endpoint, row } => Some(SidebarRow::Studio {
                endpoint: endpoints.get(*endpoint)?.endpoint_id.clone(),
                row: row.clone(),
            }),
        }
    }
}

/// W1b: the keyboard cursor's mark — a `▌` at the row's left edge in
/// the accent, over the row's own background.
fn paint_tree_cursor(buffer: &mut Buffer, rect: Rect, palette: &super::palette::Palette) {
    if rect.is_empty() {
        return;
    }
    buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
    put_text(
        buffer,
        rect.x,
        rect.y,
        1,
        "▌",
        Style::default().fg(palette.accent),
    );
}

pub(crate) fn render_expanded(
    buffer: &mut Buffer,
    area: Rect,
    active_snapshot: Option<&ClientShellSnapshot>,
    config: &super::ClientShellConfig,
    state: &mut ShellRenderState<'_>,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    super::render::render_sidebar_background(buffer, area, palette, config.studio_chrome);
    hits.sidebar_divider = if area.is_empty() {
        Rect::default()
    } else {
        Rect::new(area.right().saturating_sub(1), area.y, 1, area.height)
    };

    let empty_collapsed_groups = HashSet::new();

    let mut rows = Vec::new();
    for (endpoint_index, endpoint) in state.endpoints.iter().enumerate() {
        rows.push(Row::Endpoint(endpoint_index));
        if state.collapsed_endpoints.contains(&endpoint.endpoint_id) {
            continue;
        }
        if let Some(snapshot) = endpoint.snapshot.as_deref() {
            let collapsed_groups = collapsed_groups_for_endpoint(state, &endpoint.endpoint_id)
                .unwrap_or(&empty_collapsed_groups);
            let hidden = state
                .studio_tree
                .and_then(|tree| tree.live_studios.get(&endpoint.endpoint_id));
            rows.extend(
                workspace_entries_for_sidebar(
                    snapshot,
                    collapsed_groups,
                    state.hide_stage_space,
                    hidden,
                )
                .into_iter()
                .map(|entry| Row::Workspace {
                    endpoint: endpoint_index,
                    entry,
                }),
            );
        }
        if let Some(tree) = state.studio_tree {
            rows.extend(
                tree.get(&endpoint.endpoint_id)
                    .iter()
                    .cloned()
                    .map(|row| Row::Studio {
                        endpoint: endpoint_index,
                        row,
                    }),
            );
        }
    }
    // W1b: the flat-cursor identity of every drawn row, so the highlight
    // and the keyboard cursor name the same thing.
    let identities: Vec<Option<SidebarRow>> = rows
        .iter()
        .map(|row| row.selectable(state.endpoints))
        .collect();
    let cursor_row = state.tree_cursor.and_then(|cursor| {
        identities
            .iter()
            .position(|row| row.as_ref() == Some(cursor))
    });
    // A machine row's second line is U6c's version sub-line —
    // `machine_lines` is `None` in plain mode, so Herdr's rows stay
    // one line and the scroll math is unchanged there.
    let endpoint_height = |endpoint: &super::snapshot::ClientShellEndpoint| -> u16 {
        1 + u16::from(
            state
                .machine_lines
                .is_some_and(|lines| lines.contains_key(&endpoint.endpoint_id)),
        )
    };
    let row_heights = rows
        .iter()
        .map(|row| match row {
            Row::Endpoint(index) => endpoint_height(&state.endpoints[*index]),
            Row::Workspace { endpoint, entry } => {
                let endpoint = &state.endpoints[*endpoint];
                let collapsed_groups = collapsed_groups_for_endpoint(state, &endpoint.endpoint_id)
                    .unwrap_or(&empty_collapsed_groups);
                endpoint
                    .snapshot
                    .as_deref()
                    .and_then(|snapshot| {
                        let workspace = snapshot.workspaces.get(entry.index)?;
                        Some(
                            workspace_rows(
                                workspace,
                                displayed_workspace_status(snapshot, workspace, collapsed_groups),
                                entry.indented,
                                &config.spaces,
                            )
                            .len()
                            .max(1)
                            .min(u16::MAX as usize) as u16,
                        )
                    })
                    .unwrap_or(1)
            }
            Row::Studio { .. } => 1,
        })
        .collect::<Vec<_>>();
    let gaps = rows
        .iter()
        .enumerate()
        .map(|(index, row)| match (row, rows.get(index + 1)) {
            (
                Row::Workspace { endpoint, .. },
                Some(Row::Workspace {
                    endpoint: next_endpoint,
                    entry,
                }),
            ) if endpoint == next_endpoint => u16::from(!entry.indented) * config.spaces.row_gap,
            _ => 0,
        })
        .collect::<Vec<_>>();
    // U1: in studio mode the machines tree takes only the rows it
    // needs, at most a third of the sidebar — the cast takes the rest
    // and the section divider goes away, as the single-endpoint
    // sidebar does.
    let (workspace_area, detail_area) = if state.cast.is_some() {
        let needed = row_heights
            .iter()
            .chain(gaps.iter())
            .fold(0u16, |sum, h| sum.saturating_add(*h))
            .saturating_add(WORKSPACE_HEADER_ROWS + 1);
        crate::cast::sections(area, needed)
    } else {
        expanded_sidebar_sections(area, state.sidebar_section_split)
    };
    hits.sidebar_section_divider = if state.cast.is_some() {
        Rect::default()
    } else {
        sidebar_section_divider_rect(area, state.sidebar_section_split)
    };
    hits.update_badge = super::sidebar_deco::render_machines_header(
        buffer,
        workspace_area,
        state.tree_focused,
        config.studio_chrome,
        state.update_badge,
        palette,
    );
    let body = Rect::new(
        workspace_area.x,
        workspace_area.y.saturating_add(WORKSPACE_HEADER_ROWS),
        workspace_area.width,
        workspace_area
            .height
            .saturating_sub(WORKSPACE_HEADER_ROWS + 1),
    );
    hits.workspace_body = body;
    let reveal_navigation = !body.is_empty() && std::mem::take(state.reveal_navigation_workspace);
    let reveal_focus = !body.is_empty() && std::mem::take(state.reveal_focused_workspace);
    if reveal_navigation || reveal_focus {
        let selected_row = rows.iter().position(|row| match row {
            Row::Workspace { endpoint, entry } => {
                let endpoint = &state.endpoints[*endpoint];
                endpoint
                    .snapshot
                    .as_deref()
                    .and_then(|snapshot| snapshot.workspaces.get(entry.index))
                    .is_some_and(|workspace| {
                        if reveal_navigation {
                            state.selected_workspace_id.is_some_and(|target| {
                                target.matches(&endpoint.endpoint_id, &workspace.workspace_id)
                            })
                        } else {
                            &endpoint.endpoint_id == state.active_endpoint_id
                                && active_snapshot.is_some_and(|snapshot| {
                                    snapshot.focused_workspace_id.as_deref()
                                        == Some(workspace.workspace_id.as_str())
                                })
                        }
                    })
            }
            Row::Endpoint(_) | Row::Studio { .. } => false,
        });
        if let Some(selected_row) = selected_row {
            *state.workspace_scroll = super::scroll::list_scroll_start_to_reveal(
                &row_heights,
                &gaps,
                body.height,
                *state.workspace_scroll,
                selected_row,
            );
        }
    }
    // W1/M4: the current studio's machine row never falls out of a
    // short sidebar — the section scrolls to that machine instead of
    // dropping it. A stage-down or finished studio has no focused
    // terminal to reveal, so its own row cannot move the list.
    if !body.is_empty()
        && std::mem::take(state.reveal_studio_row)
        && let Some(key) = state.current_studio
        && let Some(machine_row) = rows.iter().position(|row| match row {
            Row::Endpoint(index) => {
                endpoint_is_machine(&state.endpoints[*index].endpoint_id, key.machine.as_deref())
            }
            _ => false,
        })
    {
        *state.workspace_scroll = super::scroll::list_scroll_start_to_reveal(
            &row_heights,
            &gaps,
            body.height,
            *state.workspace_scroll,
            machine_row,
        );
    }
    // W1b: `Ctrl+B t`'s cursor never falls out of view — the section
    // scrolls to the highlighted row, as it does for the current studio.
    if !body.is_empty()
        && std::mem::take(state.reveal_tree_row)
        && let Some(cursor_row) = cursor_row
    {
        *state.workspace_scroll = super::scroll::list_scroll_start_to_reveal(
            &row_heights,
            &gaps,
            body.height,
            *state.workspace_scroll,
            cursor_row,
        );
    }
    let metrics = super::scroll::list_scroll_metrics(
        &row_heights,
        &gaps,
        body.height,
        *state.workspace_scroll,
    );
    hits.workspace_max_scroll = metrics.max_offset_from_bottom;
    hits.workspace_scroll_metrics = Some(metrics);
    *state.workspace_scroll = metrics
        .max_offset_from_bottom
        .saturating_sub(metrics.offset_from_bottom);
    let show_scrollbar = metrics.max_offset_from_bottom > 0 && body.width > 1;
    let content_width = body.width.saturating_sub(u16::from(show_scrollbar));
    let mut y = body.y;
    for (row_index, row) in rows.iter().enumerate().skip(*state.workspace_scroll) {
        // W1b: the keyboard cursor paints the row it names with a `▌`
        // at its left edge, the accent mark the focused studio row uses
        // so the tree's own selection reads at a glance.
        let is_cursor = cursor_row == Some(row_index);
        match row {
            Row::Endpoint(index) => {
                if y >= body.bottom() {
                    break;
                }
                let endpoint = &state.endpoints[*index];
                let height = endpoint_height(endpoint).min(body.bottom().saturating_sub(y));
                if height == 0 {
                    break;
                }
                let rect = Rect::new(body.x, y, content_width, height);
                let name_row = Rect::new(rect.x, rect.y, rect.width, 1);
                let lot_marked = !is_cursor
                    && state.lot_selected_studio.is_some_and(|key| {
                        endpoint_is_machine(&endpoint.endpoint_id, key.machine.as_deref())
                    });
                let collapsed = state.collapsed_endpoints.contains(&endpoint.endpoint_id);
                let marker = if collapsed { "▸" } else { "▾" };
                let status_badge = super::endpoint_sidebar_row::render_endpoint_row(
                    buffer,
                    name_row,
                    marker,
                    endpoint,
                    collapsed && &endpoint.endpoint_id == state.active_endpoint_id,
                    state.machine_diagnostics,
                    palette,
                );
                let has_machine_line = if let Some(line) = state
                    .machine_lines
                    .and_then(|lines| lines.get(&endpoint.endpoint_id))
                    && rect.height > 1
                {
                    super::endpoint_sidebar_row::render_machine_line(
                        buffer,
                        Rect::new(rect.x, rect.y + 1, rect.width, 1),
                        line,
                        lot_marked,
                        palette,
                    );
                    true
                } else {
                    false
                };
                if lot_marked {
                    let bar_style = Style::default().fg(palette.accent);
                    put_text(buffer, rect.x, rect.y, 1, "▎", bar_style);
                    if has_machine_line {
                        put_text(buffer, rect.x, rect.y + 1, 1, "▎", bar_style);
                    }
                }
                hits.machines.push(MachineHit {
                    rect,
                    status_badge,
                    collapse_toggle: Rect::new(
                        rect.x.saturating_add(1),
                        rect.y,
                        u16::from(rect.width > 1),
                        1,
                    ),
                    endpoint_id: endpoint.endpoint_id.clone(),
                });
                if is_cursor {
                    paint_tree_cursor(buffer, rect, palette);
                } else if !lot_marked
                    && !state.tree_focused
                    && state.tree_remembered.is_some()
                    && identities.get(row_index).and_then(|o| o.as_ref()) == state.tree_remembered
                    && !rect.is_empty()
                {
                    put_text(
                        buffer,
                        rect.x,
                        rect.y,
                        1,
                        "·",
                        Style::default().fg(palette.overlay0),
                    );
                }
                y = y
                    .saturating_add(height)
                    .saturating_add(gaps.get(row_index).copied().unwrap_or(0));
            }
            Row::Workspace { endpoint, entry } => {
                let endpoint = &state.endpoints[*endpoint];
                let Some(snapshot) = endpoint.snapshot.as_deref() else {
                    continue;
                };
                let Some(workspace) = snapshot.workspaces.get(entry.index) else {
                    continue;
                };
                let collapsed_groups = collapsed_groups_for_endpoint(state, &endpoint.endpoint_id)
                    .unwrap_or(&empty_collapsed_groups);
                let status = displayed_workspace_status(snapshot, workspace, collapsed_groups);
                let tokens = workspace_rows(workspace, status, entry.indented, &config.spaces);
                let height = (tokens.len().max(1).min(u16::MAX as usize) as u16).min(body.height);
                if y.saturating_add(height) > body.bottom() {
                    break;
                }
                let rect = Rect::new(body.x, y, content_width, height);
                let nested = Rect::new(
                    rect.x.saturating_add(2),
                    rect.y,
                    rect.width.saturating_sub(2),
                    rect.height,
                );
                let endpoint_active = &endpoint.endpoint_id == state.active_endpoint_id;
                let studio_focused = state.focused_studio.is_some_and(|key| {
                    let machine_matches = match (&endpoint.endpoint_id, &key.machine) {
                        (ClientEndpointId::Local, None) => true,
                        (ClientEndpointId::Remote(ep), Some(m)) => ep == m,
                        _ => false,
                    };
                    machine_matches && workspace.label == key.id
                });
                let selected = state.selected_workspace_id.is_some_and(|target| {
                    target.matches(&endpoint.endpoint_id, &workspace.workspace_id)
                });
                if studio_focused && !selected {
                    buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
                }
                render_workspace_rows(
                    buffer,
                    nested,
                    status,
                    config.status_indicators,
                    entry,
                    tokens,
                    (endpoint_active && workspace.focused) || studio_focused,
                    selected,
                    state.selected_workspace_id.is_some(),
                    false,
                    palette,
                );
                if studio_focused && !selected && !rect.is_empty() {
                    put_text(
                        buffer,
                        rect.x,
                        rect.y,
                        1,
                        "▎",
                        Style::default().fg(palette.overlay1),
                    );
                }
                if endpoint.status != ClientEndpointStatus::Online {
                    buffer.set_style(
                        rect,
                        Style::default()
                            .fg(palette.overlay0)
                            .add_modifier(Modifier::DIM),
                    );
                }
                let group_toggle = render_parent_group_toggle(
                    buffer,
                    rect,
                    snapshot,
                    entry.index,
                    collapsed_groups,
                    palette,
                );
                hits.workspaces.push(WorkspaceHit {
                    rect,
                    endpoint_id: endpoint.endpoint_id.clone(),
                    workspace_id: workspace.workspace_id.clone(),

                    group_toggle,
                });
                if is_cursor {
                    paint_tree_cursor(buffer, rect, palette);
                } else if !state.tree_focused
                    && state.tree_remembered.is_some()
                    && identities.get(row_index).and_then(|o| o.as_ref()) == state.tree_remembered
                    && !rect.is_empty()
                {
                    put_text(
                        buffer,
                        rect.x,
                        rect.y,
                        1,
                        "·",
                        Style::default().fg(palette.overlay0),
                    );
                }
                y = y
                    .saturating_add(height)
                    .saturating_add(gaps.get(row_index).copied().unwrap_or(0));
            }
            Row::Studio { endpoint, row } => {
                if y >= body.bottom() {
                    break;
                }
                let endpoint = &state.endpoints[*endpoint];
                let rect = Rect::new(body.x, y, content_width, 1);
                super::studio_rows::render_studio_row(
                    buffer,
                    rect,
                    palette,
                    config.studio_chrome,
                    endpoint,
                    state.current_studio,
                    row,
                    hits,
                );
                if is_cursor {
                    paint_tree_cursor(buffer, rect, palette);
                } else if !state.tree_focused
                    && state.tree_remembered.is_some()
                    && identities.get(row_index).and_then(|o| o.as_ref()) == state.tree_remembered
                    && !rect.is_empty()
                {
                    put_text(
                        buffer,
                        rect.x,
                        rect.y,
                        1,
                        "·",
                        Style::default().fg(palette.overlay0),
                    );
                }
                y = y.saturating_add(1);
            }
        }
    }
    if show_scrollbar {
        let track = Rect::new(body.right().saturating_sub(1), body.y, 1, body.height);
        hits.workspace_scrollbar = track;
        super::scroll::render_list_scrollbar(buffer, track, metrics, palette);
    }

    let footer_y = workspace_area.bottom().saturating_sub(1);
    if state.tree_focused && workspace_area.height > 1 {
        let hint = crate::chrome::text::fit_hints(
            &["↑↓ ←→ Tab ↵ m n Esc", "↑↓ Tab ↵ n Esc", "Esc back"],
            workspace_area.width.saturating_sub(1) as usize,
        );
        put_text(
            buffer,
            workspace_area.x + 1,
            footer_y,
            workspace_area.width.saturating_sub(1),
            hint,
            Style::default().fg(palette.overlay0),
        );
    } else if config.mouse_capture && config.studio_chrome {
        super::sidebar_deco::render_footer_action(
            buffer,
            workspace_area,
            footer_y,
            state,
            palette,
            hits,
        );
    } else if config.mouse_capture {
        let label = format!(
            " new · {}",
            super::endpoint_sidebar_row::active_endpoint_label(state)
        );
        hits.new_workspace = Rect::new(
            workspace_area.x,
            footer_y,
            display_width(&label).min(workspace_area.width),
            u16::from(workspace_area.height > 0),
        );
        put_text(
            buffer,
            workspace_area.x,
            footer_y,
            workspace_area.width,
            &label,
            Style::default().fg(palette.overlay0),
        );
        let dot = state.update_badge.accent_style(palette);
        let width = if dot.is_some() { 8 } else { 6 };
        let width = width.min(workspace_area.width);
        hits.global_launcher = Rect::new(
            workspace_area.right().saturating_sub(width),
            footer_y,
            width,
            1,
        );
        put_right_text(
            buffer,
            workspace_area,
            footer_y,
            "menu",
            Style::default().fg(palette.overlay0),
        );
        if let Some(style) = dot.filter(|_| workspace_area.width >= 6) {
            put_text(
                buffer,
                workspace_area.right().saturating_sub(6),
                footer_y,
                2,
                "● ",
                style,
            );
        }
    }
    match &state.cast {
        Some(cast) => crate::cast::render_cast(
            buffer,
            detail_area,
            cast,
            config,
            state.cast_scroll,
            state.reveal_focused_agent,
            hits,
        ),
        None => super::endpoint_agents::render_expanded(
            buffer,
            detail_area,
            active_snapshot.and_then(|snapshot| snapshot.agent_view_label.as_deref()),
            state.endpoints,
            state.active_endpoint_id,
            config,
            state.agent_scroll,
            state.reveal_focused_agent,
            hits,
        ),
    }
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
    if !config.plain {
        super::render::render_sidebar_help(buffer, area, palette, hits);
    }
}
