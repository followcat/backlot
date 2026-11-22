// Derived from herdr src/client/shell/actions.rs and notification_policy.rs (Apache-2.0).
//! Rejected-input notices use the source's three-second lifetime and renderers.

use super::App;
use ratatui::{buffer::Buffer, layout::Rect};
use std::time::{Duration, Instant};

pub(super) struct Notice {
    title: String,
    body: String,
    pub(super) deadline: Instant,
    /// Accent dot + click-opens-update semantics (the update toast);
    /// plain rejected-input notices are the red default.
    pub(super) update: bool,
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
            update: false,
        });
        self.sched.mark_dirty(Instant::now());
    }

    /// The newer release the passive UI (badge, dot, modebar, toast)
    /// advertises — `None` when up to date or when the user skipped
    /// exactly this version. The modal still reaches a skipped release
    /// through `update_latest`.
    pub(crate) fn update_ready_version(&self) -> Option<&str> {
        let latest = self.update_latest.as_deref()?;
        (self.update_state.skipped_version.as_deref() != Some(latest)).then_some(latest)
    }

    /// The one-time-per-version update toast: accent dot, five
    /// seconds, click opens the modal. Raising it persists
    /// `seen_toast_version` so the same release never toasts twice.
    pub(super) fn maybe_toast_update(&mut self) {
        let Some(latest) = self.update_ready_version().map(str::to_owned) else {
            return;
        };
        if self.update_state.seen_toast_version.as_deref() == Some(latest.as_str()) {
            return;
        }
        let mobile = self.area.0 <= self.shell.mobile_width_threshold;
        let (title, body) =
            crate::update_check::toast(&latest, &self.live.primary_prefix_label(), mobile);
        self.show_update_toast(&title, &body);
        self.update_state.seen_toast_version = Some(latest);
        self.persist_update_state();
    }

    /// The update toast card: accent dot, five seconds, click-to-open.
    pub(super) fn show_update_toast(&mut self, title: &str, body: &str) {
        self.notice = Some(Notice {
            title: title.into(),
            body: body.into(),
            deadline: Instant::now() + Duration::from_secs(5),
            update: true,
        });
        self.sched.mark_dirty(Instant::now());
    }

    /// Best effort — a failed write retries on the next mutation.
    pub(super) fn persist_update_state(&mut self) {
        if let Some(path) = &self.update_state_path {
            crate::update_state::store(path, &self.update_state);
        }
    }

    /// `true` when the visible notice cleared — the card un-paints.
    pub(super) fn expire_notice(&mut self, now: Instant) -> bool {
        if self.notice.as_ref().is_some_and(|n| now >= n.deadline) {
            self.notice = None;
            return true;
        }
        false
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

    /// Installs the one-shot update-check report — `cli` calls it once
    /// terminal setup succeeded. `None` means the user opted out.
    pub fn set_update_check(&mut self, report: Option<crate::update_check::Report>) {
        self.update_check = report;
    }

    /// `pending()` while no check was armed; resolves once with its
    /// outcome.
    pub(super) async fn update_check_report(
        rx: &mut Option<crate::update_check::Report>,
    ) -> Option<crate::update_check::CheckReport> {
        match rx {
            Some(rx) => rx.await.ok(),
            None => std::future::pending().await,
        }
    }

    /// The report lands: the cache state becomes authoritative, a
    /// newer release arms the badge and the one-time toast. A quiet
    /// report (up to date, offline, opted out) just disarms the arm.
    pub(super) fn on_update_check(&mut self, report: Option<crate::update_check::CheckReport>) {
        self.update_check = None;
        let Some(report) = report else { return };
        self.update_state = report.state;
        self.update_state_path = report.path;
        self.update_latest = report.latest;
        self.maybe_toast_update();
        // Prefetch the notes so the modal opens instantly from cache.
        self.kick_notes_fetch(false);
        self.maybe_open_whats_new();
    }
}

/// `true` when a notice or notification card painted — the draw pass
/// counts it as overlay damage over the pane surfaces.
pub(super) fn render(
    app: &mut App,
    buffer: &mut Buffer,
    area: Rect,
    layout: &crate::chrome::snapshot::ClientShellLayout,
) -> bool {
    app.take_control_hit = Rect::default();
    app.notification_hit = Rect::default();
    app.update_toast_hit = Rect::default();
    let pane = app.pane.notice();
    let offline = match &app.view.link {
        hc_proto::watch::Link::Offline { error, .. } => Some(crate::sidebar::clean(error)),
        hc_proto::watch::Link::Stopped => Some("The inventory connection stopped".into()),
        _ => None,
    };
    let notice = if let Some(notice) = app.notice.as_ref() {
        Some((
            notice.title.as_str(),
            notice.body.as_str(),
            false,
            notice.update,
        ))
    } else if let Some(notice) = pane.as_ref() {
        Some((
            notice.title,
            notice.body.as_str(),
            notice.take_control,
            false,
        ))
    } else {
        offline
            .as_ref()
            .map(|error| ("Connection lost", error.as_str(), false, false))
    };
    let mobile = area.width <= app.shell.mobile_width_threshold;
    let (render_area, card_top_offset) = if app.plain {
        (area, 1)
    } else {
        (app.toast_surface(area, layout), 0)
    };
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
    if let Some((title, body, take_control, update)) = notice {
        let dot = if update { palette.accent } else { palette.red };
        let rect = if mobile {
            crate::chrome::notice_render::render_mobile_notice_banner(
                buffer,
                render_area,
                title,
                Some(body),
                dot,
                false,
                palette,
            )
        } else {
            crate::chrome::notice_render::render_notification_card(
                buffer,
                render_area,
                title,
                body,
                crate::config::ToastHerdrPosition::TopRight,
                card_top_offset,
                dot,
                palette,
            )
        };
        if take_control {
            app.take_control_hit = rect;
        }
        if update {
            app.update_toast_hit = rect;
        }
    } else if let Some(notification) = app.notifications.visible.as_ref() {
        app.notification_hit = if mobile {
            crate::chrome::notice_render::render_mobile_notification_banner(
                buffer,
                render_area,
                notification,
                false,
                palette,
            )
        } else {
            crate::chrome::notice_render::render_visible_notification(
                buffer,
                render_area,
                notification,
                app.shell.toast_position,
                0,
                palette,
            )
        };
    }
    notice.is_some() || app.notifications.visible.is_some()
}
