//! W2's App wiring for studio notifications (N1): derive the events
//! every publish implies and announce each one exactly once.
//!
//! The derivation is pure (`events`); this file owns the "seen" set and
//! the seeding rule. Everything rides the client's one notification
//! pipeline — settle, validate, queue, TTL — so a studio event sounds
//! and shows exactly like a pane alert.

mod events;

pub(crate) use events::{StudioEvent, StudioEventKind, derive};

use std::time::Instant;

use crate::app::notifications::{NotificationEvent, NotificationKind, NotificationSound};
use crate::chrome::snapshot::ClientEndpointId;

/// Which sound an event makes: everything that asks for the person
/// requests attention; a studio ready to finish is a completion.
pub(super) fn sound(kind: StudioEventKind) -> NotificationSound {
    match kind {
        StudioEventKind::Ready => NotificationSound::Done,
        _ => NotificationSound::Request,
    }
}

/// Which toast kind an event draws: `Finished` only for the ready
/// studio, so its card reads calm.
pub(super) fn notification_kind(kind: StudioEventKind) -> NotificationKind {
    match kind {
        StudioEventKind::Ready => NotificationKind::Finished,
        _ => NotificationKind::NeedsAttention,
    }
}

impl super::App {
    /// Compare the documents with the last publish and announce every
    /// studio event that is new. The first non-empty read seeds the
    /// seen set silently — what predates the client is not news — and a
    /// fresh subscription epoch reseeds, like the pane baselines.
    pub(super) fn synthesize_studio_events(&mut self, now: Instant) {
        if self.plain {
            return;
        }
        if self.studio_seen_generation != Some(self.view.generation) {
            self.studio_seen_generation = Some(self.view.generation);
            self.studio_seen.clear();
            self.studio_seeded = false;
        }
        let machines = self.panel_summaries();
        if machines.is_empty() {
            // An empty aggregate is not a baseline: a document that has
            // not arrived yet must not latch the seed.
            return;
        }
        let status = crate::hollywood::status_of(&self.view);
        let derived = derive(&machines, &status);
        drop(status);
        if !self.studio_seeded {
            self.studio_seen
                .extend(derived.into_iter().map(|event| event.key));
            self.studio_seeded = true;
            return;
        }
        for event in derived {
            if !self.studio_seen.insert(event.key.clone()) {
                continue;
            }
            self.receive_notification(studio_notification(event), None, now);
        }
    }
}

/// A studio event on the wire pipeline's shape — no pane, no endpoint,
/// and the target the toast's key and click later open.
fn studio_notification(event: StudioEvent) -> NotificationEvent {
    NotificationEvent {
        kind: notification_kind(event.kind),
        title: event.title,
        body: event.body,
        sound: Some(sound(event.kind)),
        agent: None,
        endpoint_id: ClientEndpointId::Local,
        workspace_id: None,
        tab_id: None,
        pane_id: None,
        studio: Some(event.target),
    }
}
