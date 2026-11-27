//! Visible delivery-status line rendered directly under the user's newest
//! `TALK` turn in a studio block (§1.3.2).

use backlot_model::summary::{QueueReason, StudioExchange, Work};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use super::data::StudioRef;
use super::draw_blocks::draw_inner;
use crate::action::ActionTracker;
use crate::chrome::palette::Palette;
use crate::chrome::render::put_text;
use crate::chrome::text::truncate_end;

fn queue_reason_phrase(reason: QueueReason, detail: Option<&str>) -> String {
    match reason {
        QueueReason::Busy => match detail {
            Some(d) => format!("… queued · busy ({d})"),
            None => "… queued · busy".to_owned(),
        },
        QueueReason::Prompt => "… queued · prompt — [y] allow [p] peek".to_owned(),
        QueueReason::RateLimited => match detail {
            Some(d) => format!("… queued · rate limited ({d})"),
            None => "… queued · rate limited".to_owned(),
        },
        QueueReason::OutOfQuota => "… queued · out of quota — [w] recast".to_owned(),
        QueueReason::AuthError => "… queued · auth error — [w] recast".to_owned(),
        QueueReason::Handoff => match detail {
            Some(d) => format!("… queued · handoff ({d})"),
            None => "… queued · handoff".to_owned(),
        },
        QueueReason::Starting => "… queued · starting…".to_owned(),
        QueueReason::WaitingQuiet => "… queued · waiting for quiet screen".to_owned(),
        QueueReason::Down => "… queued · role down".to_owned(),
    }
}

fn deliver_age(ts_ms: Option<u64>, now_ms: u64) -> String {
    let Some(ts) = ts_ms else {
        return "✓ delivered".to_owned();
    };
    if now_ms <= ts {
        return "✓ delivered · just now".to_owned();
    }
    let elapsed_secs = now_ms.saturating_sub(ts) / 1000;
    let unit = if elapsed_secs < 60 {
        format!("{elapsed_secs}s ago")
    } else if elapsed_secs < 3600 {
        format!("{}m ago", elapsed_secs / 60)
    } else {
        format!("{}h ago", elapsed_secs / 3600)
    };
    format!("✓ delivered · {unit}")
}

/// Compute `(label, fg, bold, animates)` for the newest user message in `TALK`.
pub(super) fn resolve(
    studio: StudioRef<'_>,
    line: &StudioExchange,
    stage_running: bool,
    pending: &ActionTracker,
    palette: &Palette,
    now_ms: u64,
) -> (String, Color, bool, bool) {
    let target_role = line.to.as_deref().unwrap_or("producer");
    let role_mark = crate::action::mark::role(&studio.key(), target_role);
    let in_flight = pending
        .pending_label(&role_mark)
        .is_some_and(|l| l.starts_with("telling "))
        || pending
            .pending_label(&crate::action::mark::hollywood())
            .is_some_and(|l| l.starts_with("telling "));
    if in_flight {
        return ("… sending…".to_owned(), palette.accent, false, true);
    }

    let role = studio
        .studio
        .roles
        .iter()
        .find(|r| r.role == target_role);
    let has_queue = studio.studio.pending_tell || role.and_then(|r| r.queue.as_ref()).is_some();

    if has_queue {
        if !stage_running {
            return (
                "… queued · stage stopped — [s] start".to_owned(),
                palette.peach,
                false,
                true,
            );
        }
        if let Some(q) = role.and_then(|r| r.queue.as_ref()) {
            let (fg, bold) = match q.reason {
                QueueReason::OutOfQuota => (palette.red, true),
                QueueReason::Prompt | QueueReason::AuthError => (palette.red, false),
                QueueReason::RateLimited | QueueReason::Down => (palette.peach, false),
                QueueReason::Busy | QueueReason::Handoff => (palette.accent, false),
                QueueReason::Starting | QueueReason::WaitingQuiet => {
                    (palette.elevation().muted_surface, false)
                }
            };
            return (
                queue_reason_phrase(q.reason, q.detail.as_deref()),
                fg,
                bold,
                true,
            );
        }
        return ("… queued".to_owned(), palette.peach, false, true);
    }

    if role.is_some_and(|r| r.state == "live" && r.work == Some(Work::Working)) {
        return ("⠶ thinking…".to_owned(), palette.accent, false, true);
    }

    (
        deliver_age(line.ts_ms, now_ms),
        palette.green,
        false,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_talk_status(
    buf: &mut Buffer,
    rect: Rect,
    studio: StudioRef<'_>,
    line: &StudioExchange,
    stage_running: bool,
    pending: &ActionTracker,
    frame: Style,
    palette: &Palette,
    now_ms: u64,
) -> bool {
    draw_inner(buf, rect, frame);
    let (label, fg, bold, animated) =
        resolve(studio, line, stage_running, pending, palette, now_ms);
    let width = rect.width.saturating_sub(6);
    if width > 0 {
        let mut style = Style::default().fg(fg);
        if bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        put_text(
            buf,
            rect.x + 4,
            rect.y,
            width,
            &truncate_end(&label, width as usize),
            style,
        );
    }
    animated
}
