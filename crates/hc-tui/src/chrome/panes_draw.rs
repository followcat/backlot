// Derived from herdr src/ui/panes.rs (Apache-2.0).

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    widgets::Borders,
};

use super::palette::Palette;
use super::panes::{LineCell, PaneInfo, SplitBorder};
use super::scroll::{ScrollMetrics, render_pane_scrollbar_buffer};
use crate::config::PaneBordersConfig;

fn line_touches_pane(x: u16, y: u16, info: &PaneInfo, pane_gaps: bool) -> bool {
    let rect = info.rect;
    if rect.width == 0 || rect.height == 0 {
        return false;
    }
    let right = rect.x.saturating_add(rect.width).saturating_sub(1);
    let bottom = rect.y.saturating_add(rect.height).saturating_sub(1);
    let in_rows = y >= rect.y && y <= bottom;
    let in_cols = x >= rect.x && x <= right;
    let own_border =
        (in_rows && (x == rect.x || x == right)) || (in_cols && (y == rect.y || y == bottom));

    if pane_gaps {
        return own_border;
    }

    let shared_right = rect.x.saturating_add(rect.width);
    let shared_bottom = rect.y.saturating_add(rect.height);
    own_border
        || (in_rows && x == shared_right)
        || (in_cols && y == shared_bottom)
        || (x == shared_right && y == shared_bottom)
}

fn line_cell_symbol(line: LineCell) -> &'static str {
    match (line.up, line.down, line.left, line.right) {
        (true, true, true, true) => "┼",
        (true, true, true, false) => "┤",
        (true, true, false, true) => "├",
        (true, false, true, true) => "┴",
        (false, true, true, true) => "┬",
        (true, true, false, false) | (true, false, false, false) | (false, true, false, false) => {
            "│"
        }
        (false, false, true, true) | (false, false, true, false) | (false, false, false, true) => {
            "─"
        }
        (false, true, false, true) => "┌",
        (false, true, true, false) => "┐",
        (true, false, false, true) => "└",
        (true, false, true, false) => "┘",
        _ => "",
    }
}

/// Everything the border cell map depends on: a hit means the cached
/// `Vec` replays without rebuilding or re-hashing the `HashMap`.
#[derive(PartialEq, Eq)]
struct BorderKey {
    area: Rect,
    pane_borders: PaneBordersConfig,
    pane_gaps: bool,
    panes: Vec<(Rect, Borders, bool)>,
    splits: Vec<SplitBorder>,
}

/// Cached border line cells — `(x, y, symbol, focused)` — for one
/// layout. Titles are NOT cached: their labels track live agent state.
#[derive(Default)]
pub(crate) struct PaneBorderCache {
    key: Option<BorderKey>,
    cells: Vec<(u16, u16, &'static str, bool)>,
}

/// `render_pane_borders` + `render_pane_border_titles` combined. `titles` maps
/// pane id → raw border label, formatted with the reference title helper.
/// The draw path uses `render_pane_borders_cached`; this stays as the
/// stateless wrapper for callers that hold no cache.
#[allow(dead_code)]
pub(crate) fn render_pane_borders(
    buffer: &mut Buffer,
    pane_borders: PaneBordersConfig,
    pane_gaps: bool,
    pane_infos: &[PaneInfo],
    split_borders: &[SplitBorder],
    titles: &dyn Fn(&str) -> Option<String>,
    palette: &Palette,
) {
    render_pane_borders_cached(
        &mut PaneBorderCache::default(),
        buffer,
        pane_borders,
        pane_gaps,
        pane_infos,
        split_borders,
        titles,
        palette,
    );
}

/// `render_pane_borders` through a reusable cache: an unchanged layout
/// replays the computed cell list instead of re-hashing every frame.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_pane_borders_cached(
    cache: &mut PaneBorderCache,
    buffer: &mut Buffer,
    pane_borders: PaneBordersConfig,
    pane_gaps: bool,
    pane_infos: &[PaneInfo],
    split_borders: &[SplitBorder],
    titles: &dyn Fn(&str) -> Option<String>,
    palette: &Palette,
) {
    if !pane_borders.draws_borders() || pane_infos.iter().all(|info| info.borders.is_empty()) {
        cache.key = None;
        return;
    }

    let key = BorderKey {
        area: buffer.area,
        pane_borders,
        pane_gaps,
        panes: pane_infos
            .iter()
            .map(|info| (info.rect, info.borders, info.is_focused))
            .collect(),
        splits: split_borders.to_vec(),
    };
    if cache.key.as_ref() != Some(&key) {
        let mut lines = std::collections::HashMap::<(u16, u16), LineCell>::new();
        for info in pane_infos {
            super::panes::add_pane_border_cells(&mut lines, info);
        }
        super::panes::add_split_border_cells(pane_gaps, split_borders, &mut lines);
        cache.cells = lines
            .iter()
            .filter_map(|(&(x, y), &line)| {
                let symbol = line_cell_symbol(line);
                if symbol.is_empty() {
                    return None;
                }
                let focused = pane_infos
                    .iter()
                    .any(|info| info.is_focused && line_touches_pane(x, y, info, pane_gaps));
                Some((x, y, symbol, focused))
            })
            .collect();
        cache.key = Some(key);
    }

    let area = buffer.area;
    for &(x, y, symbol, focused) in &cache.cells {
        if x < area.x
            || x >= area.x.saturating_add(area.width)
            || y < area.y
            || y >= area.y.saturating_add(area.height)
        {
            continue;
        }
        let cell = &mut buffer[(x, y)];
        cell.set_symbol(symbol);
        let color = if focused {
            palette.accent
        } else {
            palette.overlay0
        };
        cell.set_style(Style::default().fg(color));
    }

    for info in pane_infos {
        if !info.borders.contains(Borders::TOP) || info.rect.width <= 4 {
            continue;
        }
        let Some(label) = titles(&info.id) else {
            continue;
        };
        let Some(title) = pane_border_title(&label, info.rect.width) else {
            continue;
        };
        let y = info.rect.y;
        if y < area.y || y >= area.y.saturating_add(area.height) {
            continue;
        }
        let start_x = info.rect.x.saturating_add(1);
        let end_x = info
            .rect
            .x
            .saturating_add(info.rect.width)
            .saturating_sub(1)
            .min(area.x.saturating_add(area.width));
        if start_x >= end_x {
            continue;
        }
        let color = if info.is_focused {
            palette.accent
        } else {
            palette.overlay0
        };
        let mut style = Style::default().fg(color);
        if info.is_focused {
            style = style.add_modifier(Modifier::BOLD);
        }
        buffer.set_stringn(
            start_x,
            y,
            title,
            end_x.saturating_sub(start_x) as usize,
            style,
        );
    }
}

/// `render_pane_scrollbar` split from runtime access.
pub(crate) fn render_pane_scrollbar(
    buffer: &mut Buffer,
    metrics: Option<ScrollMetrics>,
    track: Option<Rect>,
    palette: &Palette,
    focused: bool,
) {
    let (Some(metrics), Some(track)) = (metrics, track) else {
        return;
    };
    render_pane_scrollbar_buffer(buffer, metrics, track, palette, focused);
}

fn pane_border_title(label: &str, pane_width: u16) -> Option<String> {
    let label = label.trim();
    if label.is_empty() || pane_width <= 4 {
        return None;
    }
    Some(format!(
        " {} ",
        super::text::truncate_end(label, pane_width.saturating_sub(4) as usize)
    ))
}
#[cfg(test)]
mod title_tests {
    use super::*;
    #[test]
    fn reference_border_labels_have_padding_and_display_width_truncation() {
        assert_eq!(
            pane_border_title(" pane-b ", 20).as_deref(),
            Some(" pane-b ")
        );
        assert_eq!(pane_border_title("abcdef", 8).as_deref(), Some(" abc… "));
        assert_eq!(pane_border_title("模块组织", 8).as_deref(), Some(" 模… "));
        assert_eq!(pane_border_title("x", 4), None);
    }
}
