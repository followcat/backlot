//! The expanded sheet's cursor: three navigation/focus zones
//! (`Waiting`, `Hollywood`, `Studios`) cycled in screen order by
//! `Tab`/`BackTab`, cross-zone `↑↓` (`k`/`j`) traversal, `←→` chip and
//! frame/strip movement, and scroll windows. Selection is always by
//! stable `Target`, never by position.

use super::App;
use crate::hollywood::{self, LayerMode, Section, Target};

impl App {
    /// Active keyboard-navigation section: follows the current selection
    /// when present so direct `selected` assignments stay consistent.
    pub(super) fn hollywood_active_section(&self) -> Section {
        self.hollywood
            .selected
            .as_ref()
            .map(Section::of)
            .unwrap_or(self.hollywood.section)
    }

    /// Select `target`, update active section & per-section memory,
    /// reset `chip_index` when switching rows, and clamp `editing`.
    pub(crate) fn hollywood_select(&mut self, target: Target) {
        if self.hollywood.selected.as_ref() != Some(&target) {
            self.hollywood.chip_index = None;
        }
        self.hollywood.selected = Some(target);
        self.hollywood_remember_selected();
        self.hollywood_sync_editing();
    }

    /// `Tab` / `Shift+Tab`: cycle screen order
    /// `Waiting → Hollywood (compose armed) → Studios → Waiting`,
    /// skipping empty zones (`Waiting` when calm).
    pub(crate) fn hollywood_switch_zone(&mut self, forward: bool) {
        let current = self
            .hollywood
            .selected
            .as_ref()
            .and_then(Section::focus_of)
            .unwrap_or(self.hollywood.section);
        let next_two = match (current, forward) {
            (Section::Waiting, true) => [Section::Hollywood, Section::Studios],
            (Section::Hollywood, true) => [Section::Studios, Section::Waiting],
            (Section::Studios, true) => [Section::Waiting, Section::Hollywood],
            (Section::Waiting, false) => [Section::Studios, Section::Hollywood],
            (Section::Hollywood, false) => [Section::Waiting, Section::Studios],
            (Section::Studios, false) => [Section::Hollywood, Section::Waiting],
        };
        let band = self.hollywood_band();
        let waits = self.hollywood_waits();

        for zone in next_two {
            match zone {
                Section::Waiting => {
                    if let Some(target) = waits.iter().find(|t| band.contains(t)).cloned() {
                        let index = band.iter().position(|r| r == &target).unwrap_or(0);
                        self.hollywood_select(target);
                        self.hollywood_reveal_card(&band, index);
                        return;
                    }
                }
                Section::Hollywood => {
                    if self.has_hollywood_cfg
                        && let Some(index) = band.iter().position(|t| matches!(t, Target::Input))
                    {
                        self.hollywood_select(Target::Input);
                        self.hollywood.editing = true;
                        self.hollywood_reveal_card(&band, index);
                        return;
                    }
                    if !self.has_hollywood_cfg && band.iter().any(|t| matches!(t, Target::Setup)) {
                        self.hollywood_select(Target::Setup);
                        return;
                    }
                }
                Section::Studios => {
                    if self.hollywood_focus_studios() {
                        return;
                    }
                }
            }
        }
    }

    /// Focus the STUDIOS zone on the last selected studio, active main-pane
    /// studio, or first available frame.
    pub(crate) fn hollywood_focus_studios(&mut self) -> bool {
        let frames = self.hollywood_frames();
        let active_studio = self.hollywood.active_studio.clone().map(Target::Studio);
        let current_studio = self.studio.current.clone().map(Target::Studio);
        let target = self
            .hollywood
            .last_upper
            .clone()
            .filter(|t| frames.contains(t))
            .or_else(|| active_studio.filter(|t| frames.contains(t)))
            .or_else(|| current_studio.filter(|t| frames.contains(t)))
            .or_else(|| frames.first().cloned());
        let Some(target) = target else {
            return false;
        };
        self.hollywood_select(target);
        self.hollywood_keep_frame_visible();
        true
    }

    /// ← / → (`h` / `l`) in `Studios`: move the studio/new-studio cursor
    /// and keep it in the drawn grid or 1-row chip strip.
    pub(crate) fn hollywood_frame_move(&mut self, delta: isize) {
        if self.hollywood.selected.as_ref() == Some(&Target::Title) {
            return;
        }
        let frames = self.hollywood_frames();
        if frames.is_empty() {
            return;
        }
        let current = self
            .hollywood
            .selected
            .as_ref()
            .and_then(|t| frames.iter().position(|f| f == t))
            .unwrap_or(0);
        let next = (current as isize + delta).clamp(0, frames.len() as isize - 1) as usize;
        self.hollywood_select(frames[next].clone());
        self.hollywood_keep_frame_visible();
    }

    /// ← / → (`h` / `l`) on waiting items: step across action chips.
    pub(crate) fn hollywood_chip_move(&mut self, delta: isize) {
        let Some(selected) = self.hollywood.selected.clone() else {
            return;
        };
        let machines = self.panel_summaries();
        let status = hollywood::status_of(&self.view);
        let waits = hollywood::waiting_items(&machines, &status);
        let Some(wait) = waits.iter().find(|w| w.target() == selected) else {
            return;
        };
        let chips = hollywood::chips_of(wait);
        if chips.is_empty() {
            return;
        }
        let cur = self
            .hollywood
            .chip_index
            .unwrap_or_else(|| hollywood::default_chip_index(wait, &chips))
            .min(chips.len() - 1);
        let next = (cur as isize + delta).clamp(0, chips.len() as isize - 1) as usize;
        self.hollywood.chip_index = Some(next);
    }

    /// ↑ / ↓ (`k` / `j`) across all visible screen rows in Resting
    /// (`Waiting ↔ Hollywood ↔ Studios`), shifting focus and expansion
    /// seamlessly across zone boundaries.
    pub(crate) fn hollywood_row_move(&mut self, delta: isize) {
        let nav_band = self.hollywood_nav_band();
        let frames = self.hollywood_frames();
        if self.hollywood.selected.as_ref() == Some(&Target::Title) {
            if delta > 0 {
                if let Some(first) = nav_band.first().cloned() {
                    self.hollywood_select(first);
                    self.hollywood_reveal_card(&nav_band, 0);
                } else {
                    let _ = self.hollywood_focus_studios();
                }
            }
            return;
        }

        if self.hollywood_active_section() == Section::Studios {
            if delta < 0
                && let Some(last) = nav_band.last().cloned()
            {
                let idx = nav_band.len() - 1;
                self.hollywood_select(last);
                self.hollywood_reveal_card(&nav_band, idx);
            }
            return;
        }

        if nav_band.is_empty() {
            if delta > 0 {
                let _ = self.hollywood_focus_studios();
            }
            return;
        }
        match self
            .hollywood
            .selected
            .as_ref()
            .and_then(|t| nav_band.iter().position(|r| r == t))
        {
            Some(index) => {
                let raw = index as isize + delta;
                if raw >= nav_band.len() as isize {
                    if !frames.is_empty() {
                        let _ = self.hollywood_focus_studios();
                    }
                    return;
                }
                let next = raw.max(0) as usize;
                self.hollywood_select(nav_band[next].clone());
                self.hollywood_reveal_card(&nav_band, next);
            }
            None => {
                let next = if delta >= 0 { 0 } else { nav_band.len() - 1 };
                self.hollywood_select(nav_band[next].clone());
                self.hollywood_reveal_card(&nav_band, next);
            }
        }
    }

    /// Adjust `frame_scroll` so the selected studio/new-studio frame is
    /// visible in either the multi-row frame grid or the 1-row strip.
    pub(super) fn hollywood_keep_frame_visible(&mut self) {
        let machines = self.panel_summaries();
        let status = hollywood::status_of(&self.view);
        let band_rect = if self.area.0 == 0 || self.area.1 == 0 {
            ratatui::layout::Rect::new(0, 0, 120, 40)
        } else {
            self.call_sheet_band(ratatui::layout::Rect::new(0, 0, self.area.0, self.area.1))
        };
        let opts = hollywood::LayerOpts {
            agent: self.has_hollywood_cfg,
            mobile: self.area.0 <= self.shell.mobile_width_threshold,
            suppress_highlights: false,
            no_shadow: true,
        };
        self.hollywood.frame_scroll = hollywood::reveal_frame_scroll(
            band_rect,
            &machines,
            &status,
            &self.hollywood,
            self.hollywood.active_studio.as_ref(),
            Some(crate::project::LOCAL_LABEL),
            opts,
            &self.shell.palette,
        );
    }

    /// Keep the selected waiting row inside the drawn window.
    pub(super) fn hollywood_reveal_card(&mut self, band: &[Target], index: usize) {
        let waits = |slice: &[Target]| {
            slice
                .iter()
                .filter(|t| {
                    matches!(
                        t,
                        Target::Prompt(..)
                            | Target::Role(..)
                            | Target::Hollywood
                            | Target::Tell(_)
                            | Target::Upgrade(_)
                            | Target::Halted(_)
                            | Target::Decision(_)
                            | Target::Stage(..)
                            | Target::Card(_)
                            | Target::OutwardCard(..)
                            | Target::Ready(_)
                    )
                })
                .count()
        };
        if !matches!(
            band.get(index),
            Some(
                Target::Prompt(..)
                    | Target::Role(..)
                    | Target::Hollywood
                    | Target::Tell(_)
                    | Target::Upgrade(_)
                    | Target::Halted(_)
                    | Target::Decision(_)
                    | Target::Stage(..)
                    | Target::Card(_)
                    | Target::OutwardCard(..)
                    | Target::Ready(_)
            )
        ) {
            return;
        }
        let wait = waits(&band[..index]);
        let visible = self.hollywood_hits.viewport_rows.max(1);
        if wait < self.hollywood.scroll {
            self.hollywood.scroll = wait;
        } else if wait >= self.hollywood.scroll + visible {
            self.hollywood.scroll = wait + 1 - visible;
        }
    }

    /// Wheel/drag scroll of the waiting list: clamp the offset into the
    /// drawn metrics.
    pub(crate) fn hollywood_scroll_to(&mut self, start: usize) {
        let max = self
            .hollywood_hits
            .metrics
            .map(|m| m.max_offset_from_bottom)
            .unwrap_or(0);
        self.hollywood.scroll = start.min(max);
    }

    /// The frames or chip strip's rect as last drawn — the wheel's target.
    pub(crate) fn hollywood_frames_hit(&self) -> ratatui::layout::Rect {
        self.hollywood_hits.body
    }

    /// Leave the layer after a row focused a terminal: the panel
    /// collapses back to its strip so the focused pane stays visible.
    pub(crate) fn hollywood_leave(&mut self) {
        self.hollywood.mode = LayerMode::Collapsed;
        self.hollywood.editing = false;
        self.hollywood.history.reset_nav();
        self.hollywood.view = None;
        self.call_sheet_return = None;
        if matches!(
            self.focus.workspace,
            crate::focus::WorkspaceRegion::CallSheet(_)
        ) {
            self.set_workspace_focus(crate::focus::WorkspaceRegion::Terminal);
        }
    }
}
