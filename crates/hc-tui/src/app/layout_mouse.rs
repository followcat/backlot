// Derived from herdr src/client/shell/mouse.rs and server/client_shell.rs (Apache-2.0).
use super::*;
use crate::operations::{Action, Change, Edit};
use ratatui::layout::{Direction, Position, Rect};

#[derive(Clone)]
pub(crate) struct SplitHit {
    pub rect: Rect,
    pub split: crate::layout::SplitBorder,
    pub root: crate::structure::Tree,
    pub tab: String,
}

impl App {
    pub(super) fn fence_chrome_core(&mut self) {
        let current = self.view.core.as_ref().map(|c| c.id.clone());
        if self.interaction_core != current {
            self.chrome_drag = None;
            self.tab_press = None;
            self.workspace_press = None;
            self.tab_dragged = false;
            self.tab_drag_index = None;
            self.interaction_core = current;
        }
    }

    pub(super) fn grab_split(&mut self, point: (u16, u16)) -> bool {
        let Some(hit) = self
            .hit_map
            .pane_splits
            .iter()
            .find(|h| h.rect.contains(Position::new(point.0, point.1)))
            .cloned()
        else {
            return false;
        };
        if self.projection.snapshot.focused_tab_id.as_ref() != Some(&hit.tab) {
            return true;
        }
        let Some(edit) = self.layout_edit(Change::Ratio {
            expected: hit.root.clone(),
            path: hit.split.path.clone(),
            ratio: hit.split.ratio,
        }) else {
            return true;
        };
        let pointer = if hit.split.direction == Direction::Horizontal {
            point.0
        } else {
            point.1
        };
        self.chrome_drag = Some(mouse::ChromeDrag::PaneSplit {
            grab: i32::from(hit.split.pos) - i32::from(pointer),
            hit: Box::new(hit),
            edit: Box::new(edit),
            last: None,
            core: self
                .view
                .core
                .as_ref()
                .map(|c| c.id.clone())
                .unwrap_or_default(),
        });
        true
    }
    pub(super) fn drag_split(&mut self, point: (u16, u16), finish: bool) {
        let Some(mouse::ChromeDrag::PaneSplit {
            hit,
            edit,
            grab,
            last,
            core,
        }) = &self.chrome_drag
        else {
            return;
        };
        if self.view.core.as_ref().is_none_or(|c| &c.id != core)
            || self.projection.snapshot.focused_tab_id.as_ref() != Some(&hit.tab)
        {
            self.chrome_drag = None;
            return;
        }
        let now = Instant::now();
        if !finish
            && last.is_some_and(|t| now.duration_since(t) < std::time::Duration::from_millis(33))
        {
            return;
        }
        let (pointer, origin, length) = if hit.split.direction == Direction::Horizontal {
            (point.0, hit.split.area.x, hit.split.area.width)
        } else {
            (point.1, hit.split.area.y, hit.split.area.height)
        };
        let ratio = ((i32::from(pointer) + grab - i32::from(origin)) as f32
            / f32::from(length.max(1)))
        .clamp(0.1, 0.9);
        let mut edit: Edit = (**edit).clone();
        if let Change::Ratio { ratio: target, .. } = &mut edit.change {
            *target = ratio;
        }
        if let Some(mouse::ChromeDrag::PaneSplit { last, .. }) = &mut self.chrome_drag {
            *last = Some(now);
        }
        self.submit_action(Action::Edit(edit));
    }
    pub(super) fn tab_drop_index(&self, point: (u16, u16)) -> Option<usize> {
        let snapshot = &self.projection.snapshot;
        let tabs = snapshot
            .tabs
            .iter()
            .filter(|t| Some(&t.workspace_id) == snapshot.focused_workspace_id.as_ref())
            .collect::<Vec<_>>();
        let visible = self
            .hit_map
            .tabs
            .iter()
            .filter_map(|(rect, id)| {
                tabs.iter()
                    .position(|t| &t.tab_id == id)
                    .map(|i| (i, *rect))
            })
            .collect::<Vec<_>>();
        let (first, rect) = *visible.first()?;
        if point.1 != rect.y {
            return None;
        }
        if self
            .hit_map
            .tab_scroll_left
            .contains(Position::new(point.0, point.1))
        {
            return Some(0);
        }
        if self
            .hit_map
            .tab_scroll_right
            .contains(Position::new(point.0, point.1))
        {
            return Some(tabs.len());
        }
        if point.0 <= rect.x {
            return Some(first);
        }
        for (index, rect) in &visible {
            if point.0 < rect.x + rect.width / 2 {
                return Some(*index);
            }
            if point.0 < rect.right() {
                return Some(index + 1);
            }
        }
        Some(visible.last()?.0 + 1)
    }
}

pub(super) fn split_hit_rect(
    split: &crate::layout::SplitBorder,
    pane_borders: bool,
    pane_gaps: bool,
    pane_frames: &[Rect],
) -> Option<Rect> {
    let hit = match (split.direction, pane_borders, pane_gaps) {
        (ratatui::layout::Direction::Horizontal, true, false) => {
            Rect::new(split.pos, split.area.y, 1, split.area.height)
        }
        (ratatui::layout::Direction::Horizontal, true, true) => {
            let start = split.pos.saturating_sub(1);
            Rect::new(
                start,
                split.area.y,
                split.pos.saturating_sub(start).saturating_add(1),
                split.area.height,
            )
        }
        (ratatui::layout::Direction::Horizontal, false, true) => Rect::new(
            split.pos.checked_sub(1)?,
            split.area.y,
            1,
            split.area.height,
        ),
        (ratatui::layout::Direction::Vertical, true, false) => {
            Rect::new(split.area.x, split.pos, split.area.width, 1)
        }
        (ratatui::layout::Direction::Vertical, true, true) => {
            let start = split.pos.saturating_sub(1);
            Rect::new(
                split.area.x,
                start,
                split.area.width,
                split.pos.saturating_sub(start).saturating_add(1),
            )
        }
        (ratatui::layout::Direction::Vertical, false, true) => {
            Rect::new(split.area.x, split.pos.checked_sub(1)?, split.area.width, 1)
        }
        (_, false, false) => return None,
    };
    if !pane_borders
        && pane_frames.iter().any(|pane| {
            hit.x < pane.right()
                && hit.right() > pane.x
                && hit.y < pane.bottom()
                && hit.bottom() > pane.y
        })
    {
        return None;
    }
    Some(hit)
}
