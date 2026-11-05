//! Core-specific connection/ownership facts for Source notice presentation.
use super::*;

pub(crate) struct PaneNotice {
    pub title: &'static str,
    pub body: String,
    pub take_control: bool,
}

impl Pane {
    // Derive this from the latest request and report. A cached flag can stay
    // stale when another viewer reaches our new allocation before we send it.
    pub(super) fn has_size_mismatch(&self) -> bool {
        matches!(&self.state, State::Live(live)
            if live.spec.role == Role::Active
                && live.core_grid != live.sent_size.unwrap_or(self.allocated))
    }

    pub(crate) fn notice(&self) -> Option<PaneNotice> {
        let (title, body, take_control) = if let Some(error) = &self.close_error {
            ("Input drain failed", error.clone(), false)
        } else {
            match &self.state {
                State::Live(live) if live.await_paint => (
                    "Terminal loading",
                    "Waiting for terminal state before forwarding input".into(),
                    false,
                ),
                State::Live(live) if live.vt_broken.is_some() => (
                    "Terminal error",
                    live.vt_broken.clone().unwrap_or_default(),
                    false,
                ),
                State::Live(_) if self.has_size_mismatch() => (
                    "Terminal size",
                    "Another viewer may own the size. Click to take control.".into(),
                    true,
                ),
                State::Down { reason, .. } => ("Terminal disconnected", reason.clone(), false),
                State::Gone { reason, .. } => ("Terminal unavailable", reason.clone(), false),
                State::Opening { .. } if self.vt.is_some() => (
                    "Reconnecting",
                    "Waiting for a fresh terminal paint".into(),
                    false,
                ),
                _ => return None,
            }
        };
        Some(PaneNotice {
            title,
            body: crate::sidebar::clean(&body),
            take_control,
        })
    }

    /// An explicit user request bypasses resize coalescing for a previous claim.
    /// Ownership remains only a suspicion until the core reports our grid.
    pub(crate) fn take_control(&mut self) {
        if let State::Live(live) = &mut self.state {
            if live.spec.role != Role::Active {
                return;
            }
            live.sent_size = None;
            live.desired_size = Some(self.allocated);
            self.push_controls();
        }
    }
}
