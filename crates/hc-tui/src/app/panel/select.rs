//! The dock's selection mechanics: the row targets in draw order,
//! j/k and paging movement with scroll-to-visible, and the
//! collapse/fold answers for the selected block, role or fold.

use super::App;
use crate::chrome::scroll;
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
    /// The first actionable row's target — `j`/`k` never park on a
    /// `None` row, so the cursor starts on something real.
    pub(super) fn panel_select_first(&mut self) {
        let targets = self.panel_targets();
        if let Some(index) = targets.iter().position(|t| *t != Target::None) {
            self.panel.selected = Some(targets[index].clone());
            self.panel.selected_row = Some(index);
        }
    }

    /// j/k/arrows and paging: move the selection by `delta` rows and
    /// keep it visible. A block's frame rows share one `Studio`
    /// target — a move always lands on a different target, so the
    /// selection visibly steps. Moving clears an armed reopen.
    pub(super) fn panel_move(&mut self, delta: isize) {
        let machines = self.panel_summaries();
        let rows = panel::rows(
            &machines,
            |m| self.machine_stage_running(m),
            |m| self.machine_reachable(m),
            &self.panel,
            self.studio.current.as_ref(),
            self.panel_hits.body.height,
        );
        if rows.len() <= 1 {
            return;
        }
        let targets: Vec<Target> = rows.iter().map(|r| r.target()).collect();
        let mut index = self.panel_selected_index(&targets);
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
            match next {
                Some(next) => index = next,
                None => break,
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
        // Keep the selection inside the scrolled body (title is fixed).
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

    /// Fold or unfold one studio block by hand — `expanded_set`
    /// resolves the shape; this writes the opposite manual override.
    pub(super) fn panel_block_toggle(&mut self, studio: crate::studio::StudioKey) {
        let machines = self.panel_summaries();
        let rows = panel::rows(
            &machines,
            |m| self.machine_stage_running(m),
            |m| self.machine_reachable(m),
            &self.panel,
            self.studio.current.as_ref(),
            self.panel_hits.body.height,
        );
        let open = rows
            .iter()
            .any(|r| matches!(r, panel::Row::BlockTop { studio: s, .. } if s.key() == studio));
        if open {
            self.panel.expanded_blocks.remove(&studio);
            self.panel.collapsed_blocks.insert(studio.clone());
            // The block's identity changes with its shape — ride the
            // `╶` line so the next Space toggles it back open.
            self.panel.selected = Some(Target::Block(studio.clone()));
            self.panel.selected_row = self
                .panel_targets()
                .iter()
                .position(|t| matches!(t, Target::Block(s) if *s == studio));
        } else {
            self.panel.collapsed_blocks.remove(&studio);
            self.panel.expanded_blocks.insert(studio.clone());
            self.panel.selected = Some(Target::Studio(studio.clone()));
            self.panel.selected_row = self
                .panel_targets()
                .iter()
                .position(|t| *t == Target::Studio(studio.clone()));
        }
    }
}
