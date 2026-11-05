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

impl App {
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
}

pub(super) fn render(app: &mut App, buffer: &mut Buffer, area: Rect) {
    app.take_control_hit = Rect::default();
    let pane = app.pane.notice();
    let offline = match &app.view.link {
        hc_proto::watch::Link::Offline { error, .. } => Some(crate::sidebar::clean(error)),
        hc_proto::watch::Link::Stopped => Some("The inventory connection stopped".into()),
        _ => None,
    };
    let (title, body, take_control) = if let Some(notice) = app.notice.as_ref() {
        (notice.title.as_str(), notice.body.as_str(), false)
    } else if let Some(notice) = pane.as_ref() {
        (notice.title, notice.body.as_str(), notice.take_control)
    } else if let Some(error) = offline.as_ref() {
        ("Connection lost", error.as_str(), false)
    } else {
        return;
    };
    let palette = &app.shell.palette;
    let rect = if area.width <= app.shell.mobile_width_threshold {
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
            1,
            palette.red,
            palette,
        )
    };
    if take_control {
        app.take_control_hit = rect;
    }
}
