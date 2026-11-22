//! The S3 overlay pair — the action toast card and the native studio
//! dialogs — plus the dim pass every modal shares.

use super::*;

/// The dim every modal paints under itself.
pub(super) fn dim_cells(buf: &mut Buffer) {
    for cell in &mut buf.content {
        cell.modifier = Modifier::from_bits_truncate(cell.modifier.bits() & !0xf000);
        cell.set_style(cell.style().add_modifier(Modifier::DIM));
    }
}

/// S3's action toast (§3): the pending reply's card under the notice
/// slot; its chip — `copy cmd` on a failure, `start stage` on the
/// stopped-stage offer — feeds the next mouse press. `true` when it
/// painted.
pub(super) fn draw_action_toast(app: &mut App, buf: &mut Buffer, area: Rect) -> bool {
    let Some(toast) = app.action_toast.as_ref() else {
        app.action_toast_hit = Rect::default();
        app.action_chip_hit = Rect::default();
        return false;
    };
    let offset = if app.notice.is_some() || !app.notification_hit.is_empty() {
        5
    } else {
        1
    };
    let (body, chip) = if toast.fail {
        (
            Some(toast.body.as_str()),
            Some(crate::chrome::notice_render::ToastChip::CopyCmd),
        )
    } else if toast.start_stage.is_some() {
        (
            Some(toast.body.as_str()),
            Some(crate::chrome::notice_render::ToastChip::StartStage),
        )
    } else {
        (None, None)
    };
    let (rect, chip_hit) = crate::chrome::notice_render::render_action_toast(
        buf,
        area,
        &toast.title,
        body,
        chip,
        offset,
        &app.shell.palette,
    );
    app.action_toast_hit = rect;
    app.action_chip_hit = chip_hit;
    true
}

/// The S3 native dialogs (§2): dimmed background, centred inside
/// `modal_band` — beside a dock they never cross its border (A6.2);
/// their field cursor owns the host cursor. `true` when one painted.
pub(super) fn draw_studio_dialog(app: &mut App, buf: &mut Buffer, area: Rect) -> bool {
    app.studio_dialog_hits = if let Some(d) = app.studio_dialog.as_ref() {
        dim_cells(buf);
        crate::chrome::studio_dialogs::render(buf, app.modal_band(area), d, &app.shell.palette)
            .unwrap_or_default()
    } else {
        crate::chrome::studio_dialogs::DialogHits::default()
    };
    if let Some(cursor) = app.studio_dialog_hits.cursor.as_ref() {
        app.want_cursor = CursorWish {
            pos: cursor.visible.then_some((cursor.x, cursor.y)),
            anchor: Some((cursor.x, cursor.y)),
            shape: CursorShape::Default,
            blinking: true,
        };
    }
    app.studio_dialog.is_some()
}
