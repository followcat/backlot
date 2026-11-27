//! The expanded sheet's cursor: two independent navigation sections
//! (`Upper` studio frames vs `Lower` waiting/exchange/input rows)
//! switched with `Tab`/`BackTab`, per-section `↑↓←→` movement,
//! action-chip cursoring, and scroll windows. Selection is always by
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

    /// `Tab` / `Shift+Tab`: 3-zone Call Sheet cycle skipping empty zones:
    /// `Compose (›) → Waiting/Exchange (first wait, else newest exchange) → Studios (Upper)`.
    /// `Shift+Tab` (`forward == false`) reverses the ring.
    pub(crate) fn hollywood_switch_zone(&mut self, forward: bool) {
        #[derive(Clone, Copy)]
        enum Zone {
            Compose,
            Waiting,
            Studios,
        }
        let current = match self.hollywood.selected.as_ref() {
            Some(Target::Input) => Zone::Compose,
            Some(target) if Section::of(target) == Section::Upper => Zone::Studios,
            Some(_) => Zone::Waiting,
            None => match self.hollywood.section {
                Section::Upper => Zone::Studios,
                Section::Lower => Zone::Waiting,
            },
        };
        let next_two = match (current, forward) {
            (Zone::Compose, true) => [Zone::Waiting, Zone::Studios],
            (Zone::Waiting, true) => [Zone::Studios, Zone::Compose],
            (Zone::Studios, true) => [Zone::Compose, Zone::Waiting],
            (Zone::Compose, false) => [Zone::Studios, Zone::Waiting],
            (Zone::Waiting, false) => [Zone::Compose, Zone::Studios],
            (Zone::Studios, false) => [Zone::Waiting, Zone::Compose],
        };
        let band = self.hollywood_band();
        let waits = self.hollywood_waits();
        let frames = self.hollywood_frames();
        let current_studio = self.studio.current.clone().map(Target::Studio);

        for zone in next_two {
            match zone {
                Zone::Compose => {
                    if self.has_hollywood_cfg
                        && let Some(index) = band.iter().position(|t| matches!(t, Target::Input))
                    {
                        self.hollywood_select(Target::Input);
                        self.hollywood.editing = true;
                        self.hollywood_reveal_card(&band, index);
                        return;
                    }
                }
                Zone::Waiting => {
                    let target = waits
                        .iter()
                        .find(|t| band.contains(t))
                        .cloned()
                        .or_else(|| band.iter().rfind(|t| matches!(t, Target::Talk(_))).cloned())
                        .or_else(|| {
                            (!self.has_hollywood_cfg)
                                .then(|| band.last().cloned())
                                .flatten()
                        });
                    if let Some(target) = target {
                        let index = band.iter().position(|r| r == &target).unwrap_or(0);
                        self.hollywood_select(target);
                        self.hollywood_reveal_card(&band, index);
                        return;
                    }
                }
                Zone::Studios => {
                    let target = self
                        .hollywood
                        .last_upper
                        .clone()
                        .filter(|t| frames.contains(t))
                        .or_else(|| {
                            current_studio
                                .as_ref()
                                .filter(|t| frames.contains(t))
                                .cloned()
                        })
                        .or_else(|| frames.first().cloned());
                    if let Some(target) = target {
                        self.hollywood_select(target);
                        self.hollywood_keep_frame_visible();
                        return;
                    }
                }
            }
        }
    }

    /// ← / → (`h` / `l`) in `Upper`: move the frame cursor, the
    /// new-studio frame included, and keep it in the drawn window.
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

    /// ← / → (`h` / `l`) in `Lower`: step across the selected waiting
    /// row's action chips (`allow y` / `deny d` / `peek p` / `open ↵`).
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

    /// ↑ / ↓ (`k` / `j`) within the active section: `Upper` moves a
    /// grid row at a time within `frames` and stays in `Upper`; `Lower`
    /// walks strictly within `band` and stays in `Lower`.
    pub(crate) fn hollywood_row_move(&mut self, delta: isize) {
        match self.hollywood_active_section() {
            Section::Upper => {
                if self.hollywood.selected.as_ref() == Some(&Target::Title) {
                    if delta > 0
                        && let Some(first) = self
                            .hollywood
                            .last_upper
                            .clone()
                            .filter(|t| self.hollywood_frames().contains(t))
                            .or_else(|| self.hollywood_frames().first().cloned())
                    {
                        self.hollywood_select(first);
                        self.hollywood_keep_frame_visible();
                    }
                    return;
                }
                let frames = self.hollywood_frames();
                if frames.is_empty() {
                    if delta < 0 {
                        self.hollywood_select(Target::Title);
                    }
                    return;
                }
                let current = self
                    .hollywood
                    .selected
                    .as_ref()
                    .and_then(|t| frames.iter().position(|f| f == t))
                    .unwrap_or(0);
                let per_row = self.hollywood_frame_window().1.max(1) as isize;
                let next = current as isize + delta.signum() * per_row;
                if next < 0 {
                    self.hollywood_select(Target::Title);
                } else if (next as usize) < frames.len() {
                    self.hollywood_select(frames[next as usize].clone());
                    self.hollywood_keep_frame_visible();
                }
            }
            Section::Lower => {
                let band = self.hollywood_band();
                if band.is_empty() {
                    return;
                }
                let next = match self
                    .hollywood
                    .selected
                    .as_ref()
                    .and_then(|t| band.iter().position(|r| r == t))
                {
                    Some(index) => {
                        (index as isize + delta).clamp(0, band.len() as isize - 1) as usize
                    }
                    None if delta >= 0 => 0,
                    None => band.len() - 1,
                };
                self.hollywood_select(band[next].clone());
                self.hollywood_reveal_card(&band, next);
            }
        }
    }

    /// Scroll the frame window so the selected frame shows, whole rows
    /// at a time so a narrow screen never starts mid-row.
    pub(super) fn hollywood_keep_frame_visible(&mut self) {
        let Some(index) = self
            .hollywood
            .selected
            .as_ref()
            .and_then(|t| self.hollywood_frames().iter().position(|f| f == t))
        else {
            return;
        };
        let (visible, per_row) = self.hollywood_frame_window();
        if visible == 0 {
            return;
        }
        let rows = (visible / per_row).max(1);
        let mut start = self.hollywood.frame_scroll;
        let row = index / per_row;
        let start_row = start / per_row;
        if row < start_row {
            start = row * per_row;
        } else if row >= start_row + rows {
            start = (row + 1 - rows) * per_row;
        }
        self.hollywood.frame_scroll = start;
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

    /// Wheel/drag scroll of the cards: clamp the offset into the drawn
    /// metrics.
    pub(crate) fn hollywood_scroll_to(&mut self, start: usize) {
        let max = self
            .hollywood_hits
            .metrics
            .map(|m| m.max_offset_from_bottom)
            .unwrap_or(0);
        self.hollywood.scroll = start.min(max);
    }

    /// The frames' rect as last drawn — the wheel's sideways target.
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
    }
}
