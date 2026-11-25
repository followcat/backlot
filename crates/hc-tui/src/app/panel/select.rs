//! The dock's selection mechanics: the row targets in draw order,
//! j/k and paging movement with scroll-to-visible, and the
//! collapse/fold answers for the selected block, role or fold.

use super::App;
use crate::panel::{self, Target};

impl App {
    /// The row targets as they would draw now — selection works on
    /// stable identities, never positions. The stacking decision
    /// needs the drawn body's height; pre-first-draw it is zero, and
    /// `rows` reads that as "no constraint" so every block opens.
    pub(crate) fn panel_targets(&self) -> Vec<Target> {
        let machines = self.panel_summaries();
        panel::rows(
            &machines,
            |m| self.machine_stage_running(m),
            |m| self.machine_reachable(m),
            &self.panel,
            self.panel_mode_at(self.area.0),
            self.studio.current.as_ref(),
            self.panel_hits.body.height,
        )
        .iter()
        .map(|r| r.target())
        .collect()
    }

    /// The selected row's index in `targets` — a vanished target
    /// reverts to the title (index 0). A `Studio`/`StageDown` target
    /// names several rows, so `selected_row` pins the occurrence the
    /// selection actually sits on; when rows shifted since (a publish
    /// above it), the nearest surviving occurrence keeps the place.
    pub(crate) fn panel_selected_index(&self, targets: &[Target]) -> usize {
        let Some(target) = self.panel.selected.as_ref() else {
            return 0;
        };
        if let Some(at) = self.panel.selected_row
            && targets.get(at) == Some(target)
        {
            return at;
        }
        targets
            .iter()
            .enumerate()
            .filter(|(_, t)| *t == target)
            .map(|(i, _)| i)
            .min_by_key(|i| i.abs_diff(self.panel.selected_row.unwrap_or(0)))
            .unwrap_or(0)
    }
    /// The body height the dock draws into — derived from `self.area`
    /// when undrawn so an opening jump (`Alt+L`) computes block
    /// stacking and scroll for the frame about to render.
    pub(super) fn panel_body_height(&self) -> u16 {
        let height = if self.panel_beside(self.area.0) {
            self.area.1
        } else {
            let layout = self.chrome_layout(self.area.0, self.area.1);
            self.panel_overlay_rect(layout.pane_surface, self.area.0)
                .height
        };
        height.saturating_sub(1 + u16::from(height >= 8))
    }

    /// The first actionable row's target — preferring the current
    /// studio's header so opening the dock does not fold the active
    /// studio away.
    pub(super) fn panel_select_first(&mut self) {
        let targets = self.panel_targets();
        self.panel.editing = false;
        let index = self
            .studio
            .current
            .as_ref()
            .and_then(|cur| targets.iter().position(|t| t.studio() == Some(cur)))
            .or_else(|| targets.iter().position(|t| *t != Target::None));
        if let Some(index) = index {
            self.panel.selected = Some(targets[index].clone());
            self.panel.selected_row = Some(index);
            self.panel_reveal_selected();
        }
    }

    /// The last actionable row's target (`Alt+L`): stay resting (never
    /// arm Compose), jump across multi-row targets to their head row,
    /// unfold the last studio if needed, and scroll so it stays visible.
    pub(super) fn panel_select_last(&mut self) {
        let machines = self.panel_summaries();
        let body_height = self.panel_body_height();
        let mut rows = self.panel_rows_at(&machines, body_height);
        self.panel.editing = false;
        self.panel.finish_confirm = None;
        self.panel.reset_history_nav();
        let last_row = |rows: &[panel::Row<'_>]| -> Option<(usize, Target)> {
            let targets: Vec<Target> = rows.iter().map(|r| r.target()).collect();
            let mut idx = targets.iter().rposition(|t| *t != Target::None)?;
            while idx > 0 && targets[idx - 1] == targets[idx] {
                idx -= 1;
            }
            Some((idx, targets[idx].clone()))
        };
        let Some((mut index, target)) = last_row(&rows) else {
            self.panel.selected = None;
            self.panel.selected_row = None;
            return;
        };
        let prev_studio = self
            .panel
            .selected
            .as_ref()
            .and_then(|t| t.studio().cloned());
        self.panel.selected = Some(target.clone());
        if target.studio() != prev_studio.as_ref() {
            rows = self.panel_rows_at(&machines, body_height);
            if let Some((new_idx, new_target)) = last_row(&rows) {
                index = new_idx;
                self.panel.selected = Some(new_target);
            }
        }
        self.panel.selected_row = Some(index);
        self.reveal_panel_line(&rows, index);
    }

    /// j/k/arrows and paging: move the selection by `delta` rows and
    /// keep it visible. Crossing into a folded studio unfolds it (and
    /// folds the studio left) while keeping the same target visible.
    pub(super) fn panel_move(&mut self, delta: isize) {
        let machines = self.panel_summaries();
        let body_h = self.panel_hits.body.height;
        let mut rows = self.panel_rows_at(&machines, body_h);
        if rows.len() <= 1 {
            return;
        }
        let mut targets: Vec<Target> = rows.iter().map(|r| r.target()).collect();
        let mut index = self.panel_selected_index(&targets);
        if delta == isize::MIN {
            if let Some(first) = targets.iter().position(|t| *t != Target::None) {
                index = first;
                let next_target = targets[index].clone();
                let prev = self
                    .panel
                    .selected
                    .as_ref()
                    .and_then(|t| t.studio().cloned());
                self.panel.selected = Some(next_target.clone());
                if next_target.studio() != prev.as_ref() {
                    rows = self.panel_rows_at(&machines, body_h);
                    targets = rows.iter().map(|r| r.target()).collect();
                    index = targets
                        .iter()
                        .position(|t| *t == next_target)
                        .unwrap_or(index);
                }
            }
        } else if delta == isize::MAX {
            if let Some(last) = targets.iter().rposition(|t| *t != Target::None) {
                let target = targets[last].clone();
                let prev = self
                    .panel
                    .selected
                    .as_ref()
                    .and_then(|t| t.studio().cloned());
                self.panel.selected = Some(target.clone());
                if target.studio() != prev.as_ref() {
                    rows = self.panel_rows_at(&machines, body_h);
                    targets = rows.iter().map(|r| r.target()).collect();
                }
                index = targets
                    .iter()
                    .rposition(|t| *t != Target::None)
                    .unwrap_or(last);
            }
        } else {
            for _ in 0..delta.unsigned_abs() {
                // Skip `None` rows — titles, dividers, block bottoms and
                // spacers are never a cursor stop (A2.2).
                let next = if delta > 0 {
                    (index + 1..targets.len())
                        .find(|&i| targets[i] != targets[index] && targets[i] != Target::None)
                } else {
                    (0..index)
                        .rev()
                        .find(|&i| targets[i] != targets[index] && targets[i] != Target::None)
                };
                let Some(next) = next else {
                    break;
                };
                let prev_studio = targets[index].studio().cloned();
                let next_target = targets[next].clone();
                let crossed = next_target.studio().cloned() != prev_studio;
                self.panel.selected = Some(next_target.clone());
                if crossed {
                    rows = self.panel_rows_at(&machines, body_h);
                    targets = rows.iter().map(|r| r.target()).collect();
                    index = targets
                        .iter()
                        .position(|t| *t == next_target)
                        .or_else(|| match &next_target {
                            Target::Block(key) => targets
                                .iter()
                                .position(|t| *t == Target::Studio(key.clone())),
                            _ => None,
                        })
                        .unwrap_or(next.min(targets.len().saturating_sub(1)));
                } else {
                    index = next;
                }
            }
        }
        // A person's extra lines share its target — the cursor lands
        // on the person's own row, the target's first contiguous row.
        while index > 0 && targets[index - 1] == targets[index] {
            index -= 1;
        }
        self.panel.selected = Some(targets[index].clone());
        self.panel.selected_row = Some(index);
        self.panel.finish_confirm = None;
        // Any row movement drops Compose — the `›` row rests with its
        // draft kept so `j`/`k`/`m`/`Esc` act on the dock at once.
        self.panel.editing = false;
        // Keep the selection inside the scrolled body (title is fixed).
        self.scroll_to_panel_row(&rows, index);
    }

    /// Move the Lot Dock selection directly to `target` (used when stepping or
    /// clicking entries from the Reader) and reveal it inside the dock body.
    pub(crate) fn panel_focus_target(&mut self, target: &Target) {
        let machines = self.panel_summaries();
        let rows = panel::rows(
            &machines,
            |m| self.machine_stage_running(m),
            |m| self.machine_reachable(m),
            &self.panel,
            self.panel_mode_at(self.area.0),
            self.studio.current.as_ref(),
            self.panel_hits.body.height,
        );
        let Some(index) = rows.iter().position(|r| &r.target() == target) else {
            self.panel.selected = Some(target.clone());
            self.panel.finish_confirm = None;
            self.panel.editing = false;
            return;
        };
        self.panel.selected = Some(target.clone());
        self.panel.selected_row = Some(index);
        self.panel.finish_confirm = None;
        self.panel.editing = false;
        let body_height = self.panel_hits.body.height.max(1);
        let heights: Vec<u16> = rows.iter().skip(1).map(|r| r.height()).collect();
        self.panel.scroll = scroll::list_scroll_start_to_reveal(
            &heights,
            &[],
            body_height,
            self.panel.scroll,
            index.saturating_sub(1),
        );
    }

    /// Direct scroll (wheel/drag): write `scroll`, then clamp the
    /// selection into the shown body so rendering does not snap back.
    pub(super) fn panel_scroll_to(&mut self, start: usize, viewport_rows: usize) {
        let targets = self.panel_targets();
        let viewport_rows = viewport_rows.max(1);
        // `scroll` is a line offset and rows are variable height: the
        // bound is the drawn metrics' line maximum, not a row count —
        // otherwise the wheel dies above the bottom on two-line rows.
        let max = self
            .panel_hits
            .metrics
            .map(|m| m.max_offset_from_bottom)
            .unwrap_or_else(|| targets.len().saturating_sub(viewport_rows));
        self.panel.scroll = start.min(max);
        let lo = (self.panel.scroll + 1).min(targets.len());
        let hi = (self.panel.scroll + viewport_rows)
            .min(targets.len().saturating_sub(1))
            .max(lo);
        let mut selected = self.panel_selected_index(&targets).clamp(lo, hi);
        // The clamp can land on a `None` row — walk to the nearest
        // actionable one inside the viewport instead.
        if targets.get(selected) == Some(&Target::None) {
            selected = (selected..=hi)
                .find(|&i| targets[i] != Target::None)
                .or_else(|| (lo..selected).rev().find(|&i| targets[i] != Target::None))
                .unwrap_or(selected);
        }
        self.panel.selected = targets
            .get(selected)
            .filter(|t| **t != Target::None)
            .cloned();
        self.panel.selected_row = self.panel.selected.as_ref().map(|_| selected);
        // A clamp that leaves the `›` row drops Compose; staying on
        // it keeps the edit.
        self.panel.editing &= matches!(self.panel.selected, Some(Target::Input(_)));
        if !self.panel.editing {
            self.panel.reset_history_nav();
        }
    }

    /// Space — the S2 meaning by row: a studio header folds or
    /// unfolds its block, a role or task row toggles its three-line
    /// drawer, the `✓ <n> accepted` line opens or shuts the fold.
    /// A stacked `╶` line and a strip row still fold/unfold their
    /// studio; anything else does nothing.
    pub(super) fn panel_collapse_selected(&mut self) {
        let targets = self.panel_targets();
        let index = self.panel_selected_index(&targets);
        let target = targets.get(index).cloned();
        match target {
            // A role/task row's drawer — `drawer` names one owner at
            // a time, so a second Space on another row moves it.
            Some(Target::Person(..) | Target::Task(..)) => {
                let target = target.unwrap();
                if self.panel.drawer.as_ref() == Some(&target) {
                    self.panel.drawer = None;
                } else {
                    self.panel.drawer = Some(target);
                }
            }
            Some(Target::Fold(studio)) => {
                if !self.panel.accepted_folds.remove(&studio) {
                    self.panel.accepted_folds.insert(studio);
                }
                self.panel_reveal_selected();
            }
            Some(Target::ClosedFold(studio)) => {
                if !self.panel.closed_folds.remove(&studio) {
                    self.panel.closed_folds.insert(studio);
                }
                self.panel_reveal_selected();
            }
            // The collapsed strip has no blocks — Space on a strip
            // row opens the dock like Enter does.
            Some(Target::Strip(_)) => self.panel_flip_mode(),
            Some(studio @ (Target::Studio(_) | Target::Block(_))) => {
                let id = studio.studio().unwrap().clone();
                self.panel_block_toggle(id);
            }
            _ => {}
        }
    }
}
