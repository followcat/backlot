// Derived from herdr src/client/shell/mobile.rs::mobile_items (Apache-2.0).

use std::collections::HashSet;

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use super::mobile::{MobileItem, compact_tab_status, mobile_endpoint_state};
use super::navigate::WorkspaceNavigationTarget;
use super::render::ClientMobileTarget;
use super::sidebar_render::workspace_entries;
use super::snapshot::{ClientEndpointId, ClientShellEndpoint, ClientShellSnapshot};
use super::status::{AgentStatus, status_color, status_icon, status_text};
use super::text::truncate_end;

pub(crate) fn mobile_items(
    snapshot: &ClientShellSnapshot,
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    config: &super::ClientShellConfig,
    selected_workspace_id: Option<&WorkspaceNavigationTarget>,
    content_width: u16,
) -> Vec<MobileItem> {
    let palette = &config.palette;
    let mut items = Vec::new();
    if endpoints.len() > 1 {
        items.push(MobileItem::section("machines", palette));
        for endpoint in endpoints {
            let background = palette.panel_bg;
            let (symbol, state, color) =
                super::endpoints::endpoint_status_presentation(endpoint.status, palette);
            items.push(MobileItem {
                lines: vec![
                    Line::from(vec![
                        Span::styled("  ", Style::default().bg(background)),
                        Span::styled(symbol, Style::default().fg(color).bg(background)),
                        Span::styled(
                            format!(" {}", endpoint.label),
                            Style::default()
                                .fg(palette.text)
                                .bg(background)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]),
                    Line::from(Span::styled(
                        format!("    {state}"),
                        Style::default().fg(palette.overlay0).bg(background),
                    )),
                ],
                background,
                target: Some(ClientMobileTarget::Machine(endpoint.endpoint_id.clone())),
            });
        }
    }
    let agents = super::endpoints::aggregate_agent_rows(
        endpoints,
        active_endpoint_id,
        config.agent_panel_sort,
    );
    let agent_view_label = snapshot.agent_view_label.as_deref();
    if !agents.is_empty() || agent_view_label.is_some() {
        let title = agent_view_label
            .map(|label| format!("agents · {label}"))
            .unwrap_or_else(|| "agents".to_owned());
        items.push(MobileItem::section(title, palette));
        if agents.is_empty() {
            items.push(MobileItem {
                lines: vec![Line::from(Span::styled(
                    "  no matching agents",
                    Style::default()
                        .fg(palette.overlay0)
                        .bg(palette.panel_bg)
                        .add_modifier(Modifier::DIM),
                ))],
                background: palette.panel_bg,
                target: None,
            });
        }
        for row in agents {
            let endpoint = row.endpoint;
            let agent = row.agent;
            let workspace = endpoint
                .snapshot
                .workspaces
                .iter()
                .find(|workspace| workspace.workspace_id == agent.workspace_id);
            let tab = endpoint
                .snapshot
                .tabs
                .iter()
                .find(|tab| tab.tab_id == agent.tab_id);
            let agent_label = agent
                .display_agent
                .as_deref()
                .or(agent.name.as_deref())
                .or(agent.agent.as_deref())
                .unwrap_or("agent");
            let primary = workspace
                .map(|workspace| workspace.label.as_str())
                .unwrap_or(agent_label);
            let mut detail = Vec::new();
            let workspace_tab_count = endpoint
                .snapshot
                .tabs
                .iter()
                .filter(|candidate| candidate.workspace_id == agent.workspace_id)
                .count();
            if let Some(tab) = tab.filter(|tab| tab.custom_label || workspace_tab_count > 1) {
                detail.push(tab.label.clone());
            }
            let status_key = status_text(agent.agent_status);
            detail.push(
                agent
                    .state_labels
                    .iter()
                    .find(|(key, _)| key == status_key)
                    .map(|(_, label)| label.clone())
                    .unwrap_or_else(|| {
                        if agent.agent_status == AgentStatus::Unknown {
                            "idle".to_owned()
                        } else {
                            status_key.to_owned()
                        }
                    }),
            );
            detail.push(agent_label.to_owned());
            if endpoint.stale() {
                detail.push(mobile_endpoint_state(endpoint.status).to_owned());
            }
            let background = if endpoint.endpoint_id == active_endpoint_id && agent.focused {
                palette.surface_dim
            } else {
                palette.panel_bg
            };
            let foreground = if endpoint.stale() {
                palette.overlay0
            } else {
                palette.text
            };
            let dim = if endpoint.stale() {
                Modifier::DIM
            } else {
                Modifier::empty()
            };
            items.push(MobileItem {
                lines: vec![
                    Line::from(vec![
                        Span::styled("  ", Style::default().bg(background)),
                        Span::styled(
                            status_icon(agent.agent_status, config.status_indicators),
                            Style::default()
                                .fg(if endpoint.stale() {
                                    palette.overlay0
                                } else {
                                    status_color(agent.agent_status, palette)
                                })
                                .bg(background)
                                .add_modifier(dim),
                        ),
                        Span::styled(" ", Style::default().bg(background)),
                        Span::styled(
                            truncate_end(
                                &format!("{} · {primary}", endpoint.label),
                                usize::from(content_width.saturating_sub(5)),
                            ),
                            Style::default()
                                .fg(foreground)
                                .bg(background)
                                .add_modifier(Modifier::BOLD | dim),
                        ),
                    ]),
                    Line::from(Span::styled(
                        truncate_end(
                            &format!("  {}", detail.join(" · ")),
                            usize::from(content_width),
                        ),
                        Style::default()
                            .fg(palette.overlay0)
                            .bg(background)
                            .add_modifier(dim),
                    )),
                ],
                background,
                target: Some(ClientMobileTarget::Agent {
                    endpoint_id: endpoint.endpoint_id.clone(),
                    pane_id: agent.pane_id.clone(),
                }),
            });
        }
    }

    items.push(MobileItem::section("spaces", palette));
    items.push(MobileItem::action(
        "  + new workspace",
        ClientMobileTarget::NewWorkspace,
        palette,
    ));
    for endpoint in super::endpoints::cached_endpoint_snapshots(endpoints) {
        for entry in workspace_entries(endpoint.snapshot, &HashSet::new()) {
            let Some(workspace) = endpoint.snapshot.workspaces.get(entry.index) else {
                continue;
            };
            let selected = selected_workspace_id.is_some_and(|target| {
                target.matches(endpoint.endpoint_id, &workspace.workspace_id)
            });
            let background = if selected {
                if palette.surface0 == ratatui::style::Color::Reset {
                    palette.active_row_bg
                } else {
                    palette.surface0
                }
            } else if endpoint.endpoint_id == active_endpoint_id && workspace.focused {
                if selected_workspace_id.is_some()
                    && palette.surface0 == ratatui::style::Color::Reset
                {
                    palette.panel_bg
                } else {
                    palette.surface_dim
                }
            } else {
                palette.panel_bg
            };
            let connector = if entry.indented {
                if entry.last_child {
                    "└─ "
                } else {
                    "├─ "
                }
            } else {
                ""
            };
            let name = if entry.indented && !workspace.custom_label {
                workspace
                    .branch
                    .as_deref()
                    .and_then(|branch| branch.strip_prefix("worktree/").or(Some(branch)))
                    .unwrap_or(&workspace.label)
            } else {
                &workspace.label
            };
            let branch = workspace.branch.as_deref().unwrap_or("shell");
            let detail_prefix = if entry.indented {
                if entry.last_child {
                    "       "
                } else {
                    "  │    "
                }
            } else {
                "  "
            };
            let dim = if endpoint.stale() {
                Modifier::DIM
            } else {
                Modifier::empty()
            };
            let foreground = if endpoint.stale() {
                palette.overlay0
            } else {
                palette.text
            };
            let status = if endpoint.stale() {
                palette.overlay0
            } else {
                status_color(workspace.agent_status, palette)
            };
            let stale_detail = if endpoint.stale() {
                format!(" · {}", mobile_endpoint_state(endpoint.status))
            } else {
                String::new()
            };
            items.push(MobileItem {
                lines: vec![
                    Line::from(vec![
                        Span::styled(
                            format!("  {connector}"),
                            Style::default()
                                .fg(palette.overlay0)
                                .bg(background)
                                .add_modifier(dim),
                        ),
                        Span::styled(
                            status_icon(workspace.agent_status, config.status_indicators),
                            Style::default().fg(status).bg(background).add_modifier(dim),
                        ),
                        Span::styled(" ", Style::default().bg(background)),
                        Span::styled(
                            truncate_end(
                                &format!("{} · {name}", endpoint.label),
                                usize::from(content_width.saturating_sub(if entry.indented {
                                    8
                                } else {
                                    5
                                })),
                            ),
                            Style::default()
                                .fg(foreground)
                                .bg(background)
                                .add_modifier(Modifier::BOLD | dim),
                        ),
                    ]),
                    Line::from(Span::styled(
                        truncate_end(
                            &format!(
                                "{detail_prefix}{branch} · {}{stale_detail}",
                                compact_tab_status(endpoint.snapshot, workspace)
                            ),
                            usize::from(content_width),
                        ),
                        Style::default()
                            .fg(palette.overlay0)
                            .bg(background)
                            .add_modifier(dim),
                    )),
                ],
                background,
                target: Some(ClientMobileTarget::Workspace {
                    endpoint_id: endpoint.endpoint_id.clone(),
                    workspace_id: workspace.workspace_id.clone(),
                }),
            });
        }
    }

    if let Some(workspace_id) = snapshot.focused_workspace_id.as_deref() {
        items.push(MobileItem::section("tabs", palette));
        items.push(MobileItem::action(
            "  + new tab",
            ClientMobileTarget::NewTab,
            palette,
        ));
        for (index, tab) in snapshot
            .tabs
            .iter()
            .filter(|tab| tab.workspace_id == workspace_id)
            .enumerate()
        {
            let background = if tab.focused {
                palette.surface_dim
            } else {
                palette.panel_bg
            };
            let label = if tab.custom_label {
                format!("{} · {}", index + 1, tab.label)
            } else {
                format!("tab {}", tab.label)
            };
            let label = format!(
                "  {}",
                truncate_end(&label, usize::from(content_width.saturating_sub(3)),)
            );
            items.push(MobileItem {
                lines: vec![Line::from(Span::styled(
                    label,
                    Style::default()
                        .fg(palette.text)
                        .bg(background)
                        .add_modifier(Modifier::BOLD),
                ))],
                background,
                target: Some(ClientMobileTarget::Tab {
                    endpoint_id: active_endpoint_id.clone(),
                    tab_id: tab.tab_id.clone(),
                }),
            });
        }
    }
    items
}
