//! Keeping the selected row and active studio visible across lot
//! folding transitions (`panel_reveal_selected`, `panel_reveal_studio`,
//! `panel_block_toggle`).

use super::App;
use crate::chrome::scroll;
use crate::panel::data::MachineSummary;
use crate::panel::{self, Target};
use crate::studio::StudioKey;

impl App {
    /// Body height available for the dock list — falls back to `self.area`
    /// before the first draw completes.
    pub(super) fn effective_panel_body_height(&self) -> u16 {
        let h = if self.panel_hits.body.height > 0 {
            self.panel_hits.body.height
        } else {
            self.panel_body_height()
        };
        h.max(1)
    }

    /// Build panel rows for `machines` at an explicit `body_height`.
    pub(super) fn panel_rows_at<'a>(
        &self,
        machines: &'a [MachineSummary],
        body_height: u16,
    ) -> Vec<panel::Row<'a>> {
        panel::rows(
            machines,
            |m| self.machine_stage_running(m),
            |m| self.machine_reachable(m),
            &self.panel,
            self.panel_mode_at(self.area.0),
            self.studio.current.as_ref(),
            body_height,
        )
    }

    /// Scroll the dock body just enough to keep `line` visible inside the
    /// current scroll viewport (`line` is an index into `rows`).
    pub(super) fn scroll_to_panel_row(&mut self, rows: &[panel::Row<'_>], line: usize) {
        let body_height = self.effective_panel_body_height();
        let heights: Vec<u16> = rows.iter().skip(1).map(|r| r.height()).collect();
        self.panel.scroll = scroll::list_scroll_start_to_reveal(
            &heights,
            &[],
            body_height,
            self.panel.scroll,
            line.saturating_sub(1),
        );
    }

    /// Scroll `line` into view using `BlockBottom` when sitting on a
    /// block's last interior row so the frame footer stays visible too.
    pub(super) fn reveal_panel_line(&mut self, rows: &[panel::Row<'_>], index: usize) {
        let body_height = self.effective_panel_body_height();
        let reveal = if body_height >= 2
            && matches!(rows.get(index + 1), Some(panel::Row::BlockBottom(_)))
        {
            index + 1
        } else {
            index
        };
        self.scroll_to_panel_row(rows, reveal);
    }

    /// Keep `panel.selected_row` and `panel.scroll` aligned with the
    /// current `panel.selected` after a studio fold/unfold or toggle.
    pub(crate) fn panel_reveal_selected(&mut self) {
        let machines = self.panel_summaries();
        let rows = self.panel_rows_at(&machines, self.panel_hits.body.height);
        let targets: Vec<Target> = rows.iter().map(|r| r.target()).collect();
        let Some(target) = self.panel.selected.as_ref() else {
            self.panel.selected_row = None;
            return;
        };
        let mut index = self.panel_selected_index(&targets);
        if targets.get(index) != Some(target) {
            return;
        }
        while index > 0 && targets[index - 1] == targets[index] {
            index -= 1;
        }
        self.panel.selected_row = Some(index);
        self.reveal_panel_line(&rows, index);
    }

    /// Scroll the dock body to keep `key`'s block visible when external
    /// studio stepping switches `studio.current`.
    pub(crate) fn panel_reveal_studio(&mut self, key: &StudioKey) {
        if !self.panel.open() {
            return;
        }
        let machines = self.panel_summaries();
        let rows = self.panel_rows_at(&machines, self.panel_hits.body.height);
        let Some(top) = rows.iter().position(|r| {
            matches!(
                r,
                panel::Row::BlockTop { studio, .. } | panel::Row::Stacked(studio)
                    if &studio.key() == key
            )
        }) else {
            return;
        };
        let bottom = rows[top..]
            .iter()
            .position(|r| {
                matches!(
                    r,
                    panel::Row::BlockBottom(studio) | panel::Row::Stacked(studio)
                        if &studio.key() == key
                )
            })
            .map(|off| top + off)
            .unwrap_or(top);
        self.scroll_to_panel_row(&rows, bottom);
        self.scroll_to_panel_row(&rows, top);
    }

    /// Fold or unfold one studio block by hand — manual overrides win;
    /// this writes the opposite override and keeps the row visible.
    pub(super) fn panel_block_toggle(&mut self, studio: StudioKey) {
        let machines = self.panel_summaries();
        let rows = self.panel_rows_at(&machines, self.panel_hits.body.height);
        let open = rows
            .iter()
            .any(|r| matches!(r, panel::Row::BlockTop { studio: s, .. } if s.key() == studio));
        if open {
            self.panel.expanded_blocks.remove(&studio);
            self.panel.collapsed_blocks.insert(studio.clone());
            self.panel.selected = Some(Target::Block(studio));
        } else {
            self.panel.collapsed_blocks.remove(&studio);
            self.panel.expanded_blocks.insert(studio.clone());
            self.panel.selected = Some(Target::Studio(studio));
        }
        self.panel_reveal_selected();
    }
}
