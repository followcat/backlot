// Derived from herdr src/client/shell/mobile.rs (Apache-2.0).

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use super::palette::Palette;
use super::render::{ClientMobileTarget, ShellHitMap, display_width, put_segment, put_text};
use super::snapshot::{ClientEndpointStatus, ClientShellSnapshot, ClientShellWorkspace};
use super::status::{AgentStatus, status_color, status_icon};

pub(super) const MOBILE_BUTTON_WIDTH: u16 = 10;

pub(crate) struct MobileItem {
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) background: Color,
    pub(crate) target: Option<ClientMobileTarget>,
}

impl MobileItem {
    pub(crate) fn section(label: impl Into<String>, palette: &Palette) -> Self {
        Self {
            lines: vec![Line::from(Span::styled(
                format!(" {} ", label.into()),
                Style::default()
                    .fg(palette.overlay1)
                    .bg(palette.panel_bg)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            ))],
            background: palette.panel_bg,
            target: None,
        }
    }

    pub(crate) fn action(
        label: &'static str,
        target: ClientMobileTarget,
        palette: &Palette,
    ) -> Self {
        Self {
            lines: vec![Line::from(Span::styled(
                label,
                Style::default()
                    .fg(palette.accent)
                    .bg(palette.panel_bg)
                    .add_modifier(Modifier::BOLD),
            ))],
            background: palette.panel_bg,
            target: Some(target),
        }
    }
}

pub(crate) fn render_mobile_header(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: &ClientShellSnapshot,
    config: &super::ClientShellConfig,
    hits: &mut ShellHitMap,
) {
    if area.is_empty() {
        return;
    }
    let palette = &config.palette;
    buffer.set_style(area, Style::default().bg(palette.panel_bg));
    let button_width = MOBILE_BUTTON_WIDTH.min(area.width);
    let button = Rect::new(
        area.right().saturating_sub(button_width),
        area.y,
        button_width,
        area.height,
    );
    hits.mobile_switch = button;
    let status_width = button.x.saturating_sub(area.x).saturating_sub(1);
    let status = Rect::new(area.x, area.y, status_width, area.height);
    render_header_status(buffer, status, snapshot, config);
    render_header_button(buffer, button, snapshot, config);
}

fn render_header_status(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: &ClientShellSnapshot,
    config: &super::ClientShellConfig,
) {
    if area.is_empty() {
        return;
    }
    let palette = &config.palette;
    let Some(workspace) = snapshot.focused_workspace_id.as_deref().and_then(|id| {
        snapshot
            .workspaces
            .iter()
            .find(|workspace| workspace.workspace_id == id)
    }) else {
        put_text(
            buffer,
            area.x,
            area.y,
            area.width,
            " no workspace",
            Style::default().fg(palette.text).bg(palette.panel_bg),
        );
        return;
    };
    let tab_status = compact_tab_status(snapshot, workspace);
    let tab_width = display_width(&tab_status).saturating_add(1).min(area.width);
    let name_width = area.width.saturating_sub(tab_width);
    put_text(
        buffer,
        area.x,
        area.y,
        name_width.min(3),
        &format!(
            " {} ",
            status_icon(workspace.agent_status, config.status_indicators)
        ),
        Style::default()
            .fg(status_color(workspace.agent_status, palette))
            .bg(palette.panel_bg),
    );
    put_text(
        buffer,
        area.x.saturating_add(3),
        area.y,
        name_width.saturating_sub(3),
        &super::text::truncate_end(&workspace.label, usize::from(name_width.saturating_sub(4))),
        Style::default()
            .fg(palette.text)
            .bg(palette.panel_bg)
            .add_modifier(Modifier::BOLD),
    );
    put_text(
        buffer,
        area.right().saturating_sub(tab_width).saturating_add(1),
        area.y,
        tab_width.saturating_sub(1),
        &tab_status,
        Style::default().fg(palette.overlay1).bg(palette.panel_bg),
    );
    if area.height > 1 {
        render_agent_summary(
            buffer,
            Rect::new(area.x, area.y + 1, area.width, 1),
            snapshot,
            config,
        );
    }
}

fn render_header_button(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: &ClientShellSnapshot,
    config: &super::ClientShellConfig,
) {
    if area.is_empty() {
        return;
    }
    let palette = &config.palette;
    buffer.set_style(area, Style::default().bg(palette.surface0));
    for y in area.y..area.bottom() {
        put_text(
            buffer,
            area.x,
            y,
            1,
            "│",
            Style::default()
                .fg(palette.surface_dim)
                .bg(palette.surface0),
        );
    }
    let label_y = if area.height > 1 { area.y + 1 } else { area.y };
    let label = "switch";
    let label_width = display_width(label);
    put_text(
        buffer,
        area.x
            .saturating_add(1)
            .saturating_add(area.width.saturating_sub(1 + label_width) / 2),
        label_y,
        area.width.saturating_sub(1),
        label,
        Style::default()
            .fg(palette.text)
            .bg(palette.surface0)
            .add_modifier(Modifier::BOLD),
    );
    if snapshot
        .agents
        .iter()
        .any(|agent| agent.agent_status == AgentStatus::Blocked)
    {
        put_text(
            buffer,
            area.right().saturating_sub(1),
            area.y,
            1,
            status_icon(AgentStatus::Blocked, config.status_indicators),
            Style::default().fg(palette.red).bg(palette.surface0),
        );
    }
}

pub(super) fn mobile_endpoint_state(status: ClientEndpointStatus) -> &'static str {
    match status {
        ClientEndpointStatus::Connecting => "connecting",
        ClientEndpointStatus::Online => "online",
        ClientEndpointStatus::Reconnecting => "reconnecting",
        ClientEndpointStatus::Attention => "attention",
        ClientEndpointStatus::Disabled => "disabled",
    }
}

pub(super) fn compact_tab_status(
    snapshot: &ClientShellSnapshot,
    workspace: &ClientShellWorkspace,
) -> String {
    let tabs = snapshot
        .tabs
        .iter()
        .filter(|tab| tab.workspace_id == workspace.workspace_id)
        .collect::<Vec<_>>();
    let active = tabs
        .iter()
        .position(|tab| tab.tab_id == workspace.active_tab_id)
        .unwrap_or(0);
    let label = tabs
        .get(active)
        .map(|tab| tab.label.as_str())
        .unwrap_or("1");
    if tabs.len() <= 1 {
        format!("tab {label}")
    } else {
        format!("tab {label} · {}/{}", active + 1, tabs.len())
    }
}

fn render_agent_summary(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: &ClientShellSnapshot,
    config: &super::ClientShellConfig,
) {
    let counts = [
        (AgentStatus::Blocked, "blocked"),
        (AgentStatus::Done, "done"),
        (AgentStatus::Working, "working"),
        (AgentStatus::Idle, "idle"),
    ]
    .map(|(status, label)| {
        (
            status,
            label,
            snapshot
                .agents
                .iter()
                .filter(|agent| agent.agent_status == status)
                .count(),
        )
    });
    let total = counts.iter().map(|(_, _, count)| count).sum::<usize>();
    let pending = counts[..3].iter().map(|(_, _, count)| count).sum::<usize>();
    if total == 0 {
        put_text(
            buffer,
            area.x,
            area.y,
            area.width,
            " no agents",
            Style::default()
                .fg(config.palette.overlay1)
                .bg(config.palette.panel_bg),
        );
        return;
    }
    if pending == 0 {
        put_text(
            buffer,
            area.x,
            area.y,
            area.width,
            " all idle",
            Style::default()
                .fg(config.palette.overlay1)
                .bg(config.palette.panel_bg),
        );
        return;
    }

    let mut x = area.x.saturating_add(1);
    let mut shown = 0usize;
    let mut omitted = false;
    for (status, label, count) in counts {
        if count == 0 {
            continue;
        }
        let symbol = match (config.status_indicators, status) {
            (crate::config::StatusIndicatorStyle::Dots, AgentStatus::Blocked) => Some("◉"),
            (crate::config::StatusIndicatorStyle::Dots, AgentStatus::Done) => Some("●"),
            (crate::config::StatusIndicatorStyle::Dots, _) => None,
            _ => Some(status_icon(status, config.status_indicators)),
        };
        let text = symbol.map_or_else(
            || format!("{count} {label}"),
            |symbol| format!("{symbol} {count} {label}"),
        );
        let separator = if shown == 0 { "" } else { " · " };
        let needed = display_width(separator).saturating_add(display_width(&text));
        if area.right().saturating_sub(x) < needed {
            omitted = true;
            break;
        }
        if !separator.is_empty() {
            x = put_segment(
                buffer,
                x,
                area.y,
                area.right(),
                separator,
                Style::default()
                    .fg(config.palette.overlay0)
                    .bg(config.palette.panel_bg),
            );
        }
        let color = if shown == 0 {
            match status {
                AgentStatus::Done => config.palette.blue,
                _ => status_color(status, &config.palette),
            }
        } else {
            config.palette.overlay1
        };
        x = put_segment(
            buffer,
            x,
            area.y,
            area.right(),
            &text,
            Style::default()
                .fg(color)
                .bg(config.palette.panel_bg)
                .add_modifier(if shown == 0 {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        );
        shown += 1;
    }
    if omitted && area.right().saturating_sub(x) >= 2 {
        put_text(
            buffer,
            x,
            area.y,
            2,
            " …",
            Style::default()
                .fg(config.palette.overlay0)
                .bg(config.palette.panel_bg),
        );
    }
}
