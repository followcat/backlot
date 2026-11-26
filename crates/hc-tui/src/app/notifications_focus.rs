// Derived from herdr src/client/shell/notifications.rs and
// src/client/shell.rs (`focus_notification_target`, the terminal-bell
// forward) (Apache-2.0).
//! `prefix+o` target focus and the pane-BEL forward — the imperative
//! halves of the notification surface that act on `App` state directly.

use super::*;
use crate::chrome::snapshot::ClientEndpointId;
use std::time::Instant;

impl App {
    /// `focus_visible_notification` (`prefix+o`): dismiss, promote, then
    /// focus the toast's pane. A studio event has no pane — its target is
    /// the needs-you row, opened the same way Enter on that row would. A
    /// non-local endpoint reports unavailable and keeps the toast, like
    /// the reference's offline path.
    pub(super) fn focus_visible_notification(&mut self) {
        let Some(notification) = self.notifications.visible.as_ref() else {
            return;
        };
        if notification.event.studio.is_some() {
            let notification = self
                .notifications
                .visible
                .take()
                .expect("checked visible notification");
            self.notifications.promote_queued(Instant::now());
            if let Some(target) = notification.event.studio.clone() {
                self.studio_notify_open(&target);
            }
            return;
        }
        if notification.event.pane_id.is_some()
            && !self.endpoint_is_online(&notification.event.endpoint_id)
        {
            let label = self.endpoint_label(&notification.event.endpoint_id);
            self.show_notice("Machine unavailable", &format!("{label} is unavailable"));
            return;
        }
        let notification = self
            .notifications
            .visible
            .take()
            .expect("checked visible notification");
        self.notifications.promote_queued(Instant::now());
        let Some(pane_id) = notification.event.pane_id else {
            return;
        };
        // Local endpoints focus directly — the same path as a pane-row
        // click; remote endpoints have no direct focus RPC.
        if notification.event.endpoint_id == ClientEndpointId::Local
            && self.view.inventory.get(&pane_id).is_some()
        {
            self.focused = Some(pane_id);
            self.refresh_projection();
            self.mark_active_seen();
            self.persist_focus();
            self.sync_target();
        }
    }

    /// A studio event's jump: open the row's own target. The panel and
    /// Call Sheet jumps select the row without running its key — the
    /// person presses `s`/`a` there themselves.
    pub(super) fn studio_notify_open(&mut self, target: &crate::hollywood::Target) {
        use crate::hollywood::Target;
        match target.clone() {
            Target::Prompt(studio, role) | Target::Role(studio, role) => {
                match self.studio_role_terminal(&studio, &role) {
                    Some(terminal) => {
                        self.hollywood_leave_if_open();
                        self.focus_pane(terminal);
                    }
                    None => self.show_notice("Studio", "the role has no terminal to focus"),
                }
            }
            Target::Hollywood | Target::HollywoodLive => match self.hollywood_live_terminal() {
                Some(terminal) => {
                    self.hollywood_leave_if_open();
                    self.focus_pane(terminal);
                }
                None => self.show_notice(
                    "Hollywood",
                    "bl-hollywood is not running — the stage starts it",
                ),
            },
            Target::Card(id) => {
                // Expand the Call Sheet on that card in Resting.
                self.hollywood.mode = crate::hollywood::LayerMode::Expanded;
                self.hollywood_select(Target::Card(id));
                self.hollywood.scroll = 0;
                self.studio.lot.counts.unread = 0;
            }
            Target::OutwardCard(key, id) => {
                self.hollywood.mode = crate::hollywood::LayerMode::Expanded;
                self.hollywood_select(Target::OutwardCard(key, id));
                self.hollywood.scroll = 0;
                self.studio.lot.counts.unread = 0;
            }
            Target::Ready(key) | Target::Tell(key) | Target::Upgrade(key) => {
                self.focus_studio(&key)
            }
            Target::Stage(machine, offline) => {
                self.studio_notify_stage(&machine, offline);
            }
            Target::Talk(seq) => {
                let card = self.hollywood_exchange_is_card(seq);
                self.run_action(crate::studio_cmds::hollywood_exchange(seq, card));
            }
            Target::Headline => self.hollywood_headline_open(),
            // The remaining targets are not studio events today: a
            // studio frame, the new-studio entry, the title, the calm
            // row, the input line and the setup hint stay inert here.
            Target::NewStudio
            | Target::Studio(_)
            | Target::Title
            | Target::Calm
            | Target::Input
            | Target::Setup => {}
        }
    }

    /// The role's live terminal from the aggregate — already
    /// machine-qualified for a remote studio.
    fn studio_role_terminal(
        &self,
        studio: &crate::studio::StudioKey,
        role: &str,
    ) -> Option<String> {
        let machines = self.panel_summaries();
        crate::panel::data::studio(&machines, studio).and_then(|found| {
            found
                .studio
                .roles
                .iter()
                .find(|r| r.role == role)
                .and_then(|r| r.terminal.clone())
        })
    }

    /// This core's active live Hollywood terminal address.
    pub(super) fn hollywood_live_terminal(&self) -> Option<String> {
        self.hollywood_terminal()
    }

    /// Collapse the expanded Call Sheet before a jump focuses a pane —
    /// the same leave the sheet's own Enter takes.
    pub(super) fn hollywood_leave_if_open(&mut self) {
        if self.hollywood.expanded() {
            self.hollywood.mode = crate::hollywood::LayerMode::Collapsed;
            self.hollywood.editing = false;
            self.hollywood.view = None;
        }
    }

    /// A stopped stage's jump: land the dock on its `stage stopped` row,
    /// focused and revealed — never start it. `s` on that row is the
    /// person's press.
    fn studio_notify_stage(&mut self, machine: &Option<String>, offline: bool) {
        if offline {
            // A lost machine has no stage row to select; say where it is.
            self.show_notice(
                "Machine",
                &match machine {
                    Some(name) => format!("{name}: unreachable"),
                    None => "unreachable".to_string(),
                },
            );
            return;
        }
        self.panel.mode = crate::panel::PanelMode::Expanded;
        self.panel.focused = true;
        self.panel.editing = false;
        let target = crate::panel::Target::Stage(machine.clone());
        self.panel.selected = Some(target.clone());
        self.panel.selected_row = self.panel_targets().iter().position(|t| *t == target);
        // Reveal the row in the scrolled body, the same arithmetic
        // `panel_move` runs.
        if let Some(index) = self.panel.selected_row {
            let body_height = self.panel_hits.body.height.max(1);
            let machines = self.panel_summaries();
            let rows = crate::panel::rows(
                &machines,
                |m| crate::panel::data::stage_running(&self.view, m),
                |m| self.machine_reachable(m),
                &self.panel,
                self.panel_mode_at(self.area.0),
                self.studio.current.as_ref(),
                self.panel_hits.body.height,
            );
            let heights: Vec<u16> = rows.iter().skip(1).map(|r| r.height()).collect();
            self.panel.scroll = crate::chrome::scroll::list_scroll_start_to_reveal(
                &heights,
                &[],
                body_height,
                self.panel.scroll,
                index.saturating_sub(1),
            );
        }
    }

    pub(super) fn endpoint_is_online(&self, endpoint_id: &ClientEndpointId) -> bool {
        self.projection
            .endpoints
            .iter()
            .find(|endpoint| &endpoint.endpoint_id == endpoint_id)
            .is_some_and(|endpoint| {
                endpoint.status == crate::chrome::snapshot::ClientEndpointStatus::Online
            })
    }

    pub(super) fn endpoint_label(&self, endpoint_id: &ClientEndpointId) -> String {
        self.projection
            .endpoints
            .iter()
            .find(|endpoint| &endpoint.endpoint_id == endpoint_id)
            .map(|endpoint| endpoint.label.clone())
            .unwrap_or_else(|| match endpoint_id {
                ClientEndpointId::Local => "Local".to_owned(),
                ClientEndpointId::Remote(name) => name.clone(),
            })
    }

    /// `ServerMessage::TerminalBell`: pane programs' BELs reach the outer
    /// terminal. hc's core has no bell message, so hc-vt's bell hook
    /// counts them and the client writes them here — a documented
    /// core/data difference (docs/parity.md).
    pub(super) fn drain_bells(&mut self) {
        // A bell both wires saw rings once: while the focused pane is
        // painted, its retained duplicate observer's bells are drained
        // but never added — the live wire already reported them.
        let painted = self
            .pane
            .is_painted()
            .then(|| self.pane.target().map(str::to_owned))
            .flatten();
        let count = self.pane.take_bells().saturating_add(
            self.observers
                .iter_mut()
                .map(|(id, pane)| {
                    let bells = pane.take_bells();
                    if painted.as_deref() == Some(id.as_str()) {
                        0
                    } else {
                        bells
                    }
                })
                .sum::<u64>(),
        );
        if count > 0 {
            let _ = (self.host_control)(&crate::terminal_notify::terminal_bells(count));
        }
    }
}
