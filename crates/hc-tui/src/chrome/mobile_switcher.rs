// Derived from herdr src/client/shell/mobile.rs (Apache-2.0).

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Clear, Paragraph, Widget},
};

use super::mobile::MOBILE_BUTTON_WIDTH;
use super::mobile_items::mobile_items;
use super::navigate::WorkspaceNavigationTarget;
use super::render::{ClientMobileTarget, ShellHitMap, put_text};
use super::snapshot::{ClientEndpointId, ClientShellEndpoint, ClientShellSnapshot};

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_mobile_switcher(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: &ClientShellSnapshot,
    endpoints: &[ClientShellEndpoint],
    active_endpoint_id: &ClientEndpointId,
    config: &super::ClientShellConfig,
    selected_workspace_id: Option<&WorkspaceNavigationTarget>,
    scroll: &mut usize,
    reveal_workspace: &mut bool,
    hits: &mut ShellHitMap,
) {
    if area.is_empty() {
        return;
    }
    let palette = &config.palette;
    Clear.render(area, buffer);
    buffer.set_style(area, Style::default().bg(palette.panel_bg));
    hits.mobile_switch = Rect::default();
    if area.height <= 2 {
        *scroll = 0;
        put_text(
            buffer,
            area.x,
            area.y,
            area.width,
            &"─".repeat(usize::from(area.width)),
            Style::default()
                .fg(palette.surface_dim)
                .bg(palette.panel_bg),
        );
        return;
    }
    let header_height = area.height.min(2);
    let close_width = MOBILE_BUTTON_WIDTH.min(area.width);
    let close = Rect::new(
        area.right().saturating_sub(close_width),
        area.y,
        close_width,
        header_height,
    );
    hits.mobile_close = close;
    put_text(
        buffer,
        area.x,
        area.y,
        close.x.saturating_sub(area.x),
        " switch",
        Style::default()
            .fg(palette.text)
            .bg(palette.panel_bg)
            .add_modifier(Modifier::BOLD),
    );
    render_close_button(buffer, close, palette);
    let rule_y = area.y + header_height;
    put_text(
        buffer,
        area.x,
        rule_y,
        area.width,
        &"─".repeat(usize::from(area.width)),
        Style::default()
            .fg(palette.surface_dim)
            .bg(palette.panel_bg),
    );
    let viewport = Rect::new(
        area.x,
        rule_y.saturating_add(1),
        area.width,
        area.height.saturating_sub(header_height + 1),
    );
    if viewport.is_empty() {
        *scroll = 0;
        return;
    }

    let items = mobile_items(
        snapshot,
        endpoints,
        active_endpoint_id,
        config,
        selected_workspace_id,
        viewport.width.saturating_sub(1),
    );
    let total_rows = items.iter().map(|item| item.lines.len()).sum::<usize>();
    let max_scroll = total_rows.saturating_sub(usize::from(viewport.height));
    *scroll = (*scroll).min(max_scroll);
    if *reveal_workspace {
        if let Some(selected_workspace_id) = selected_workspace_id {
            let mut start = 0usize;
            for item in &items {
                let end = start.saturating_add(item.lines.len());
                if let Some(ClientMobileTarget::Workspace {
                    endpoint_id,
                    workspace_id,
                }) = &item.target
                    && selected_workspace_id.matches(endpoint_id, workspace_id.as_str())
                {
                    if start < *scroll {
                        *scroll = start;
                    } else if end > (*scroll).saturating_add(usize::from(viewport.height)) {
                        *scroll = end
                            .saturating_sub(usize::from(viewport.height))
                            .min(max_scroll);
                    }
                    break;
                }
                start = end;
            }
        }
        *reveal_workspace = false;
    }
    hits.mobile_max_scroll = max_scroll;
    let content = if viewport.width > 1 {
        Rect::new(
            viewport.x + 1,
            viewport.y,
            viewport.width - 1,
            viewport.height,
        )
    } else {
        Rect::default()
    };
    if max_scroll > 0 {
        render_left_scrollbar(buffer, viewport, total_rows, *scroll, palette);
    }
    if content.is_empty() {
        return;
    }

    let viewport_start = *scroll;
    let viewport_end = viewport_start.saturating_add(usize::from(viewport.height));
    let mut document_row = 0usize;
    for item in items {
        let item_start = document_row;
        let item_end = item_start.saturating_add(item.lines.len());
        let visible_start = item_start.max(viewport_start);
        let visible_end = item_end.min(viewport_end);
        if visible_start < visible_end {
            let y = viewport.y + u16::try_from(visible_start - viewport_start).unwrap_or(u16::MAX);
            let height = u16::try_from(visible_end - visible_start).unwrap_or(u16::MAX);
            let rect = Rect::new(content.x, y, content.width, height);
            buffer.set_style(rect, Style::default().bg(item.background));
            for row in visible_start..visible_end {
                let line = item.lines[row - item_start].clone();
                Paragraph::new(line).render(
                    Rect::new(
                        content.x,
                        viewport.y + u16::try_from(row - viewport_start).unwrap_or(u16::MAX),
                        content.width,
                        1,
                    ),
                    buffer,
                );
            }
            if let Some(target) = item.target {
                hits.mobile_targets.push((rect, target));
            }
        }
        document_row = item_end;
    }
}

fn render_close_button(buffer: &mut Buffer, area: Rect, palette: &super::palette::Palette) {
    if area.is_empty() {
        return;
    }
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
    let label_width = 5;
    let label_x = area
        .x
        .saturating_add(1)
        .saturating_add(area.width.saturating_sub(1 + label_width) / 2);
    put_text(
        buffer,
        label_x,
        area.y,
        area.width.saturating_sub(1),
        "close",
        Style::default()
            .fg(palette.overlay1)
            .bg(palette.surface0)
            .add_modifier(Modifier::BOLD),
    );
    if area.height > 1 {
        put_text(
            buffer,
            area.x.saturating_add(area.width / 2),
            area.y + 1,
            1,
            "×",
            Style::default()
                .fg(palette.text)
                .bg(palette.surface0)
                .add_modifier(Modifier::BOLD),
        );
    }
}

fn render_left_scrollbar(
    buffer: &mut Buffer,
    viewport: Rect,
    total_rows: usize,
    scroll: usize,
    palette: &super::palette::Palette,
) {
    let track = Rect::new(viewport.x, viewport.y, 1, viewport.height);
    let metrics = super::scroll::ScrollMetrics {
        offset_from_bottom: total_rows.saturating_sub(scroll + usize::from(viewport.height)),
        max_offset_from_bottom: total_rows.saturating_sub(usize::from(viewport.height)),
        viewport_rows: usize::from(viewport.height).min(total_rows),
    };
    super::scroll::render_scrollbar_buffer(
        buffer,
        metrics,
        track,
        palette.surface_dim,
        palette.overlay0,
        "▕",
    );
}
