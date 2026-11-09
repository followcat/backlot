// Derived from herdr src/client/shell/actions.rs and notification_policy.rs (Apache-2.0).
//! Rejected-input notices use the source's three-second lifetime and renderers.

use super::App;
use ratatui::{buffer::Buffer, layout::Rect};
use std::time::{Duration, Instant};

pub(super) struct Notice {
    title: String,
    body: String,
    pub(super) deadline: Instant,
}

/// The reference's `CopyFeedback` — the `copied to clipboard` card.
pub(super) struct CopyFeedback {
    pub message: String,
}

impl App {
    /// The reference's `show_copy_feedback`: two seconds of the copy
    /// card when `[ui.toast.clipboard]` is enabled.
    pub(super) fn show_copy_feedback(&mut self, now: Instant) -> bool {
        if !self.shell.clipboard_toast_enabled {
            return false;
        }
        self.copy_feedback = Some(CopyFeedback {
            message: "copied to clipboard".to_owned(),
        });
        self.copy_feedback_deadline = Some(now + Duration::from_secs(2));
        true
    }

    /// The deadline arm of the reference's `tick_copy_feedback`.
    pub(super) fn tick_copy_feedback(&mut self, now: Instant) -> bool {
        if self
            .copy_feedback_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            self.copy_feedback = None;
            self.copy_feedback_deadline = None;
            return true;
        }
        false
    }

    pub(super) fn reject_input(&mut self, body: &str) {
        self.show_notice("Input rejected", body);
    }
    pub(super) fn show_notice(&mut self, title: &str, body: &str) {
        if self.notice.as_ref().is_some_and(|n| n.body == body) {
            return;
        }
        self.notice = Some(Notice {
            title: title.into(),
            body: body.into(),
            deadline: Instant::now() + Duration::from_secs(3),
        });
    }

    pub(super) fn expire_notice(&mut self, now: Instant) {
        if self.notice.as_ref().is_some_and(|n| now >= n.deadline) {
            self.notice = None;
        }
    }

    /// Installs the one-shot `hc server start` report — `cli` calls it
    /// once terminal setup succeeded.
    pub fn set_core_start(&mut self, report: Option<crate::core_start::Report>) {
        self.core_start = report;
    }

    /// `pending()` while no start was kicked; resolves once with its
    /// outcome — `Some(notice)` means the start failed and is shown.
    pub(super) async fn core_start_report(
        rx: &mut Option<crate::core_start::Report>,
    ) -> Option<crate::core_start::Notice> {
        match rx {
            Some(rx) => rx.await.ok().flatten(),
            None => std::future::pending().await,
        }
    }

    /// One resolution ever: a failed start surfaces as a notice; a
    /// quiet report (started, or nothing to do) just disarms the arm.
    pub(super) fn on_core_start(&mut self, report: Option<crate::core_start::Notice>) {
        self.core_start = None;
        if let Some((title, body)) = report {
            self.show_notice(&title, &body);
        }
    }
}

pub(super) fn render(app: &mut App, buffer: &mut Buffer, area: Rect) {
    app.take_control_hit = Rect::default();
    app.notification_hit = Rect::default();
    let pane = app.pane.notice();
    let offline = match &app.view.link {
        hc_proto::watch::Link::Offline { error, .. } => Some(crate::sidebar::clean(error)),
        hc_proto::watch::Link::Stopped => Some("The inventory connection stopped".into()),
        _ => None,
    };
    let notice = if let Some(notice) = app.notice.as_ref() {
        Some((notice.title.as_str(), notice.body.as_str(), false))
    } else if let Some(notice) = pane.as_ref() {
        Some((notice.title, notice.body.as_str(), notice.take_control))
    } else {
        offline
            .as_ref()
            .map(|error| ("Connection lost", error.as_str(), false))
    };
    let mobile = area.width <= app.shell.mobile_width_threshold;
    let palette = &app.shell.palette;
    // The reference's `if notice else if notification`: the endpoint
    // notice takes precedence; the toast shows only in its absence.
    // Either card composes through the ratatui rebuild, which drops the
    // extended underline bits from everything painted so far.
    if notice.is_some() || app.notifications.visible.is_some() {
        for cell in &mut buffer.content {
            cell.modifier =
                ratatui::style::Modifier::from_bits_truncate(cell.modifier.bits() & !0xf000);
        }
    }
    if let Some((title, body, take_control)) = notice {
        let rect = if mobile {
            crate::chrome::notice_render::render_mobile_notice_banner(
                buffer,
                area,
                title,
                Some(body),
                palette.red,
                false,
                palette,
            )
        } else {
            crate::chrome::notice_render::render_notification_card(
                buffer,
                area,
                title,
                body,
                crate::config::ToastHerdrPosition::TopRight,
                1,
                palette.red,
                palette,
            )
        };
        if take_control {
            app.take_control_hit = rect;
        }
    } else if let Some(notification) = app.notifications.visible.as_ref() {
        app.notification_hit = if mobile {
            crate::chrome::notice_render::render_mobile_notification_banner(
                buffer,
                area,
                notification,
                false,
                palette,
            )
        } else {
            crate::chrome::notice_render::render_visible_notification(
                buffer,
                area,
                notification,
                app.shell.toast_position,
                0,
                palette,
            )
        };
    }
}
