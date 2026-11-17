//! Studio mode's App wiring (U1): the mode latch synced from the last
//! good `backlot` document, the four direct-key navigation actions
//! and the studio-mode sidebar width. The cast's own renderer lives
//! in `crate::cast`; the row model of the panel in `crate::panel`.

use backlot_model::summary::RoleSummary;

use super::App;
use crate::panel::StudioNav;
use crate::studio::{self, StudioKey};

impl App {
    /// Sync the studio latch against the last good summaries and the
    /// focus — called from `refresh_projection`, so publishes and
    /// focus moves both land here. A mode flip re-derives the sidebar
    /// width unless the user dragged it this session.
    pub(super) fn sync_studio(&mut self) {
        // U2: unadorned `backlot` is always in studio mode — the cast
        // (with its `others` row) replaces the agents list even while
        // no studio exists and the opening page covers the panes.
        // `--plain` is Herdr's exactly: no cast, no studio keys —
        // the parity harness measures that client.
        let was = self.studio.active;
        self.studio.active = !self.plain;
        let machines = self.panel_summaries();
        self.studio.sync(&machines, self.focused.as_deref());
        if self.studio.active != was && !self.sidebar_width_manual {
            self.sidebar_width = self.configured_sidebar_width();
        }
    }

    /// The sidebar's live width per mode: backlot's `[sidebar] width`
    /// in studio mode, herdr's `ui.sidebar_width` outside it.
    pub(crate) fn configured_sidebar_width(&self) -> u16 {
        if self.studio.active {
            self.panel_config.sidebar_width
        } else {
            self.shell.sidebar_width
        }
    }

    /// `--plain` (U2) — install the flag before the run loop so the
    /// first `sync_studio` already refuses to latch studio mode.
    pub(crate) fn set_plain(&mut self, plain: bool) {
        self.plain = plain;
        self.studio.active = !plain;
        if !self.sidebar_width_manual {
            self.sidebar_width = self.configured_sidebar_width();
        }
        // U7: `--plain` drops the Hollywood bar; studio mode has it.
        self.sync_hollywood_plain();
    }

    /// A direct studio key: `alt+up/down` move between studios in
    /// the aggregate's order — this core's first, then machine-list
    /// order — `alt+left/right` between the current studio's live
    /// people in cast order — all wrapping.
    pub(crate) fn studio_nav(&mut self, nav: StudioNav) {
        let machines = self.panel_summaries();
        let studios = crate::panel::data::studios(&machines);
        if studios.is_empty() {
            return;
        }
        match nav {
            StudioNav::PreviousStudio | StudioNav::NextStudio => {
                let dir: isize = if matches!(nav, StudioNav::NextStudio) {
                    1
                } else {
                    -1
                };
                let len = studios.len() as isize;
                let current = self.studio.current_index(&machines).unwrap_or(0) as isize;
                let next = studios[(current + dir).rem_euclid(len) as usize].key();
                self.focus_studio(&next);
            }
            StudioNav::PreviousRole | StudioNav::NextRole => {
                let dir: isize = if matches!(nav, StudioNav::NextRole) {
                    1
                } else {
                    -1
                };
                let Some(index) = self.studio.current_index(&machines) else {
                    return;
                };
                let studio = studios[index];
                // The cycle: the live people in cast order, then the
                // studio's shells in shell-number order — a remote
                // studio's terminals arrive machine-qualified, so the
                // focus addresses match the inventory's.
                let cycle: Vec<String> = studio::cast_order(studio.studio)
                    .into_iter()
                    .filter_map(|role| studio::live_terminal(role).map(str::to_owned))
                    .chain(self.focused_studio_shells())
                    .collect();
                // A first press while the focus is outside the current
                // studio lands on its remembered person.
                let inside = self
                    .focused
                    .as_deref()
                    .is_some_and(|terminal| cycle.iter().any(|t| t == terminal));
                if !inside {
                    let key = studio.key();
                    self.focus_studio(&key);
                    return;
                }
                if cycle.is_empty() {
                    return;
                }
                let position = cycle
                    .iter()
                    .position(|t| Some(t.as_str()) == self.focused.as_deref())
                    .unwrap_or(0) as isize;
                let next = &cycle[(position + dir).rem_euclid(cycle.len() as isize) as usize];
                self.focus_pane(next.clone());
            }
        }
    }

    /// Make `key` the current studio and focus its remembered person,
    /// else its producer, else its first person with a live terminal
    /// — in cast order. A studio with no live terminal still becomes
    /// current (cast and frame follow) and a notice says so.
    pub(crate) fn focus_studio(&mut self, key: &StudioKey) {
        let machines = self.panel_summaries();
        let Some(studio) = crate::panel::data::studio(&machines, key) else {
            return;
        };
        self.studio.current = Some(key.clone());
        let live = |role: &RoleSummary| studio::live_terminal(role).is_some();
        let remembered = self
            .studio
            .remembered
            .get(key)
            .and_then(|role| studio.studio.roles.iter().find(|r| &r.role == role))
            .filter(|role| live(role));
        let target = remembered
            .or_else(|| {
                studio
                    .studio
                    .roles
                    .iter()
                    .find(|role| role.role == "producer" && live(role))
            })
            .or_else(|| {
                studio::cast_order(studio.studio)
                    .into_iter()
                    .find(|role| live(role))
            })
            .and_then(|role| role.terminal.clone());
        match target {
            // The role's terminal is machine-qualified on an added
            // machine — the same address shape a sidebar focus takes.
            Some(terminal) => self.focus_pane(terminal),
            None => self.show_notice("Studio", &format!("{} has no live terminal", key.id)),
        }
    }
}
