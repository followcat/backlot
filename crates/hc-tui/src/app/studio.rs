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
        let shell_key = self
            .focused
            .as_deref()
            .and_then(|focused| self.shell_studio_key(focused, &machines));
        self.studio
            .sync_with_shell(&machines, self.focused.as_deref(), shell_key);
        // W1/M4: a new current studio scrolls its machine row into the
        // machines tree — the row never falls out of a short sidebar.
        if self.studio.current != self.last_current_studio {
            self.last_current_studio = self.studio.current.clone();
            self.shell_state.reveal_studio_row = true;
            if !self.panel.focused
                && self
                    .panel
                    .selected
                    .as_ref()
                    .and_then(crate::panel::Target::studio)
                    != self.studio.current.as_ref()
            {
                self.panel.selected = None;
                self.panel.selected_row = None;
            }
            if !self.panel.focused
                && let Some(key) = self.studio.current.clone()
            {
                self.panel_reveal_studio(&key);
            }
        }
        self.hollywood_sync_history_from(&machines);
        self.panel.sync_histories(&machines);
        if let Some(cat) = crate::panel::data::catalog(&self.view, None) {
            self.has_hollywood_cfg = cat.hollywood_cfg.configured;
            self.panel.hollywood_hint = !self.has_hollywood_cfg;
        }
        let panel_w = if self.panel.mode == crate::panel::PanelMode::Expanded {
            44
        } else {
            38
        };
        let sheet_w = self.area.0.saturating_sub(self.sidebar_width + 2).max(62) as usize;
        self.panel.server_stopped_cmd = self.server_stopped_command(Some(panel_w));
        self.hollywood.server_stopped_cmd = self.server_stopped_command(Some(sheet_w));
        self.sync_lot(&machines);
        // W1b: the empty lot's line — no document lists a studio, but a
        // stopped machine's ledger does. Read here so the panel, its
        // cursor and its draw all agree.
        self.panel.stopped = self.stopped_ledger(&machines);
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
        if plain {
            // The plain multiplexer's frame is herdr's exactly —
            // the dock stays closed.
            self.panel_close();
        }
        if !self.sidebar_width_manual {
            self.sidebar_width = self.configured_sidebar_width();
        }
        // U7: `--plain` drops the Hollywood bar; studio mode has it.
        self.sync_hollywood_plain();
    }

    /// Whether the sidebar draws the machine-first view: always in
    /// studio mode — a lone `Local` machine still leads the tree —
    /// and in `--plain` only with several endpoints, herdr's own gate.
    pub(super) fn federated_sidebar(&self) -> bool {
        self.studio.active || self.projection.endpoints.len() > 1
    }

    /// The focused terminal's studio when it is a studio shell —
    /// its `backlot.studio`/`backlot.shell` labels name it — and that
    /// studio is still in the aggregate.
    pub(in crate::app) fn shell_studio_key(
        &self,
        terminal: &str,
        machines: &[crate::panel::data::MachineSummary],
    ) -> Option<StudioKey> {
        let t = self.view.inventory.get(terminal)?;
        t.labels.get(crate::shells::SHELL_LABEL)?;
        let key = StudioKey {
            machine: terminal
                .split_once('/')
                .map(|(machine, _)| machine.to_string()),
            id: t.labels.get(crate::shells::STUDIO_LABEL)?.clone(),
        };
        crate::panel::data::studios(machines)
            .iter()
            .any(|s| s.key() == key)
            .then_some(key)
    }

    pub(crate) fn studio_for_workspace(
        &self,
        endpoint_id: &crate::chrome::snapshot::ClientEndpointId,
        workspace_id: &str,
    ) -> Option<StudioKey> {
        let endpoint = self
            .projection
            .endpoints
            .iter()
            .find(|e| &e.endpoint_id == endpoint_id)?;
        let snapshot = endpoint.snapshot.as_deref()?;
        let ws = snapshot
            .workspaces
            .iter()
            .find(|w| w.workspace_id == workspace_id)?;
        let machine = match endpoint_id {
            crate::chrome::snapshot::ClientEndpointId::Local => None,
            crate::chrome::snapshot::ClientEndpointId::Remote(m) => Some(m.as_str()),
        };
        let machines = self.panel_summaries();
        crate::panel::data::studios(&machines)
            .into_iter()
            .find(|s| s.machine == machine && s.studio.id == ws.label)
            .map(|s| s.key())
    }

    /// A switch hands the keys to the pane it lands on: a focused or
    /// full-screen dock, the Call Sheet or a CardView that stayed up
    /// over the switch would keep them instead.
    pub(crate) fn nav_leave_overlays(&mut self) {
        self.panel_leave();
        if self.hollywood.expanded() || self.hollywood.view.is_some() || self.hollywood.editing {
            self.hollywood_leave();
        }
    }

    /// A direct studio key: `alt+pageup/pagedown` move between studios in
    /// the aggregate's order — this core's first, then machine-list
    /// order — `alt+up/down` between the current studio's live
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
                // Entering from outside a studio lands on the current
                // studio without stepping past it; from inside a studio or
                // studio row, steps in aggregate order.
                let anchor = match &self.studio.focus {
                    Some(studio::StudioFocus::Studio(key))
                    | Some(studio::StudioFocus::Role(key, _)) => studios
                        .iter()
                        .position(|s| &s.key() == key)
                        .map(|i| (i as isize + dir).rem_euclid(len) as usize)
                        .unwrap_or_else(|| self.studio.current_index(&machines).unwrap_or(0)),
                    None => self.studio.current_index(&machines).unwrap_or(0),
                };
                let next = studios[anchor].key();
                // The member the person was on when they step away — a
                // role id or a shell's address — so stepping back lands
                // there even if the last sync never saw it.
                if let Some(left) = match &self.studio.focus {
                    Some(studio::StudioFocus::Studio(key))
                    | Some(studio::StudioFocus::Role(key, _)) => Some(key.clone()),
                    None => self.studio.current.clone(),
                } {
                    self.remember_member(&left, &machines);
                }
                // The dock and the machines tree keep the keyboard: a
                // step swaps the main pane but leaves that owner in
                // place, its cursor on the studio it landed on. A
                // full-screen dock has no visible pane beside it and a
                // composing `›` line is mid-sentence, so both step back
                // as before.
                let tree_owns = self.tree_focused();
                let dock_owns = !tree_owns
                    && self.panel.focused
                    && !self.panel_fullscreen()
                    && !self.panel_editing()
                    && self.panel.finish_confirm.is_none();
                if !dock_owns && !tree_owns {
                    self.nav_leave_overlays();
                }
                self.studio.set_studio_focus(next.clone());
                self.last_current_studio = self.studio.current.clone();
                if !dock_owns
                    && self
                        .panel
                        .selected
                        .as_ref()
                        .and_then(crate::panel::Target::studio)
                        != Some(&next)
                {
                    self.panel.selected = None;
                    self.panel.selected_row = None;
                }
                self.panel_reveal_studio(&next);
                self.sync_lot(&machines);
                // Land the stepped-to studio's member in the main pane.
                // Quiet when it has none: a studio still seating queues
                // its landing for the next publish, a settled one leaves
                // the pane as it is — no notice on a step. Either way a
                // queue left by an earlier step is dropped, so a studio
                // left behind never takes the keys later.
                self.ns_pending_focus = None;
                self.land_studio_member(&next);
                if dock_owns {
                    self.panel_select_studio(&next);
                }
                if tree_owns {
                    self.tree_focus_studio_row(&next);
                }
            }
            StudioNav::PreviousRole | StudioNav::NextRole => {
                self.nav_leave_overlays();
                let dir: isize = if matches!(nav, StudioNav::NextRole) {
                    1
                } else {
                    -1
                };
                let target_key = match &self.studio.focus {
                    Some(studio::StudioFocus::Studio(key))
                    | Some(studio::StudioFocus::Role(key, _)) => key.clone(),
                    None => match self.studio.current_index(&machines) {
                        Some(idx) => studios[idx].key(),
                        None => return,
                    },
                };
                let Some(studio) = crate::panel::data::studio(&machines, &target_key) else {
                    return;
                };
                let cycle: Vec<String> = studio::cast_order(studio.studio)
                    .into_iter()
                    .filter_map(|role| studio::live_terminal(role).map(str::to_owned))
                    .chain(self.studio_shells_live(&target_key))
                    .collect();
                let current_role_term = match &self.studio.focus {
                    Some(studio::StudioFocus::Role(key, term)) if key == &target_key => {
                        cycle.iter().find(|t| *t == term).cloned()
                    }
                    _ => None,
                };
                let Some(current_term) = current_role_term else {
                    self.focus_studio(&target_key);
                    return;
                };
                if cycle.is_empty() {
                    return;
                }
                let position = cycle.iter().position(|t| t == &current_term).unwrap_or(0) as isize;
                let next = &cycle[(position + dir).rem_euclid(cycle.len() as isize) as usize];
                self.focus_pane(next.clone());
            }
        }
        // M4: the machines tree follows the selection, never dropping
        // the studio's machine row out of a short sidebar.
        self.shell_state.reveal_studio_row = true;
    }

    /// Make `key` the current studio and focus its remembered member
    /// — a role or a shell — else its producer, else its first live
    /// person in cast order, else its first live shell. A studio
    /// with no live member still becomes current (cast and frame
    /// follow): one whose stage can still seat a member queues the
    /// landing (`ns_pending_focus`), a settled one gets the notice.
    pub(crate) fn focus_studio(&mut self, key: &StudioKey) {
        let machines = self.panel_summaries();
        let Some(studio) = crate::panel::data::studio(&machines, key) else {
            return;
        };
        self.studio.current = Some(key.clone());
        self.nav_leave_overlays();
        match self.studio_landing(key, &studio) {
            // The member's terminal is machine-qualified on an added
            // machine — the same address shape a sidebar focus takes.
            Some(terminal) => self.focus_pane(terminal),
            None if self.studio_still_seating(key, studio.studio) => {
                self.ns_pending_focus = Some(key.clone());
            }
            None => self.show_notice("Studio", &format!("{} has no live terminal", key.id)),
        }
    }

    /// The terminal a landing on `studio` focuses — its remembered
    /// member (a role id or a shell's address) while live, else its
    /// producer, else its first live person in cast order, else its
    /// first live shell. `None` when the studio has no live member.
    /// Shared by `focus_studio` and the `Alt+arrow` step.
    pub(crate) fn studio_landing(
        &self,
        key: &StudioKey,
        studio: &crate::panel::data::StudioRef<'_>,
    ) -> Option<String> {
        let live = |role: &RoleSummary| studio::live_terminal(role).is_some();
        let shells = self.studio_shells_live(key);
        let remembered = self.studio.remembered.get(key).and_then(|member| {
            studio
                .studio
                .roles
                .iter()
                .find(|role| &role.role == member && live(role))
                .and_then(|role| role.terminal.clone())
                .or_else(|| shells.iter().find(|a| *a == member).cloned())
        });
        remembered
            .or_else(|| {
                studio
                    .studio
                    .roles
                    .iter()
                    .find(|role| role.role == "producer" && live(role))
                    .and_then(|role| role.terminal.clone())
            })
            .or_else(|| {
                studio::cast_order(studio.studio)
                    .into_iter()
                    .find(|role| live(role))
                    .and_then(|role| role.terminal.clone())
            })
            .or_else(|| shells.first().cloned())
    }

    /// Land `key`'s member in the main pane quietly — a studio switch
    /// the person made that must not announce itself. The landing is
    /// `studio_landing`; a studio still seating queues the landing for
    /// the next publish, a settled one leaves the pane as it is.
    pub(crate) fn land_studio_member(&mut self, key: &StudioKey) {
        let machines = self.panel_summaries();
        let Some(studio) = crate::panel::data::studio(&machines, key) else {
            return;
        };
        match self.studio_landing(key, &studio) {
            Some(terminal) => self.focus_pane(terminal),
            None if self.studio_still_seating(key, studio.studio) => {
                self.ns_pending_focus = Some(key.clone());
            }
            None => {}
        }
    }

    /// Record the member the person is on in `key` — the role id or the
    /// shell's address of the focused terminal, when it belongs there.
    /// `sync_studio` already latches this on a focus move; stepping away
    /// calls it so a member chosen since the last sync (a cast or lot
    /// click) is remembered too.
    fn remember_member(
        &mut self,
        key: &StudioKey,
        machines: &[crate::panel::data::MachineSummary],
    ) {
        let Some(focused) = self.focused.clone() else {
            return;
        };
        let Some(studio) = crate::panel::data::studio(machines, key) else {
            return;
        };
        let member = studio
            .studio
            .roles
            .iter()
            .find(|role| role.terminal.as_deref() == Some(focused.as_str()))
            .map(|role| role.role.clone())
            .or_else(|| {
                self.studio_shells_live(key)
                    .into_iter()
                    .find(|shell| shell == &focused)
            });
        if let Some(member) = member {
            self.studio.remembered.insert(key.clone(), member);
        }
    }

    /// R3-M4: `key` can still seat a member — unfinished, at least
    /// one open role in its cast, its machine reachable and its
    /// `backlot-stage` live — so a landing on it queues instead of
    /// failing with `has no live terminal`.
    fn studio_still_seating(
        &self,
        key: &StudioKey,
        studio: &backlot_model::summary::StudioSummary,
    ) -> bool {
        studio.state != "finished"
            && studio
                .roles
                .iter()
                .any(|role| !matches!(role.state.as_str(), "closed" | "standby"))
            && self.machine_reachable(key.machine.as_deref())
            && self.machine_stage_running(key.machine.as_deref())
    }
}
