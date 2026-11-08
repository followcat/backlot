// Derived from herdr src/app/actions.rs (`record_or_deliver_agent_notification`,
// `notification_toast_for_state_change`, `notification_sound_for_state_change`,
// `active_tab_suppresses_notifications`) and the herdr-app src/model.rs `alert`
// rule (Apache-2.0).
//! Alert synthesis: hc's core emits no notification events, so the client
//! diffs every agent pane's effective status word per reconcile — blocked/error
//! entries and busy-to-idle exits, panes only, one alert per transition. The
//! core's `turn` counter (bumps on every working exit) catches completions
//! that happened inside a reconnect gap the word diff cannot see.

use super::*;
use crate::app::notifications::{
    NotificationEvent, NotificationKind, NotificationSound, PaneAlert, is_blocked, is_busy,
};
use crate::chrome::snapshot::ClientEndpointId;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

impl App {
    /// `notification_toast_for_state_change`: blocked entries always
    /// announce; busy exits announce a completion. `error` counts as
    /// blocked — an hc extension the reference's AgentState lacks.
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
    /// attention — even on the active tab; a completion sounds only when
    /// the tab wasn't watched.
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

    /// Diff every agent pane's effective status word against the last
    /// reconcile. Sub-agent terminals report to their parent and never
    /// alert. The word diff is the dedup — exactly one alert per change —
    /// and a `turn` bump with a settled word means a completion was
    /// compressed inside a reconnect gap (the core still counted it).
    /// Ported from `apply_pane_state_change` + `record_or_deliver_agent_notification`.
    pub(super) fn synthesize_alerts(&mut self, now: Instant) {
        // A fresh inventory replaces the whole pane set — the subscription
        // epoch stands in for the reference's fresh attach, so retire
        // everything tied to the previous inventory and re-seed.
        if self.notifications.generation != Some(self.view.generation) {
            self.notifications = crate::app::notifications::Notifications::default();
            self.notifications.generation = Some(self.view.generation);
            self.notifications_seeded = false;
        }
        let mut panes: HashMap<String, (ClientEndpointId, String, String, bool)> = HashMap::new();
        let mut endpoints = HashSet::new();
        for endpoint in &self.projection.endpoints {
            endpoints.insert(endpoint.endpoint_id.clone());
            let Some(snapshot) = endpoint.snapshot.as_deref() else {
                continue;
            };
            for agent in &snapshot.agents {
                panes.insert(
                    agent.pane_id.clone(),
                    (
                        endpoint.endpoint_id.clone(),
                        agent.workspace_id.clone(),
                        agent.tab_id.clone(),
                        snapshot
                            .focused_tab_id
                            .as_deref()
                            .is_some_and(|tab| tab == agent.tab_id),
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
                        turn: term.turn,
                    },
                );
            }
            self.notifications_seeded = true;
        }
        let mut events = Vec::new();
        for (id, (endpoint_id, workspace_id, tab_id, is_active_tab)) in &panes {
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
                    turn: term.turn,
                });
            if state.created_ms != term.created_ms {
                state.created_ms = term.created_ms;
                state.word = None;
                state.turn = term.turn;
            }
            let prev = state.word.take();
            let prev_turn = state.turn;
            state.word = word.clone();
            state.turn = term.turn;
            // `turn` only climbs on working exits; a compressed gap bumps it
            // while the word sits still. A shrink means a fresh core — reseed.
            let turn_bump = term.turn > prev_turn;
            if prev == word && !turn_bump {
                continue;
            }
            // `active_tab_suppresses_notifications` at event creation —
            // toasts on the watched tab are never built, while the blocked
            // request sound still rings.
            let suppress = *is_active_tab && self.outer_focused != Some(false);
            let completion = !is_busy(word.as_deref()) && (is_busy(prev.as_deref()) || turn_bump);
            let kind = (!suppress)
                .then(|| {
                    Self::alert_for(prev.as_deref(), word.as_deref())
                        .or(completion.then_some(NotificationKind::Finished))
                })
                .flatten();
            let sound = Self::sound_for(suppress, prev.as_deref(), word.as_deref())
                .or((completion && !suppress).then_some(NotificationSound::Done));
            // The reference client validates `Finished` against a snapshot
            // trailing the event by one update; the pre-transition word
            // stands in for it. A compressed completion's trailing snapshot
            // showed the working stretch the gap hid.
            let stale = (kind == Some(NotificationKind::Finished)).then(|| {
                match prev.as_deref().filter(|p| is_busy(Some(*p))) {
                    Some(p) => p.to_owned(),
                    _ => "working".to_owned(),
                }
            });
            if let Some(kind) = kind {
                let event_sound = match kind {
                    NotificationKind::NeedsAttention => Some(NotificationSound::Request),
                    NotificationKind::Finished => sound.filter(|_| !suppress),
                };
                events.push((
                    self.notification_event(
                        term,
                        kind,
                        event_sound,
                        endpoint_id.clone(),
                        workspace_id.clone(),
                        tab_id.clone(),
                    ),
                    stale,
                ));
            } else if let Some(sound) = sound {
                // Suppressed toast, audible request — the sound path is
                // independent of the event, like `forward_agent_notification`.
                let agent = self.known_agent(term, endpoint_id);
                self.emit_notification_sound(sound, agent);
            }
        }
        for (event, stale) in events {
            self.receive_notification(event, stale, now);
        }
    }

    /// `sound.allows` + the env kill-switches, then the host bell — hc-tui's
    /// audio substitute (docs/parity.md).
    pub(super) fn emit_notification_sound(
        &mut self,
        _sound: NotificationSound,
        agent: Option<crate::config::agent::Agent>,
    ) {
        if self.shell.sound.allows(agent) && !super::notifications::sound_disabled_by_env() {
            let _ = (self.host_control)(&crate::terminal_notify::terminal_bells(1));
        }
    }

    fn known_agent(
        &self,
        term: &hc_proto::terminal::Terminal,
        endpoint_id: &ClientEndpointId,
    ) -> Option<crate::config::agent::Agent> {
        self.projection
            .endpoints
            .iter()
            .find(|endpoint| endpoint.endpoint_id == *endpoint_id)
            .and_then(|endpoint| endpoint.snapshot.as_deref())
            .and_then(|snapshot| {
                snapshot
                    .agents
                    .iter()
                    .find(|agent| agent.pane_id == term.address)
            })
            .and_then(|agent| agent.agent.as_deref())
            .and_then(crate::config::agent::parse_agent_label)
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
            agent: self.known_agent(term, &endpoint_id),
            endpoint_id,
            workspace_id: Some(workspace_id),
            tab_id: Some(tab_id),
            pane_id: Some(term.address.clone()),
        }
    }
}
