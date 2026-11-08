// Derived from herdr src/client/shell/notification_policy.rs,
// src/client/shell/notifications.rs and src/app/actions.rs, plus the
// herdr-app src/model.rs `alert` rule (Apache-2.0).
//! Synthesized agent notifications: hc's core emits no notification events,
//! so alerts are derived from status-word transitions on our panes, then run
//! through the reference client pipeline — settle, validate, queue, TTL.

use super::*;
use crate::chrome::snapshot::ClientEndpointId;
use crate::chrome::status::AgentStatus;
use crate::config::{ToastDelivery, agent::Agent};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

const MAX_QUEUED_NOTIFICATIONS: usize = 8;
/// How long a completion alert waits for snapshot evidence before it drops.
const COMPLETION_EVIDENCE_GRACE: Duration = Duration::from_secs(1);
const COMPLETION_RECHECK_INTERVAL: Duration = Duration::from_millis(50);

/// `HERDR_DISABLE_SOUND`/`NEXTEST` — the reference playback kill-switch.
pub(super) fn sound_disabled_by_env() -> bool {
    std::env::var_os("HERDR_DISABLE_SOUND").is_some() || std::env::var_os("NEXTEST").is_some()
}

/// The wire `SemanticNotificationKind` minus variants hc-tui never emits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NotificationKind {
    NeedsAttention,
    Finished,
}

/// The reference `SemanticNotificationSound`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NotificationSound {
    Done,
    Request,
}

/// The reference `SemanticNotification`, synthesized from pane status words.
pub(crate) struct NotificationEvent {
    pub kind: NotificationKind,
    pub title: String,
    pub body: Option<String>,
    pub sound: Option<NotificationSound>,
    /// Canonical agent — `[ui.sound.agents]` override lookup.
    pub agent: Option<Agent>,
    pub endpoint_id: ClientEndpointId,
    pub workspace_id: Option<String>,
    pub tab_id: Option<String>,
    pub pane_id: Option<String>,
}

/// A shown or queued toast — the reference `ClientVisibleNotification`.
pub(crate) struct Notification {
    pub event: NotificationEvent,
    pub deadline: Instant,
}

pub(super) struct PendingNotification {
    event: NotificationEvent,
    deadline: Instant,
    expires_at: Instant,
    validate_state: bool,
    /// The projected status the reference's trailing snapshot would have
    /// shown at send time — consulted on the first validation, then
    /// cleared once the event survives to a later tick.
    stale_status: Option<String>,
}

/// Per-pane synthesis baseline: the last word, the last `turn` and the
/// pane's incarnation (a reincarnation resets both). The word diff itself
/// is the dedup — one alert per transition — and `turn` catches
/// completions compressed inside a reconnect gap.
pub(super) struct PaneAlert {
    pub created_ms: u64,
    pub word: Option<String>,
    pub turn: u64,
}

/// The reference client notification state.
#[derive(Default)]
pub(crate) struct Notifications {
    pub(super) pending: Vec<PendingNotification>,
    pub(crate) visible: Option<Notification>,
    pub(super) queued: VecDeque<Notification>,
    pub(super) panes: HashMap<String, PaneAlert>,
    /// The subscription epoch the current events and baselines belong to;
    /// a fresh inventory (reconnect, restarted core) retires them all —
    /// a client attaches to a new snapshot with an empty queue.
    pub(super) generation: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Validation {
    Current,
    AwaitingSnapshot,
    Stale,
}

/// The herdr-app `busy` set — words that mean "still on the job".
pub(super) fn is_busy(word: Option<&str>) -> bool {
    matches!(word, Some("working" | "blocked" | "error"))
}

/// The herdr-app `blocked` set — words that mean "needs a person".
pub(super) fn is_blocked(word: Option<&str>) -> bool {
    matches!(word, Some("blocked" | "error"))
}

fn notification_duration(kind: NotificationKind) -> Duration {
    Duration::from_secs(match kind {
        NotificationKind::NeedsAttention => 8,
        NotificationKind::Finished => 5,
    })
}

impl Notifications {
    fn next_deadline(&self) -> Option<Instant> {
        self.visible
            .as_ref()
            .map(|visible| visible.deadline)
            .into_iter()
            .chain(self.pending.iter().map(|pending| pending.deadline))
            .min()
    }

    fn queue_visible(&mut self, mut notification: Notification, now: Instant) {
        if self.visible.is_none() {
            notification.deadline = now + notification_duration(notification.event.kind);
            self.visible = Some(notification);
            return;
        }
        if self.queued.len() == MAX_QUEUED_NOTIFICATIONS {
            self.queued.pop_front();
        }
        self.queued.push_back(notification);
    }

    pub(super) fn promote_queued(&mut self, now: Instant) -> bool {
        let Some(mut notification) = self.queued.pop_front() else {
            return false;
        };
        notification.deadline = now + notification_duration(notification.event.kind);
        self.visible = Some(notification);
        true
    }

    /// Retire every notification targeting a dead endpoint or pane.
    pub(super) fn retire(
        &mut self,
        endpoints: &HashSet<ClientEndpointId>,
        panes: &HashSet<String>,
    ) {
        let keep = |event: &NotificationEvent| {
            endpoints.contains(&event.endpoint_id)
                && event.pane_id.as_deref().is_none_or(|id| panes.contains(id))
        };
        self.pending.retain(|pending| keep(&pending.event));
        self.queued.retain(|queued| keep(&queued.event));
        if self
            .visible
            .as_ref()
            .is_some_and(|visible| !keep(&visible.event))
        {
            self.visible = None;
            self.promote_queued(Instant::now());
        }
    }
}

impl App {
    /// `receive_notification`: same-pane dedupe across pending, queued and
    /// visible, then the settle deadline and immediate tick. `stale` is the
    /// status the reference's trailing snapshot would have shown at send
    /// time — the synthesized stand-in for its out-of-order delivery.
    pub(super) fn receive_notification(
        &mut self,
        event: NotificationEvent,
        stale: Option<String>,
        now: Instant,
    ) {
        let delay = self.shell.toast_delay_seconds;
        let deadline = now.checked_add(Duration::from_secs(delay)).unwrap_or(now);
        let cleared_visible = event.pane_id.as_deref().is_some_and(|pane_id| {
            self.notifications
                .visible
                .as_ref()
                .is_some_and(|visible| visible.event.pane_id.as_deref() == Some(pane_id))
        });
        if let Some(pane_id) = event.pane_id.as_deref() {
            self.notifications
                .pending
                .retain(|pending| pending.event.pane_id.as_deref() != Some(pane_id));
            self.notifications
                .queued
                .retain(|queued| queued.event.pane_id.as_deref() != Some(pane_id));
            if cleared_visible {
                self.notifications.visible = None;
                self.notifications.promote_queued(now);
            }
        }
        let validate_state = delay > 0 || event.kind == NotificationKind::Finished;
        self.notifications.pending.push(PendingNotification {
            event,
            deadline,
            expires_at: now.checked_add(COMPLETION_EVIDENCE_GRACE).unwrap_or(now),
            validate_state,
            stale_status: stale,
        });
        self.tick_notifications(now);
    }

    /// `tick_notifications`: expire the visible toast, promote the queue,
    /// then validate and deliver every due pending.
    pub(super) fn tick_notifications(&mut self, now: Instant) {
        if self
            .notifications
            .visible
            .as_ref()
            .is_some_and(|visible| now >= visible.deadline)
        {
            self.notifications.visible = None;
            self.notifications.promote_queued(now);
        }
        let pending = std::mem::take(&mut self.notifications.pending);
        for mut pending in pending {
            if pending.deadline > now {
                // The settle window stands in for the reference's wire
                // round-trip: past it, the snapshot has caught up and the
                // trailing status no longer applies.
                pending.stale_status = None;
                self.notifications.pending.push(pending);
                continue;
            }
            if pending.validate_state {
                match self.notification_validation(&pending.event, pending.stale_status.as_deref())
                {
                    Validation::Current => {}
                    Validation::AwaitingSnapshot if now < pending.expires_at => {
                        pending.deadline = now
                            .checked_add(COMPLETION_RECHECK_INTERVAL)
                            .unwrap_or(now)
                            .min(pending.expires_at);
                        pending.stale_status = None;
                        self.notifications.pending.push(pending);
                        continue;
                    }
                    Validation::AwaitingSnapshot | Validation::Stale => {
                        continue;
                    }
                }
            }
            let target_active = self.notification_target_is_active(&pending.event);
            let suppress_external = target_active && self.outer_focused != Some(false);
            if let Some(sound) = pending.event.sound {
                let suppress_sound =
                    pending.event.kind == NotificationKind::Finished && suppress_external;
                if !suppress_sound {
                    self.emit_notification_sound(sound, pending.event.agent);
                }
            }
            match self.shell.toast_delivery {
                ToastDelivery::Off => {}
                ToastDelivery::Herdr if !target_active => {
                    self.notifications.queue_visible(
                        Notification {
                            event: pending.event,
                            deadline: now,
                        },
                        now,
                    );
                }
                ToastDelivery::Herdr => {}
                ToastDelivery::Terminal | ToastDelivery::System if !suppress_external => {
                    // `System` falls back to the terminal OSC path — hc-tui
                    // has no desktop notification surface (docs/parity.md).
                    let message = format!(
                        "{}: {}",
                        pending.event.title,
                        pending.event.body.as_deref().unwrap_or_default()
                    );
                    let (title, body) = crate::terminal_notify::split_message(&message);
                    if let Some(bytes) = crate::terminal_notify::notification_bytes(
                        self.notify_backend,
                        self.in_tmux,
                        title,
                        body.filter(|body| !body.is_empty()),
                    ) {
                        let _ = (self.host_control)(&bytes);
                    }
                }
                ToastDelivery::Terminal | ToastDelivery::System => {}
            }
        }
    }

    /// `notification_target_is_active`: the event's tab (else workspace) is
    /// the focused one on its own endpoint's snapshot.
    fn notification_target_is_active(&self, event: &NotificationEvent) -> bool {
        if event.endpoint_id != self.projection.active_endpoint_id {
            return false;
        }
        let Some(snapshot) = self
            .projection
            .endpoints
            .iter()
            .find(|endpoint| endpoint.endpoint_id == event.endpoint_id)
            .and_then(|endpoint| endpoint.snapshot.as_deref())
        else {
            return false;
        };
        if let Some(tab_id) = event.tab_id.as_deref() {
            return snapshot.focused_tab_id.as_deref() == Some(tab_id);
        }
        event.workspace_id.as_deref().is_some_and(|workspace_id| {
            snapshot.focused_workspace_id.as_deref() == Some(workspace_id)
        })
    }

    /// `notification_validation` against the projected `agent_status`:
    /// NeedsAttention is current on a blocked word; Finished is current
    /// only when the pane reads Done — a working read (or a missing agent)
    /// waits for the next snapshot inside the grace, anything else drops.
    /// The reference's snapshot trails the event by one update, so the
    /// first pass consults `stale` — the pre-transition word — instead.
    fn notification_validation(
        &self,
        event: &NotificationEvent,
        stale: Option<&str>,
    ) -> Validation {
        let Some(pane_id) = event.pane_id.as_deref() else {
            return if event.kind == NotificationKind::Finished {
                Validation::Stale
            } else {
                Validation::Current
            };
        };
        match event.kind {
            NotificationKind::NeedsAttention => {
                let Some(term) = self.view.inventory.get(pane_id) else {
                    return Validation::AwaitingSnapshot;
                };
                let word = crate::project::effective_status(term).map(|s| s.state.as_str());
                if is_blocked(word) {
                    Validation::Current
                } else {
                    Validation::Stale
                }
            }
            NotificationKind::Finished => {
                // `stale` models the snapshot the reference client had when
                // the event arrived; once cleared, the projected status —
                // already carrying kept-done presentation — speaks.
                let status = match stale
                    .map(crate::chrome::status::agent_status_from_word)
                    .unwrap_or(AgentStatus::Working)
                {
                    AgentStatus::Working => {
                        self.projected_agent_status(&event.endpoint_id, pane_id)
                    }
                    stale_status => Some(stale_status),
                };
                match status {
                    Some(AgentStatus::Done) => Validation::Current,
                    Some(AgentStatus::Working) | None => Validation::AwaitingSnapshot,
                    _ => Validation::Stale,
                }
            }
        }
    }

    /// `agent.agent_status` on the endpoint's snapshot — the projected
    /// status the reference client reads at validation.
    fn projected_agent_status(
        &self,
        endpoint_id: &ClientEndpointId,
        pane_id: &str,
    ) -> Option<AgentStatus> {
        self.projection
            .endpoints
            .iter()
            .find(|endpoint| endpoint.endpoint_id == *endpoint_id)
            .and_then(|endpoint| endpoint.snapshot.as_deref())
            .and_then(|snapshot| {
                snapshot
                    .agents
                    .iter()
                    .find(|agent| agent.pane_id == pane_id)
            })
            .map(|agent| agent.agent_status)
    }

    /// The next notification deadline — folded into the runtime wake set.
    pub(super) fn notification_deadline(&self) -> Option<Instant> {
        self.notifications.next_deadline()
    }
}
