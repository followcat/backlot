// Derived from herdr src/client/shell/agent_sidebar.rs (Apache-2.0).

use std::collections::HashMap;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Paragraph, Widget},
};

use super::agents::ordered_agent_pane_ids;
use super::render::put_text;
use super::snapshot::ClientShellSnapshot;
use super::spans::resolved_token_spans;
use super::status::{AgentStatus, status_color, status_icon, status_text};
use super::tokens::{self, ResolvedToken, ResolvedTokenKind};

pub(crate) struct AgentRow {
    pub pane_id: String,
    pub status: AgentStatus,
    pub focused: bool,
    pub rows: Vec<Vec<ResolvedToken>>,
}

/// One entry of the agents panel: an agent, or (grouped by space) the
/// name of the space whose agents follow, as the Herdr app's grouped
/// agents list heads each group.
pub(crate) enum PanelRow {
    /// A space header; `gap` puts a blank line above every group but the first.
    Space {
        label: String,
        gap: bool,
    },
    Agent(AgentRow),
}

impl PanelRow {
    pub(super) fn lines(&self) -> usize {
        match self {
            PanelRow::Space { gap, .. } => 1 + usize::from(*gap),
            PanelRow::Agent(row) => row.rows.len(),
        }
    }
}

/// The panel's entries. With `agent_panel_space_headers` on, sorted by
/// space and not filtered to a view, a header with the space's name opens
/// each run of that space's agents. Otherwise the list stays flat: the
/// reference's space order has no headers, and by priority one space's
/// agents are spread.
pub(crate) fn panel_rows(
    snapshot: &ClientShellSnapshot,
    config: &super::ClientShellConfig,
) -> Vec<PanelRow> {
    let grouped = config.agent_panel_space_headers
        && config.agent_panel_sort == crate::config::AgentPanelSortConfig::Spaces
        && snapshot.agent_view_label.is_none();
    let mut rows = Vec::new();
    let mut current: Option<&str> = None;
    for pane_id in ordered_agent_pane_ids(snapshot, config.agent_panel_sort) {
        let Some(row) = agent_row(snapshot, &pane_id, config, None) else {
            continue;
        };
        if grouped {
            let workspace = snapshot
                .agents
                .iter()
                .find(|agent| agent.pane_id == pane_id)
                .and_then(|agent| {
                    snapshot
                        .workspaces
                        .iter()
                        .find(|workspace| workspace.workspace_id == agent.workspace_id)
                });
            if let Some(workspace) = workspace
                && current != Some(workspace.workspace_id.as_str())
            {
                rows.push(PanelRow::Space {
                    label: workspace.label.clone(),
                    gap: current.is_some(),
                });
                current = Some(workspace.workspace_id.as_str());
            }
        }
        rows.push(PanelRow::Agent(row));
    }
    rows
}

/// A space header: the space's name, dim, on the panel's last line of the
/// entry (the first line is the blank gap when there is one).
pub(super) fn render_space_header(
    buffer: &mut Buffer,
    rect: Rect,
    label: &str,
    gap: bool,
    config: &super::ClientShellConfig,
) {
    let y = rect.y + u16::from(gap);
    if y >= rect.bottom() {
        return;
    }
    put_text(
        buffer,
        rect.x,
        y,
        rect.width,
        &format!(" {label}"),
        Style::default()
            .fg(config.palette.overlay0)
            .add_modifier(Modifier::DIM),
    );
}

pub(crate) fn agent_row(
    snapshot: &ClientShellSnapshot,
    pane_id: &str,
    config: &super::ClientShellConfig,
    machine: Option<&str>,
) -> Option<AgentRow> {
    let agent = snapshot
        .agents
        .iter()
        .find(|agent| agent.pane_id == pane_id)?;
    let workspace = snapshot
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id == agent.workspace_id)?;
    let tab = snapshot.tabs.iter().find(|tab| tab.tab_id == agent.tab_id);
    let pane = snapshot
        .panes
        .iter()
        .find(|pane| pane.pane_id == agent.pane_id);
    let tab_count = snapshot
        .tabs
        .iter()
        .filter(|candidate| candidate.workspace_id == agent.workspace_id)
        .count();
    let tab_label = tab
        .filter(|tab| tab_count > 1 || tab.custom_label)
        .map(|tab| tab.label.as_str());
    let agent_label = agent
        .display_agent
        .as_deref()
        .or(agent.name.as_deref())
        .or(agent.agent.as_deref())
        .or(agent.title.as_deref());
    let labels = agent
        .state_labels
        .iter()
        .cloned()
        .collect::<HashMap<_, _>>();
    let tokens = agent.tokens.iter().cloned().collect::<HashMap<_, _>>();
    let state_text = labels
        .get(status_text(agent.agent_status))
        .map(String::as_str)
        .unwrap_or_else(|| sidebar_status_text(agent.agent_status));
    let canonical_agent = agent
        .agent
        .as_deref()
        .and_then(super::detect::parse_agent_label);
    let rows = tokens::agent_rows(
        &config.agents,
        tokens::AgentTokenContext {
            machine,
            workspace: &workspace.label,
            tab: tab_label,
            pane: agent
                .title
                .as_deref()
                .or_else(|| pane.and_then(|pane| pane.label.as_deref())),
            agent_label,
            terminal_title: agent.terminal_title.as_deref(),
            terminal_title_stripped: agent.terminal_title_stripped.as_deref(),
            canonical_agent,
            tokens: &tokens,
        },
        state_text,
    );
    Some(AgentRow {
        pane_id: agent.pane_id.clone(),
        status: agent.agent_status,
        focused: agent.focused,
        rows,
    })
}

pub(crate) fn render_agent_row(
    buffer: &mut Buffer,
    rect: Rect,
    row: &AgentRow,
    config: &super::ClientShellConfig,
) {
    let palette = &config.palette;
    let row_style = if row.focused {
        Style::default().bg(palette.active_row_bg)
    } else {
        Style::default()
    };
    let name_style = if row.focused {
        Style::default()
            .fg(palette.text)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(palette.subtext0)
            .add_modifier(Modifier::BOLD)
    };
    let status_style = Style::default().fg(status_color(row.status, palette));
    let secondary = Style::default().fg(palette.overlay0);
    let icon = (
        status_icon(row.status, config.status_indicators),
        Style::default().fg(status_color(row.status, palette)),
    );
    let rows = if row.rows.is_empty() {
        vec![vec![ResolvedToken {
            kind: ResolvedTokenKind::StateIcon,
            style: Default::default(),
        }]]
    } else {
        row.rows.clone()
    };
    for (index, tokens) in rows.iter().take(rect.height as usize).enumerate() {
        let indent = if index == 0 { 1 } else { 3 };
        let mut spans = vec![ratatui::text::Span::raw(" ".repeat(indent))];
        spans.extend(resolved_token_spans(
            tokens,
            icon,
            status_style,
            name_style,
            secondary,
            secondary,
            palette,
            rect.width.saturating_sub(indent as u16) as usize,
        ));
        Paragraph::new(Line::from(spans)).style(row_style).render(
            Rect::new(rect.x, rect.y + index as u16, rect.width, 1),
            buffer,
        );
    }
}

fn sidebar_status_text(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::Blocked => "blocked",
        AgentStatus::Done => "done",
        AgentStatus::Working => "working",
        AgentStatus::Waiting => "waiting",
        AgentStatus::Idle | AgentStatus::Unknown => "idle",
    }
}
