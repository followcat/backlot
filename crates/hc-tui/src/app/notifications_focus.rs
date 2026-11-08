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
    /// focus the toast's pane. A non-local endpoint reports unavailable
    /// and keeps the toast, like the reference's offline path.
    pub(super) fn focus_visible_notification(&mut self) {
        let Some(notification) = self.notifications.visible.as_ref() else {
            return;
        };
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
            self.refresh_projection();
            self.persist_focus();
            self.sync_target();
        }
    }

    fn endpoint_is_online(&self, endpoint_id: &ClientEndpointId) -> bool {
        self.projection
            .endpoints
            .iter()
            .find(|endpoint| &endpoint.endpoint_id == endpoint_id)
            .is_some_and(|endpoint| {
                endpoint.status == crate::chrome::snapshot::ClientEndpointStatus::Online
            })
    }

    fn endpoint_label(&self, endpoint_id: &ClientEndpointId) -> String {
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
        let count = self.pane.take_bells().saturating_add(
            self.observers
                .values_mut()
                .map(pane::Pane::take_bells)
                .sum(),
        );
        if count > 0 {
            let _ = (self.host_control)(&crate::terminal_notify::terminal_bells(count));
        }
    }
}
