//! W1's machines-tree rows: the studios a machine's ledger lists when
//! its stage is down (`stage down`), the folded `finished (n)` group, and
//! the machine's one `unreachable` line. Split from the tree renderer to
//! keep each file small.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::palette::Palette;
use super::render::{
    ShellHitMap, StudioRowAct, StudioRowHit, display_width, put_right_text, put_text,
};
use super::snapshot::{ClientEndpointId, ClientShellEndpoint};
use super::studio_tree::{Row as StudioRow, StudioKind};

/// Whether the endpoint is `machine` — `None` is this core.
pub(super) fn endpoint_is_machine(endpoint: &ClientEndpointId, machine: Option<&str>) -> bool {
    match (endpoint, machine) {
        (ClientEndpointId::Local, None) => true,
        (ClientEndpointId::Remote(name), Some(machine)) => name == machine,
        _ => false,
    }
}

/// One W1 machines-tree row: a ledger studio (`stage down` tag), the
/// `finished (n)` group, or the machine's one `unreachable` line.
pub(super) fn render_studio_row(
    buffer: &mut Buffer,
    rect: Rect,
    palette: &Palette,
    endpoint: &ClientShellEndpoint,
    current: Option<&crate::studio::StudioKey>,
    row: &StudioRow,
    hits: &mut ShellHitMap,
) {
    let dim = Style::default().fg(palette.overlay0);
    let current_here = |id: &str| {
        current.is_some_and(|key| {
            endpoint_is_machine(&endpoint.endpoint_id, key.machine.as_deref()) && key.id == id
        })
    };
    let highlight = |buffer: &mut Buffer, id: &str| {
        if current_here(id) && !rect.is_empty() {
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
    };
    let indent = 3u16;
    match row {
        StudioRow::Unreachable => put_text(
            buffer,
            rect.x.saturating_add(indent),
            rect.y,
            rect.width.saturating_sub(indent),
            "× unreachable",
            dim,
        ),
        StudioRow::FinishedFold { count, open } => {
            put_text(
                buffer,
                rect.x.saturating_add(indent),
                rect.y,
                rect.width.saturating_sub(indent),
                &format!("{} finished ({count})", if *open { "▾" } else { "▸" }),
                dim,
            );
            hits.studio_rows.push(StudioRowHit {
                rect,
                endpoint_id: endpoint.endpoint_id.clone(),
                act: StudioRowAct::ToggleFinished,
            });
        }
        StudioRow::Studio { id, kind } => {
            highlight(buffer, id);
            let (glyph, style) = match kind {
                StudioKind::StageDown => ("◌", dim),
                StudioKind::Live => ("·", Style::default().fg(palette.subtext0)),
            };
            let tag = (*kind == StudioKind::StageDown).then_some("stage down");
            let tag_width = tag.map_or(0, |tag| display_width(tag) + 1);
            let id_area = rect.width.saturating_sub(indent + 2 + tag_width);
            put_text(
                buffer,
                rect.x.saturating_add(indent),
                rect.y,
                1,
                glyph,
                style,
            );
            put_text(
                buffer,
                rect.x.saturating_add(indent + 2),
                rect.y,
                id_area,
                &super::text::truncate_end(id, id_area as usize),
                if *kind == StudioKind::Live {
                    Style::default().fg(palette.text)
                } else {
                    dim
                },
            );
            if let Some(tag) = tag {
                put_right_text(buffer, rect, rect.y, tag, dim);
            }
            if *kind == StudioKind::StageDown {
                hits.studio_rows.push(StudioRowHit {
                    rect,
                    endpoint_id: endpoint.endpoint_id.clone(),
                    act: StudioRowAct::StartStage,
                });
            }
        }
        StudioRow::Finished { id } => {
            highlight(buffer, id);
            put_text(
                buffer,
                rect.x.saturating_add(indent),
                rect.y,
                rect.width.saturating_sub(indent),
                &super::text::truncate_end(
                    &format!("✓ {id}"),
                    rect.width.saturating_sub(indent) as usize,
                ),
                dim,
            );
            hits.studio_rows.push(StudioRowHit {
                rect,
                endpoint_id: endpoint.endpoint_id.clone(),
                act: StudioRowAct::OpenFinished { id: id.clone() },
            });
        }
    }
}
