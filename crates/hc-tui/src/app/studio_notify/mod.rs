//! W2's App wiring for studio notifications (N1): derive the events
//! every publish implies, announce each one once, and let the toast's
//! key or click open its target.
//!
//! The derivation is pure (`events`); this file owns the "seen" set,
//! the seeding rule and the jump. Everything rides the client's one
//! notification pipeline — settle, validate, queue, TTL — so a studio
//! event sounds and shows exactly like a pane alert.

mod events;

pub(crate) use events::{StudioEvent, StudioEventKind, derive};

use std::time::Instant;

use crate::app::notifications::{NotificationEvent, NotificationKind, NotificationSound};
use crate::chrome::snapshot::ClientEndpointId;
use crate::hollywood::Target;

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
        let toast = !self.plain && self.shell.toast_studio;
        let ping = !self.plain && self.shell.studio_notify_asks;
        if !toast && !ping {
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
            // One desktop ping per new item, independent of toast
            // delivery and of whether the person is looking at it.
            if ping && let Some(text) = event.ping.clone() {
                self.ping_ask(&text);
            }
            if !toast {
                continue;
            }
            if target_is_focused(self, &event.target) {
                // Looking right at it: the same rule pane alerts follow.
                continue;
            }
            self.receive_notification(studio_notification(event), None, now);
        }
    }

    /// The person asked to be told when a studio waits on them: one
    /// desktop notification through the outer terminal, once per new
    /// item (the caller only pings an unseen key). Independent of the
    /// toast delivery — `[studio] notify_asks` is the switch — and
    /// emitted before the focused-target suppression, so being on the
    /// screen does not silence the ping.
    fn ping_ask(&mut self, text: &str) {
        let (title, body) = crate::terminal_notify::split_message(text);
        if let Some(bytes) = crate::terminal_notify::notification_bytes(
            self.notify_backend,
            self.in_tmux,
            title,
            body.filter(|body| !body.is_empty()),
        ) {
            let _ = (self.host_control)(&bytes);
        }
    }
}

/// A studio event on the wire pipeline's shape — no pane, no endpoint,
/// and the target the toast's key and click open.
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

impl super::App {
    /// This core's active live Hollywood address — the read the Call
    /// Sheet's Enter takes.
    pub(super) fn visible_hollywood_terminal(&self) -> Option<String> {
        self.hollywood_terminal()
    }
}

/// Whether the person is already looking at `target` — the studio half
/// of `notification_target_is_active`.
pub(super) fn target_is_focused(app: &super::App, target: &Target) -> bool {
    let focused_in = |key: &crate::studio::StudioKey| {
        let Some(focused) = app.focused.as_deref() else {
            return false;
        };
        let machines = app.panel_summaries();
        crate::panel::data::studio(&machines, key).is_some_and(|found| {
            found
                .studio
                .roles
                .iter()
                .filter_map(|role| role.terminal.as_deref())
                .any(|terminal| terminal == focused)
        })
    };
    match target {
        Target::Prompt(studio, role) | Target::Role(studio, role) => {
            let machines = app.panel_summaries();
            crate::panel::data::studio(&machines, studio)
                .and_then(|found| {
                    found
                        .studio
                        .roles
                        .iter()
                        .find(|r| r.role == *role)
                        .and_then(|r| r.terminal.clone())
                })
                .is_some_and(|terminal| app.focused.as_deref() == Some(terminal.as_str()))
        }
        Target::Hollywood | Target::HollywoodLive | Target::Headline | Target::Talk(_) => app
            .visible_hollywood_terminal()
            .is_some_and(|terminal| app.focused.as_deref() == Some(terminal.as_str())),
        Target::Card(id) => {
            app.hollywood.expanded() && app.hollywood.selected == Some(Target::Card(id.clone()))
        }
        Target::OutwardCard(key, id) => {
            app.hollywood.expanded()
                && app.hollywood.selected == Some(Target::OutwardCard(key.clone(), id.clone()))
        }
        Target::Ready(key)
        | Target::Tell(key)
        | Target::Upgrade(key)
        | Target::Halted(key)
        | Target::Decision(key) => app.studio.current.as_ref() == Some(key) && focused_in(key),
        Target::Stage(machine, false) => {
            app.panel.focused
                && app.panel.selected == Some(crate::panel::Target::Stage(machine.clone()))
        }
        _ => false,
    }
}
