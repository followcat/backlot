// Derived from herdr src/client/shell/notifications.rs and
// src/app/actions.rs, plus the herdr-app src/model.rs `alert` rule
// (Apache-2.0).
//! Alert synthesis: hc's core emits no notification events, so the
//! client diffs every pane's effective status word per reconcile —
//! blocked/error entries and busy-to-idle exits, panes only, at most
//! once per turn.

use super::*;
use crate::app::notifications::{
    NotificationEvent, NotificationKind, NotificationSound, PaneAlert, is_blocked, is_busy,
};
use crate::chrome::snapshot::ClientEndpointId;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

impl App {
    /// The synthesized-alert rule — herdr-app's `alert` gated by the
    /// reference's active-tab suppression, on raw 7501 words:
    /// `Some` needs a previous baseline (mid-session panes alert from
    /// nothing; app-start baselines are seeded silently below).
    fn alert_for(prev: Option<&str>, new: Option<&str>) -> Option<NotificationKind> {
        if is_blocked(new) && !is_blocked(prev) {
            Some(NotificationKind::NeedsAttention)
        } else if is_busy(prev) && new.is_some() && !is_busy(new) {
            Some(NotificationKind::Finished)
        } else {
            None
        }
    }

    /// `notification_sound_for_state_change`: Blocked always requests
    /// attention; a completion sounds only when the tab wasn't watched.
    fn sound_for(
        suppress: bool,
        prev: Option<&str>,
        new: Option<&str>,
    ) -> Option<NotificationSound> {
        if is_blocked(new) && !is_blocked(prev) {
            Some(NotificationSound::Request)
        } else if !suppress && is_busy(prev) && new.is_some() && !is_busy(new) {
            Some(NotificationSound::Done)
        } else {
            None
        }
    }

    /// Diff every pane's effective status word against the last reconcile;
    /// panes only (never sub-agents), at most one alert per turn per pane.
    /// Ported from `apply_pane_state_change` +
    /// `record_or_deliver_agent_notification`.
    pub(super) fn synthesize_alerts(&mut self, now: Instant) {
        let visible: HashSet<String> = if self.outer_focused == Some(false) {
            HashSet::new()
        } else {
            self.visible_panes().into_iter().collect()
        };
        let mut panes: HashMap<String, (ClientEndpointId, String, String)> = HashMap::new();
        let mut endpoints = HashSet::new();
        for endpoint in &self.projection.endpoints {
            endpoints.insert(endpoint.endpoint_id.clone());
            let Some(snapshot) = endpoint.snapshot.as_deref() else {
                continue;
            };
            for pane in &snapshot.panes {
                panes.insert(
                    pane.pane_id.clone(),
                    (
                        endpoint.endpoint_id.clone(),
                        pane.workspace_id.clone(),
                        pane.tab_id.clone(),
                    ),
                );
            }
        }
        self.notifications
            .retire(&endpoints, &panes.keys().cloned().collect::<HashSet<_>>());
        self.notifications
            .panes
            .retain(|id, _| self.view.inventory.get(id).is_some() && panes.contains_key(id));
        // The first pass *with panes* seeds the baseline silently — the
        // herdr-app `listed_once` rule: state that predates the client is
        // not news. An empty listing must not latch it, or pre-existing
        // blocked panes would alert on their first appearance.
        if !panes.is_empty()
            && self.notifications.panes.is_empty()
            && self.notifications.pending.is_empty()
            && self.notifications.queued.is_empty()
            && self.notifications.visible.is_none()
            && !self.notifications_seeded
        {
            for id in panes.keys() {
                let Some(term) = self.view.inventory.get(id) else {
                    continue;
                };
                self.notifications.panes.insert(
                    id.clone(),
                    PaneAlert {
                        created_ms: term.created_ms,
                        word: crate::project::effective_status(term)
                            .map(|status| status.state.clone()),
                        alerted_turn: None,
                    },
                );
            }
            self.notifications_seeded = true;
        }
        let mut events = Vec::new();
        for (id, (endpoint_id, workspace_id, tab_id)) in &panes {
            let Some(term) = self.view.inventory.get(id) else {
                continue;
            };
            if term.parent.as_ref().is_some_and(|p| !p.is_empty()) {
                continue; // sub-agents report to their parent, not the person
            }
            let word = crate::project::effective_status(term).map(|s| s.state.clone());
            let state = self
                .notifications
                .panes
                .entry(id.clone())
                .or_insert_with(|| PaneAlert {
                    created_ms: term.created_ms,
                    word: None,
                    alerted_turn: None,
                });
            if state.created_ms != term.created_ms {
                state.created_ms = term.created_ms;
                state.word = None;
                state.alerted_turn = None;
            }
            let prev = state.word.take();
            state.word = word.clone();
            if prev == word {
                continue;
            }
            let Some(kind) = Self::alert_for(prev.as_deref(), word.as_deref()) else {
                continue;
            };
            if state.alerted_turn == Some(term.turn) {
                continue; // once per turn
            }
            state.alerted_turn = Some(term.turn);
            let looking = visible.contains(id);
            let sound = Self::sound_for(looking, prev.as_deref(), word.as_deref());
            events.push(self.notification_event(
                term,
                kind,
                sound,
                endpoint_id.clone(),
                workspace_id.clone(),
                tab_id.clone(),
            ));
        }
        for event in events {
            self.receive_notification(event, now);
        }
    }

    /// `agent_notification_delivery`'s title/context build. The agent label
    /// matches the sidebar row's resolution order.
    fn notification_event(
        &self,
        term: &hc_proto::terminal::Terminal,
        kind: NotificationKind,
        sound: Option<NotificationSound>,
        endpoint_id: ClientEndpointId,
        workspace_id: String,
        tab_id: String,
    ) -> NotificationEvent {
        let snapshot = self
            .projection
            .endpoints
            .iter()
            .find(|endpoint| endpoint.endpoint_id == endpoint_id)
            .and_then(|endpoint| endpoint.snapshot.as_deref());
        let agent = snapshot.and_then(|snapshot| {
            snapshot
                .agents
                .iter()
                .find(|agent| agent.pane_id == term.address)
        });
        let agent_label = agent
            .and_then(|agent| {
                agent
                    .display_agent
                    .as_deref()
                    .or(agent.name.as_deref())
                    .or(agent.agent.as_deref())
                    .or(agent.title.as_deref())
            })
            .or(term.name.as_deref())
            .unwrap_or(term.address.as_str())
            .to_owned();
        let known_agent = agent
            .and_then(|agent| agent.agent.as_deref())
            .and_then(crate::config::agent::parse_agent_label);
        let event_text = match kind {
            NotificationKind::NeedsAttention => "needs attention",
            NotificationKind::Finished => "finished",
        };
        // `notification_context`: "workspace · n[ · tab]" — the tab label
        // only joins when the workspace has more than one tab.
        let context = snapshot
            .and_then(|snapshot| {
                let ws_index = snapshot
                    .workspaces
                    .iter()
                    .position(|ws| ws.workspace_id == workspace_id)?;
                let workspace = &snapshot.workspaces[ws_index];
                let mut context = format!("{} · {}", workspace.label, ws_index + 1);
                if snapshot
                    .tabs
                    .iter()
                    .filter(|tab| tab.workspace_id == workspace_id)
                    .count()
                    > 1
                    && let Some(tab) = snapshot.tabs.iter().find(|tab| tab.tab_id == tab_id)
                {
                    context.push_str(&format!(" · {}", tab.label));
                }
                Some(context)
            })
            .unwrap_or_else(|| workspace_id.clone());
        NotificationEvent {
            kind,
            title: format!("{agent_label} {event_text}"),
            body: Some(context),
            sound,
            agent: known_agent,
            endpoint_id,
            workspace_id: Some(workspace_id),
            tab_id: Some(tab_id),
            pane_id: Some(term.address.clone()),
        }
    }
}
