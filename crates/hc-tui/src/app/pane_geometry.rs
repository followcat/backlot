// Derived from herdr src/client/shell/composition.rs and src/layout.rs (Apache-2.0).
use super::*;
use crate::chrome::panes::{PaneInfo, SplitBorder};
use ratatui::layout::Rect;

pub(super) fn focused_tab_count(app: &App) -> usize {
    app.projection
        .snapshot
        .focused_workspace_id
        .as_ref()
        .map(|id| {
            app.projection
                .snapshot
                .tabs
                .iter()
                .filter(|t| &t.workspace_id == id)
                .count()
        })
        .unwrap_or(0)
}

pub(super) fn pane_infos(app: &App, surface: Rect) -> Vec<PaneInfo> {
    let tab = app
        .projection
        .snapshot
        .focused_tab_id
        .as_ref()
        .and_then(|id| app.projection.layouts.get(id));
    let infos = if let Some(zoomed) = tab.and_then(|t| t.zoomed.as_ref()) {
        vec![PaneInfo {
            id: zoomed.clone(),
            rect: surface,
            borders: Default::default(),
            is_focused: true,
        }]
    } else if let Some(root) = tab.and_then(|t| t.root.as_ref()) {
        let (layout, ids) = root.layout(app.focused.as_deref());
        layout
            .panes(surface)
            .into_iter()
            .map(|info| PaneInfo {
                id: ids[&info.id].clone(),
                rect: info.rect,
                borders: info.borders,
                is_focused: info.is_focused,
            })
            .collect()
    } else {
        vec![PaneInfo {
            id: app.focused.clone().unwrap_or_default(),
            rect: surface,
            borders: Default::default(),
            is_focused: true,
        }]
    };
    crate::chrome::panes::apply_pane_chrome(
        infos,
        app.shell.pane_borders,
        app.shell.pane_gaps,
        app.shell.pane_outer_borders,
    )
}

pub(super) fn split_borders(app: &App, surface: Rect) -> Vec<SplitBorder> {
    let Some(tab) = app
        .projection
        .snapshot
        .focused_tab_id
        .as_ref()
        .and_then(|id| app.projection.layouts.get(id))
    else {
        return vec![];
    };
    if tab.zoomed.is_some() {
        return vec![];
    }
    let Some(root) = &tab.root else {
        return vec![];
    };
    root.layout(app.focused.as_deref())
        .0
        .splits(surface)
        .into_iter()
        .map(|s| SplitBorder {
            direction: s.direction,
            pos: s.pos,
            area: s.area,
        })
        .collect()
}

pub(super) fn inner_for(
    pane: &pane::Pane,
    info: &PaneInfo,
    scrollbars: bool,
) -> (
    Rect,
    Option<Rect>,
    Option<crate::chrome::scroll::ScrollMetrics>,
) {
    let inner = crate::chrome::panes::pane_inner_rect(info.rect, info.borders);
    let scroll = pane.vt_ref().and_then(|vt| vt.scrollbar()).map(|state| {
        let maximum = state.total.saturating_sub(state.len) as usize;
        crate::chrome::scroll::ScrollMetrics {
            offset_from_bottom: maximum.saturating_sub(state.offset as usize),
            max_offset_from_bottom: maximum,
            viewport_rows: state.len as usize,
        }
    });
    let alternate = pane
        .vt_ref()
        .and_then(|vt| vt.alt_screen().ok())
        .unwrap_or(false);
    let (inner, track) =
        crate::chrome::panes::stable_scrollbar_gutter(inner, scrollbars, alternate, scroll);
    (inner, track, scroll)
}

pub(crate) fn pane_inner(app: &mut App, cols: u16, rows: u16) -> Rect {
    let layout = app.shell.layout_config().layout(
        cols,
        rows,
        app.shell_state.sidebar_collapsed,
        focused_tab_count(app),
        app.sidebar_width,
    );
    let infos = pane_infos(app, layout.pane_surface);
    let info = infos.iter().find(|p| p.is_focused).unwrap_or(&infos[0]);
    inner_for(&app.pane, info, app.shell.pane_scrollbars).0
}
