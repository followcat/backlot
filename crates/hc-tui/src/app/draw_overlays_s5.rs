//! S5's `Ctrl+B ?` composition (split from `draw_overlays.rs` to
//! keep it under the file-size cap): the studio action launcher —
//! with its floating target picker — when studios exist or
//! `[hollywood]` is configured, else herdr's own keybind overlay
//! (D6 covers `--plain` and the no-studio frame).

use super::App;

/// The help overlay's draw: the launcher or herdr's reference, the
/// picker floating inside the launcher's panel when a `…` row
/// opened one.
pub(super) fn render_help(
    app: &mut App,
    buf: &mut ratatui::buffer::Buffer,
    area: ratatui::layout::Rect,
    layout: &crate::chrome::snapshot::ClientShellLayout,
) -> crate::chrome::help::HelpRender {
    if !app.help_launcher() {
        crate::chrome::popup_frame::dim_backdrop(
            buf,
            &app.shell.palette,
            app.shell.popup_backdrop,
            None,
        );
        return crate::chrome::help::render_help_overlay_with_shadow(
            buf,
            &app.help_state,
            &app.live,
            app.studio.active.then(|| {
                crate::keybind_help::backlot_help_groups(
                    &app.live.keybinds,
                    &app.live.prefix,
                    &app.panel_config,
                    app.hollywood_key.unwrap_or_default(),
                    app.panel_key_combo,
                )
            }),
            &app.shell.palette,
            app.shell.popup_shadow,
        )
        .unwrap_or_default();
    }
    // S5's action launcher: runnable rows on top, herdr's reference
    // sections under them, the glyph legend pinned. `backlot_modal_surface`
    // centers it inside `pane_surface` (falling back on narrow windows).
    // When `picker` is open, paint the launcher first, then dim ONCE so
    // both background and launcher attenuate under the foreground picker
    // without compounding.
    let picker_open = app.help_state.picker.is_some();
    if !picker_open {
        crate::chrome::popup_frame::dim_backdrop(
            buf,
            &app.shell.palette,
            app.shell.popup_backdrop,
            None,
        );
    }
    let groups = app.help_filtered_groups();
    let legend = app.help_legend();
    let mut hits = crate::chrome::help_launcher::render_launcher_with_shadow(
        buf,
        app.backlot_modal_surface(area, layout, 64),
        &app.help_state,
        &groups,
        &legend,
        &app.shell.palette,
        app.shell.popup_shadow,
    )
    .unwrap_or_default();
    if let Some(picker) = app.help_state.picker.as_ref() {
        crate::chrome::popup_frame::dim_backdrop(
            buf,
            &app.shell.palette,
            app.shell.popup_backdrop,
            None,
        );
        let inside = if !hits.inner.is_empty() {
            hits.inner
        } else {
            ratatui::layout::Rect::new(
                hits.area.x + 1,
                hits.area.y + 1,
                hits.area.width.saturating_sub(2),
                hits.area.height.saturating_sub(2),
            )
        };
        crate::chrome::help_picker::render_picker_with_shadow(
            buf,
            inside,
            picker,
            &mut hits,
            &app.shell.palette,
            app.shell.popup_shadow,
        );
    }
    hits
}
