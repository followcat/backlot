// Derived from herdr src/client/shell/endpoint_agents.rs (Apache-2.0).

use std::collections::HashMap;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};

use super::endpoints::aggregate_agent_rows;
use super::render::{ShellHitMap, put_text};
use super::snapshot::{ClientEndpointId, ClientShellEndpoint};
use super::status::{status_color, status_icon};

pub(crate) fn render_collapsed(
    buffer: &mut Buffer,
    area: Rect,
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    config: &super::ClientShellConfig,
    hits: &mut ShellHitMap,
) {
    let rows = agent_rows(endpoints, active_endpoint_id, config);
    for (index, row) in rows.into_iter().take(area.height as usize).enumerate() {
        let rect = Rect::new(area.x, area.y + index as u16, area.width, 1);
        if row.agent.focused {
            buffer.set_style(rect, Style::default().bg(config.palette.active_row_bg));
        }
        let initial = row.machine_label.chars().next().unwrap_or('?');
        put_text(
            buffer,
            rect.x,
            rect.y,
            rect.width,
            &format!(
                "{initial}{}",
                status_icon(row.agent.status, config.status_indicators)
            ),
            Style::default()
                .fg(if row.stale {
                    config.palette.overlay0
                } else {
                    status_color(row.agent.status, &config.palette)
                })
                .add_modifier(if row.stale {
                    Modifier::DIM
                } else {
                    Modifier::empty()
                }),
        );
        hits.endpoint_agents
            .push((rect, row.endpoint_id, row.agent.pane_id));
    }
}

// Keep the pinned source renderer signature for later ports.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_expanded(
    buffer: &mut Buffer,
    area: Rect,
    agent_view_label: Option<&str>,
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    config: &super::ClientShellConfig,
    agent_scroll: &mut usize,
    reveal_focused_agent: &mut bool,
    hits: &mut ShellHitMap,
) {
    if !super::agents::render_agent_panel_header(buffer, area, agent_view_label, config, hits) {
        return;
    }
    let entries = panel_entries(endpoints, active_endpoint_id, agent_view_label, config);
    let reveal = std::mem::take(reveal_focused_agent);
    let target = reveal
        .then(|| {
            entries
                .iter()
                .position(|entry| matches!(entry, PanelEntry::Agent(row) if row.agent.focused))
        })
        .flatten();
    super::agents::render_agent_list(
        buffer,
        area,
        &entries,
        agent_view_label.map(|_| " no matching agents"),
        config,
        agent_scroll,
        hits,
        target,
        PanelEntry::lines,
        |buffer, rect, entry, hits| match entry {
            PanelEntry::Space { label, gap } => {
                super::agents::render_space_header(buffer, rect, label, *gap, config);
            }
            PanelEntry::Agent(row) => {
                super::agents::render_agent_row(buffer, rect, &row.agent, config);
                if row.stale {
                    buffer.set_style(
                        rect,
                        Style::default()
                            .fg(config.palette.overlay0)
                            .add_modifier(Modifier::DIM),
                    );
                }
                hits.endpoint_agents.push((
                    rect,
                    row.endpoint_id.clone(),
                    row.agent.pane_id.clone(),
                ));
            }
        },
    );
}

/// The position `agent_scroll` takes to show one machine's agent: its
/// index among the panel's rendered entries, space headers included.
/// `None` when the panel does not render that agent.
pub(crate) fn agent_entry_index(
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    agent_view_label: Option<&str>,
    config: &super::ClientShellConfig,
    endpoint_id: &ClientEndpointId,
    pane_id: &str,
) -> Option<usize> {
    panel_entries(endpoints, active_endpoint_id, agent_view_label, config)
        .iter()
        .position(|entry| {
            matches!(entry, PanelEntry::Agent(row)
                if &row.endpoint_id == endpoint_id && row.agent.pane_id == pane_id)
        })
}

/// One entry of the multi-machine agents panel: an agent, or (grouped by
/// space) the name of the space whose agents follow.
enum PanelEntry {
    /// A space header; `gap` puts a blank line above every group but the first.
    Space {
        label: String,
        gap: bool,
    },
    Agent(EndpointAgentRow),
}

impl PanelEntry {
    fn lines(&self) -> usize {
        match self {
            PanelEntry::Space { gap, .. } => 1 + usize::from(*gap),
            PanelEntry::Agent(row) => row.agent.rows.len(),
        }
    }
}

/// The panel's entries across machines. With `agent_panel_space_headers`
/// on, sorted by space and not filtered to a view, a header with the
/// space's name opens each run of one machine's space, as the Herdr app
/// heads its grouped list: the machine is not named, and a space of the
/// same name on another machine opens a group of its own.
fn panel_entries(
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    agent_view_label: Option<&str>,
    config: &super::ClientShellConfig,
) -> Vec<PanelEntry> {
    let grouped = config.agent_panel_space_headers
        && config.agent_panel_sort == crate::config::AgentPanelSortConfig::Spaces
        && agent_view_label.is_none();
    let mut entries = Vec::new();
    let mut current: Option<(ClientEndpointId, String)> = None;
    for row in agent_rows(endpoints, active_endpoint_id, config) {
        if grouped && let Some(label) = row.space_label.clone() {
            let group = (row.endpoint_id.clone(), row.workspace_id.clone());
            if current.as_ref() != Some(&group) {
                entries.push(PanelEntry::Space {
                    label,
                    gap: current.is_some(),
                });
                current = Some(group);
            }
        }
        entries.push(PanelEntry::Agent(row));
    }
    entries
}

struct EndpointAgentRow {
    endpoint_id: ClientEndpointId,
    machine_label: String,
    stale: bool,
    workspace_id: String,
    /// The label of the agent's space on its machine, when the machine's
    /// snapshot lists that space.
    space_label: Option<String>,
    agent: super::agents::AgentRow,
}

fn agent_rows(
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    config: &super::ClientShellConfig,
) -> Vec<EndpointAgentRow> {
    let mut rendered_rows = endpoints
        .iter()
        .filter_map(|endpoint| {
            endpoint.snapshot.as_deref().map(|snapshot| {
                snapshot
                    .agents
                    .iter()
                    .filter_map(|agent| {
                        super::agents::agent_row(
                            snapshot,
                            &agent.pane_id,
                            config,
                            Some(&endpoint.label),
                        )
                    })
                    .map(|agent| ((endpoint.endpoint_id.clone(), agent.pane_id.clone()), agent))
                    .collect::<Vec<_>>()
            })
        })
        .flatten()
        .collect::<HashMap<_, _>>();

    aggregate_agent_rows(endpoints, active_endpoint_id, config.agent_panel_sort)
        .into_iter()
        .filter_map(|row| {
            let key = (row.endpoint.endpoint_id.clone(), row.agent.pane_id.clone());
            let mut agent = rendered_rows.remove(&key)?;
            agent.focused &= row.endpoint.endpoint_id == active_endpoint_id;
            Some(EndpointAgentRow {
                endpoint_id: row.endpoint.endpoint_id.clone(),
                machine_label: row.endpoint.label.to_owned(),
                stale: row.endpoint.stale(),
                workspace_id: row.agent.workspace_id.clone(),
                space_label: row
                    .endpoint
                    .snapshot
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.workspace_id == row.agent.workspace_id)
                    .map(|workspace| workspace.label.clone()),
                agent,
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "endpoint_agents_tests.rs"]
mod tests;
