//! The S3 overlay pair — the action toast card and the native studio
//! dialogs.

use super::*;

/// S3's action toast (§3): the pending reply's card under the notice
/// slot; its chip — `copy cmd` on a failure, `start stage` on the
/// stopped-stage offer — feeds the next mouse press. `true` when it
/// painted.
pub(super) fn draw_action_toast(
    app: &mut App,
    buf: &mut Buffer,
    area: Rect,
    layout: &crate::chrome::snapshot::ClientShellLayout,
) -> bool {
    let Some(toast) = app.action_toast.as_ref() else {
        app.action_toast_hit = Rect::default();
        app.action_chip_hit = Rect::default();
        return false;
    };
    let (toast_area, offset) = if app.plain {
        let offset = if app.notice.is_some() || !app.notification_hit.is_empty() {
            5
        } else {
            1
        };
        (area, offset)
    } else {
        let offset = if app.notice.is_some() || !app.notification_hit.is_empty() {
            4
        } else {
            0
        };
        (app.toast_surface(area, layout), offset)
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
        toast_area,
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

/// Whether the beside Lot Dock should stay highlighted while `hollywood.view`
/// is the only open modal (so `←`/`→` moves `▌` live on `Target::Talk`).
pub(super) fn dock_live_with_reader(app: &App, area: Rect) -> bool {
    app.hollywood.view.is_some()
        && app.panel_beside(area.width)
        && !app.hollywood.expanded()
        && app.sheet.is_none()
        && app.new_studio.is_none()
        && app.hw_setup.is_none()
        && app.studio_dialog.is_none()
        && app.peek.is_none()
        && !app.help
        && app.settings.is_none()
        && app.navigator.is_none()
        && app.dialog.is_none()
        && app.update_modal.is_none()
        && matches!(app.panel.selected, Some(crate::panel::Target::Talk(..)))
}

/// The unified top-level Backlot Reader (`R3-M2`): renders `hollywood.view`
/// (`card show`, `report show`, `hollywood show`, `records`) or an open
/// Task Sheet report (`sheet.view`) centered inside `backlot_modal_surface`
/// with dimmed backdrop. `true` when one painted.
pub(super) fn draw_reader(
    app: &mut App,
    buf: &mut Buffer,
    area: Rect,
    layout: &crate::chrome::snapshot::ClientShellLayout,
) -> bool {
    let card_pending =
        app.card_pending_label(app.hollywood.view.as_ref().and_then(|v| v.card.as_deref()));
    let owned_sheet_view;
    let (from_sheet, view, pending) = if let Some(view) = app.hollywood.view.as_ref() {
        (false, view, card_pending.as_deref())
    } else if let Some(sv) = app.sheet.as_ref().and_then(|s| s.view.as_ref()) {
        owned_sheet_view = crate::hollywood::CardView {
            card: None,
            title: sv.title.clone(),
            lines: sv.lines.clone(),
            raw_body: sv.raw_body.clone(),
            scroll: sv.scroll,
            meta: sv.meta.clone(),
            tell: sv.tell.clone(),
        };
        (true, &owned_sheet_view, None)
    } else {
        app.hollywood_hits.view = None;
        return false;
    };
    let exclude = (app.panel_beside(area.width)
        && app.sheet.is_none()
        && !app.hollywood.expanded()
        && !app.panel_hits.area.is_empty())
    .then_some(app.panel_hits.area);
    crate::chrome::popup_frame::dim_backdrop(
        buf,
        &app.shell.palette,
        app.shell.popup_backdrop,
        exclude,
    );
    let surface = app.backlot_modal_surface(area, layout, 64);
    let compose_info = app.reader_compose_draw_ctx(from_sheet);
    let compose_opts =
        compose_info.as_ref().map(
            |(target, studio, editor)| crate::hollywood::ReaderComposeOpts {
                editor: *editor,
                target: target.as_str(),
                studio: *studio,
            },
        );
    let ren = crate::hollywood::render_view_with_opts(
        buf,
        surface,
        view,
        pending,
        &app.shell.palette,
        crate::hollywood::ReaderRenderOpts {
            compose: compose_opts,
            now_ms: crate::update_state::now_ms(),
            shadow: app.shell.popup_shadow,
        },
    );
    if let Some(cursor) = ren.cursor.as_ref() {
        app.want_cursor = CursorWish {
            pos: cursor.visible.then_some((cursor.x, cursor.y)),
            anchor: Some((cursor.x, cursor.y)),
            shape: CursorShape::Default,
            blinking: true,
        };
    } else {
        app.want_cursor.pos = None;
        app.want_cursor.anchor = None;
    }
    app.hollywood_hits.view = Some(ren);
    true
}

/// The S3 native dialogs (§2): dimmed background, centred inside
/// `backlot_modal_surface` — beside a dock they never cross its border
/// (A6.2); their field cursor owns the host cursor. `true` when one painted.
pub(super) fn draw_studio_dialog(
    app: &mut App,
    buf: &mut Buffer,
    area: Rect,
    layout: &crate::chrome::snapshot::ClientShellLayout,
) -> bool {
    app.studio_dialog_hits = if let Some(d) = app.studio_dialog.as_ref() {
        crate::chrome::popup_frame::dim_backdrop(
            buf,
            &app.shell.palette,
            app.shell.popup_backdrop,
            None,
        );
        crate::chrome::studio_dialogs::render_with_shadow(
            buf,
            app.backlot_modal_surface(area, layout, 64),
            d,
            &app.shell.palette,
            app.shell.popup_shadow,
        )
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
