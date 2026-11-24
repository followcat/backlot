//! Overlay composition and host-cursor resolution — everything that
//! paints over the pane surfaces after their cells land: the mode bar,
//! mobile switcher, help/diagnostic/feedback cards, popups, notices,
//! navigator, settings, dialogs and the context menu. Returns the mode
//! bar's rect plus whether *any* overlay painted, so the wide-tail
//! marking pass knows whether the cached pane interiors may be skipped.

use super::*;

/// Draws every overlay over the pane surfaces, resolves `want_cursor`
/// and returns the mode bar rect and the any-overlay-painted flag.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_overlays_and_cursor(
    app: &mut App,
    buf: &mut Buffer,
    area: Rect,
    layout: &crate::chrome::snapshot::ClientShellLayout,
    inner: Rect,
    _now: Instant,
    cursor: CursorWish,
    pane_overlay_flatten: bool,
) -> (Option<Rect>, bool) {
    let mut overlay_painted = false;
    if pane_overlay_flatten {
        for cell in &mut buf.content {
            cell.modifier = Modifier::from_bits_truncate(cell.modifier.bits() & !0xf000);
        }
    }
    let mode_bar_area = if layout.mobile_header.is_empty()
        && app.shell.tab_bar_position == crate::config::TabBarPositionConfig::Bottom
        && !layout.tab_bar.is_empty()
    {
        layout.tab_bar
    } else {
        layout.pane_surface
    };
    let mobile_navigate = !layout.mobile_header.is_empty() && app.mode == Mode::Navigate;
    // Source suppresses the mode bar while any overlay is open, including
    // the context menu — Navigate can be the mode underneath it.
    let mode_bar = if app.help
        || app.dialog.is_some()
        || app.new_studio.is_some()
        || app.menu.is_some()
        || app.navigator.is_some()
        || app.settings.is_some()
        || app.update_modal.is_some()
        || mobile_navigate
    {
        None
    } else {
        crate::chrome::modebar::render_mode_bar(
            buf,
            mode_bar_area,
            match app.mode {
                Mode::Normal if app.prefix.armed() => {
                    crate::chrome::modebar::ClientShellMode::Prefix
                }
                Mode::Normal => crate::chrome::modebar::ClientShellMode::Terminal,
                Mode::Navigate => crate::chrome::modebar::ClientShellMode::Navigate,
                Mode::Copy => crate::chrome::modebar::ClientShellMode::Copy,
                Mode::Resize => crate::chrome::modebar::ClientShellMode::Resize,
            },
            app.keyboard_locked,
            app.copy_mode.as_ref(),
            None,
            app.update_ready_version().is_some(),
            app.restart_ready.is_some(),
            !app.plain && app.stage_terminal_focused(),
            &app.live,
            &app.shell.palette,
        )
    };
    if mode_bar == Some(layout.tab_bar) {
        app.hit_map.tabs.clear();
        app.hit_map.new_tab = Rect::default();
        app.hit_map.tab_scroll_left = Rect::default();
        app.hit_map.tab_scroll_right = Rect::default();
    }
    // The studio panel: beside the panes when it fits, else over the
    // whole frame — always under the popup/notice/overlay stack so
    // dialogs and action notices still win. A full-screen panel writes
    // over the cached pane surfaces, so the wide-tail pass must rescan.
    // The beside-dock seam (A5.2): while the Call Sheet is expanded
    // the dock draws from `y = bar_h` — under the band, never under
    // the panes' own layout and never a PTY resize.
    // The expanded band floats over everything the frame owns —
    // including the opening card (U8): a studio-less start with the
    // sheet open draws it over the one-time page, so the advertised
    // `alt+y` gets visible feedback and its keys are never swallowed
    // behind an invisible layer. The collapsed strip still waits for
    // the card to leave (U7: with no studio the card is the surface).
    let active_surface = !app.has_active_modal();
    let band_over_page = app.hollywood.expanded();
    let call_sheet_band = app.call_sheet_band(area);
    let bar_h = if band_over_page && app.opening.ready() {
        let machines = app.panel_summaries();
        crate::hollywood::expanded_height(
            call_sheet_band,
            &machines,
            &crate::hollywood::status_of(&app.view),
            &app.hollywood,
            crate::hollywood::LayerOpts {
                agent: app.has_hollywood_cfg,
                mobile: app.area.0 <= app.shell.mobile_width_threshold,
                suppress_highlights: !active_surface,
            },
        )
    } else {
        0
    };
    app.panel_hits = if app.panel_mode_at(area.width) != crate::panel::PanelMode::Closed {
        overlay_painted |= app.panel_fullscreen_at(area.width);
        let rect = if app.panel_beside(area.width) {
            let width = app.panel_width(area.width);
            Rect::new(
                area.x + area.width - width,
                area.y + bar_h,
                width,
                area.height.saturating_sub(bar_h),
            )
        } else {
            // An overlay dock lives inside the pane surface: the
            // sidebar stays visible and, on mobile, the header row.
            app.panel_overlay_rect(layout.pane_surface, area.width)
        };
        let machines = app.panel_summaries();
        // R3-M6: `▸` belongs to a studio workspace's studio only —
        // over `backlot`/`stage`, `bl-hollywood` or a plain workspace
        // the remembered `current` does not paint as active.
        let mark = match &app.studio.focus {
            Some(crate::studio::StudioFocus::Studio(key))
            | Some(crate::studio::StudioFocus::Role(key, _)) => Some(key),
            None if app.studio.lot.studio_workspace => app.studio.current.as_ref(),
            None => None,
        };
        // wide-01: `←→ role` only has somewhere to go while the
        // current studio cycles ≥2 live members (roles + shells).
        let cast_entries = app
            .studio
            .current_index(&machines)
            .map(|index| {
                let studios = crate::panel::data::studios(&machines);
                let studio = studios[index];
                crate::studio::cast_order(studio.studio)
                    .iter()
                    .filter(|role| crate::studio::live_terminal(role).is_some())
                    .count()
                    + app.studio_shells_live(&studio.key()).len()
            })
            .unwrap_or(0);
        // S2: a newer installed backlot than this client runs puts
        // one dim line at the foot of the lot (the row model reads it).
        app.panel.update = app.update_hint();
        crate::panel::render(
            buf,
            rect,
            &machines,
            |machine| crate::panel::stage_running(&app.view, machine),
            |machine| app.machine_reachable(machine),
            &app.panel,
            app.panel_mode_at(area.width),
            app.studio.current.as_ref(),
            mark,
            cast_entries,
            Some(crate::project::LOCAL_LABEL),
            &app.panel_config.keys,
            app.hollywood_key.map(|key| {
                format!(
                    "{} sheet",
                    key.label(&crate::config::format_prefix_combos(&app.live.prefix))
                )
            }),
            !app.panel_beside(area.width),
            app.pane.input_refused(),
            active_surface,
            &app.actions,
            &app.shell.palette,
            crate::update_state::now_ms(),
        )
    } else {
        crate::panel::PanelRender::default()
    };
    // The Hollywood layer floats over `pane + dock` (leaving the left
    // sidebar whole — `R3-4` / `wide-13`), under the modal/notice stack.
    app.hollywood.spin = app.panel.spin;
    app.hollywood_hits = if app.hollywood.visible()
        && app.opening.ready()
        && (band_over_page || !app.opening_visible())
    {
        overlay_painted |= app.hollywood.expanded();
        let machines = app.panel_summaries();
        crate::hollywood::render(
            buf,
            call_sheet_band,
            &machines,
            Some(crate::project::LOCAL_LABEL),
            &crate::hollywood::status_of(&app.view),
            &app.hollywood,
            crate::hollywood::LayerOpts {
                agent: app.has_hollywood_cfg,
                mobile: app.area.0 <= app.shell.mobile_width_threshold,
                suppress_highlights: !active_surface,
            },
            &app.shell.palette,
        )
    } else {
        crate::hollywood::HollywoodRender::default()
    };
    let pill_working = app.studio.lot.pill
        && app.hollywood.visible()
        && app.studio.lot.counts.activity.is_active();
    if app.panel_hits.working_drawn || app.hollywood_hits.working_drawn || pill_working {
        // Arm only when nothing is pending — a redraw before the
        // deadline must not push it out or the spin starves.
        app.panel_anim_deadline
            .get_or_insert(_now + super::super::panel::PANEL_SPIN);
    } else {
        app.panel_anim_deadline = None;
    }
    // The input line's cursor is placed below, once the pane's own wish
    // is known: it must be able to replace it, not be overwritten by it.
    // The mobile switcher IS the Navigate surface under the threshold —
    // `composition.rs` renders it over the whole frame, rebuilding the
    // wire frame as ratatui cells (dropping extended underline bits).
    if layout.mobile_header.height > 0
        && app.mode == Mode::Navigate
        && !app.mobile_switcher_suspended
    {
        overlay_painted = true;
        for cell in &mut buf.content {
            cell.modifier = Modifier::from_bits_truncate(cell.modifier.bits() & !0xf000);
        }
        let federated = app.federated_sidebar();
        crate::chrome::mobile_switcher::render_mobile_switcher(
            buf,
            area,
            &app.projection.snapshot,
            &app.projection.endpoints,
            &app.projection.active_endpoint_id,
            &app.shell,
            app.navigate_target.as_ref(),
            &mut app.shell_state.mobile_switcher_scroll,
            &mut app.shell_state.reveal_mobile_workspace,
            &mut app.hit_map,
            federated,
        );
    }
    app.help_hits = if app.help {
        overlay_painted = true;
        // Source overlays reconstruct the wire frame as ratatui cells first
        // (wire.rs u16_to_modifier drops extended underline bits), then dim
        // the whole background before the panel removes DIM in its own area.
        super::draw_overlays_s3::dim_cells(buf);
        // S5's launcher or herdr's overlay — `draw_overlays_s5` owns
        // the branch and the floating picker.
        super::draw_overlays_s5::render_help(app, buf, area, layout)
    } else {
        crate::chrome::help::HelpRender::default()
    };
    let mut config_covered = Vec::new();
    if let Some(diagnostic) = app.shell.startup_config_diagnostic.as_deref() {
        overlay_painted = true;
        // The reference composes the diagnostic card through the ratatui
        // rebuild, dropping extended underline bits first.
        for cell in &mut buf.content {
            cell.modifier = Modifier::from_bits_truncate(cell.modifier.bits() & !0xf000);
        }
        let diagnostic_area = if layout.mobile_header.is_empty() {
            area
        } else {
            layout.pane_surface
        };
        crate::chrome::config_diagnostic::render(
            buf,
            diagnostic_area,
            diagnostic,
            &app.shell.palette,
            |rect| config_covered.push(rect),
        );
    }
    // `composition.rs::restore_mode_bar`: the bar row is repainted over
    // the toast — save its cells, draw notices, then put them back.
    let mode_bar_cells: Vec<ratatui::buffer::Cell> = mode_bar
        .map(|bar| {
            (bar.x..bar.x + bar.width)
                .map(|x| buf[(x, bar.y)].clone())
                .collect()
        })
        .unwrap_or_default();
    // The `copied to clipboard` card sits under the popup and the
    // notices — the reference composes it after its toast and before
    // the popup. The notification hit from the last frame is the toast
    // it must not overlap.
    if let Some(feedback) = app.copy_feedback.as_ref() {
        overlay_painted = true;
        // Same ratatui rebuild as the other overlays: extended underline
        // bits drop before the card paints.
        for cell in &mut buf.content {
            cell.modifier = Modifier::from_bits_truncate(cell.modifier.bits() & !0xf000);
        }
        let feedback_area = if layout.mobile_header.is_empty() {
            layout.pane_surface
        } else {
            area
        };
        let base = u16::from(!config_covered.is_empty());
        let offset = crate::chrome::copy_feedback::copy_feedback_offset_for_toast(
            feedback_area,
            &feedback.message,
            base,
            app.shell.clipboard_toast_position,
            app.notification_hit,
        );
        crate::chrome::copy_feedback::render_copy_feedback_buffer(
            buf,
            feedback_area,
            &feedback.message,
            offset,
            app.shell.clipboard_toast_position,
            &app.shell.palette,
        );
    }
    overlay_painted |= app.popup.is_some() || app.peek.is_some();
    app.draw_popups(buf, _now);
    overlay_painted |= crate::app::notices::render(app, buf, area, layout);
    // S3's action toast (§3) — the pending reply's card under the
    // notice slot; its `copy cmd` chip feeds the next mouse press.
    overlay_painted |= super::draw_overlays_s3::draw_action_toast(app, buf, area, layout);
    // The restore would repaint the bar over a full-screen panel —
    // skipped then; beside the panes the bar is never covered anyway.
    if !app.panel_fullscreen_at(area.width)
        && let Some(bar) = mode_bar
    {
        for (offset, cell) in mode_bar_cells.into_iter().enumerate() {
            *buf.cell_mut((bar.x + offset as u16, bar.y))
                .expect("bar cell") = cell;
        }
    }
    // A popup owns the host cursor while open; `draw_popups` already
    // stored its surface-relative wish in `app.want_cursor`.
    if app.popup.is_none() {
        app.want_cursor = CursorWish {
            pos: cursor.pos.map(|(x, y)| (inner.x + x, inner.y + y)),
            anchor: cursor.anchor.map(|(x, y)| (inner.x + x, inner.y + y)),
            ..cursor
        };
    }
    // The dock's `›` line owns the host cursor while it composes —
    // unless the Call Sheet floats over it (R3-B2); the sheet's own
    // cursor block below outranks this one then.
    if app.popup.is_none()
        && !app.hollywood.expanded()
        && app.hollywood.view.is_none()
        && let Some(cursor) = app.panel_hits.cursor.as_ref()
    {
        app.want_cursor = CursorWish {
            pos: cursor.visible.then_some((cursor.x, cursor.y)),
            anchor: Some((cursor.x, cursor.y)),
            shape: CursorShape::Default,
            blinking: true,
        };
    }
    if mode_bar.is_some_and(|bar| app.want_cursor.pos.is_some_and(|(_, y)| y == bar.y)) {
        app.want_cursor.pos = None;
        app.want_cursor.anchor = None;
        app.want_cursor.shape = CursorShape::Default;
    }
    // The Hollywood bar is opaque, and that includes the host cursor: it
    // floats over the panes, so a pane cursor that lands inside the band
    // would sit in a frame or on the input line (U7 fix 1). While the
    // bar is expanded, such a cursor is hidden, and the `tell hollywood`
    // input's own cursor takes its place while the line edits. A strip
    // or a hidden bar covers no cells and changes nothing; a popup owns
    // the cursor while one is open.
    if app.hollywood.expanded() && app.popup.is_none() && !app.has_active_modal() {
        if app
            .want_cursor
            .pos
            .is_some_and(|point| app.hollywood_hits.contains(point))
        {
            app.want_cursor.pos = None;
            app.want_cursor.anchor = None;
            app.want_cursor.shape = CursorShape::Default;
        }
        if let Some(cursor) = app.hollywood_hits.cursor.as_ref() {
            app.want_cursor = CursorWish {
                pos: cursor.visible.then_some((cursor.x, cursor.y)),
                anchor: Some((cursor.x, cursor.y)),
                shape: CursorShape::Default,
                blinking: true,
            };
        }
    }
    if app.want_cursor.pos.is_some_and(|(x, y)| {
        config_covered
            .iter()
            .any(|rect| rect.contains(ratatui::layout::Position::new(x, y)))
    }) {
        app.want_cursor.pos = None;
    }
    if let Some(cursor) = app.help_hits.cursor.as_ref() {
        app.want_cursor = CursorWish {
            pos: cursor.visible.then_some((cursor.x, cursor.y)),
            anchor: Some((cursor.x, cursor.y)),
            shape: CursorShape::Default,
            blinking: true,
        };
    }
    // `render_client_overlay` draws the overlay over the notices and
    // the restored mode bar; the Navigator variant is the only one the
    // reference does not dim the background for.
    app.navigator_hits = if let Some(navigator) = app.navigator.as_ref() {
        overlay_painted = true;
        // The source composes overlays by rebuilding the wire frame as
        // ratatui cells (wire.rs u16_to_modifier drops extended underline
        // bits); it does not dim the background for the navigator.
        for cell in &mut buf.content {
            cell.modifier = Modifier::from_bits_truncate(cell.modifier.bits() & !0xf000);
        }
        crate::chrome::navigator_render::render_navigator_overlay(
            buf,
            navigator,
            &app.projection.endpoints,
            &app.projection.active_endpoint_id,
            app.federated_sidebar(),
            &app.shell.palette,
        )
        .unwrap_or_default()
    } else {
        crate::chrome::navigator_render::NavigatorRender::default()
    };
    if let Some(cursor) = app.navigator_hits.cursor.as_ref() {
        app.want_cursor = CursorWish {
            pos: cursor.visible.then_some((cursor.x, cursor.y)),
            anchor: Some((cursor.x, cursor.y)),
            shape: CursorShape::Default,
            blinking: true,
        };
    }
    // The Settings overlay dims its background like help/dialogs.
    app.settings_hits = if let Some(settings) = app.settings.as_ref() {
        overlay_painted = true;
        super::draw_overlays_s3::dim_cells(buf);
        crate::chrome::settings_render::render_settings_overlay(buf, settings, &app.shell.palette)
            .unwrap_or_default()
    } else {
        crate::chrome::settings_render::SettingsRender::default()
    };
    // The update card dims the background like the other overlays.
    if let Some(modal) = app.update_modal.as_ref() {
        overlay_painted = true;
        super::draw_overlays_s3::dim_cells(buf);
        let view = crate::app::update_modal::view(
            modal,
            &app.update_state,
            app.update_latest.as_deref(),
            app.update_notes_fetching,
            app.install.as_ref(),
        );
        let rendered = crate::chrome::update_modal::render(buf, &view, &app.shell.palette);
        if let (Some(modal), Some(rendered)) = (app.update_modal.as_mut(), rendered) {
            modal.render = rendered;
        }
    }
    overlay_painted |= app.dialog.is_some();
    app.render_dialog(buf);
    // The New Studio dialog dims like the help overlay and paints
    // the same panel frame; its hit map feeds the next mouse press
    // and its cursor wish owns the host cursor like the fields do.
    app.ns_hits = if let Some(d) = app.new_studio.as_ref() {
        overlay_painted = true;
        super::draw_overlays_s3::dim_cells(buf);
        crate::new_studio::render(
            buf,
            d,
            &app.shell.palette,
            app.backlot_modal_surface(area, layout, 64),
        )
    } else {
        crate::new_studio::NsHits::default()
    };
    if let Some(cursor) = app.ns_hits.cursor.as_ref() {
        app.want_cursor = CursorWish {
            pos: cursor.visible.then_some((cursor.x, cursor.y)),
            anchor: Some((cursor.x, cursor.y)),
            shape: CursorShape::Default,
            blinking: true,
        };
    }
    // The Hollywood setup dialog dims like the other modals and
    // centres in `backlot_modal_surface`; its hit map and cursor feed
    // the next press and the host cursor like the fields do.
    app.hw_hits = if let Some(d) = app.hw_setup.as_ref() {
        overlay_painted = true;
        super::draw_overlays_s3::dim_cells(buf);
        crate::hw_setup::render(
            buf,
            d,
            &app.shell.palette,
            app.backlot_modal_surface(area, layout, 64),
        )
    } else {
        crate::hw_setup::HwHits::default()
    };
    if let Some(cursor) = app.hw_hits.cursor.as_ref() {
        app.want_cursor = CursorWish {
            pos: cursor.visible.then_some((cursor.x, cursor.y)),
            anchor: Some((cursor.x, cursor.y)),
            shape: CursorShape::Default,
            blinking: true,
        };
    }
    // The Task & Delivery Sheet dims like the other modals and
    // centres in `backlot_modal_surface`; its keys are the sheet's
    // own while open.
    app.sheet_hits = if let Some(sheet) = app.sheet.as_ref() {
        overlay_painted = true;
        super::draw_overlays_s3::dim_cells(buf);
        let machines = app.panel_summaries();
        let summary = machines
            .iter()
            .flat_map(|m| m.summary.studios.iter())
            .find(|s| s.id == sheet.studio.id);
        crate::sheet::render(
            buf,
            app.backlot_modal_surface(area, layout, 64),
            sheet,
            summary,
            &app.shell.palette,
        )
    } else {
        crate::sheet::SheetRender::default()
    };
    // The unified top-level Backlot Reader (`CardView` + Task Sheet
    // report reads) dims everything underneath and centers via
    // `backlot_modal_surface` (`R3-M2`).
    overlay_painted |= super::draw_overlays_s3::draw_reader(app, buf, area, layout);
    // The S3 native dialogs (§2): dimmed background, centred inside
    // `backlot_modal_surface`; their field cursor owns the host cursor.
    overlay_painted |= super::draw_overlays_s3::draw_studio_dialog(app, buf, area, layout);
    // The context menu is the topmost shell overlay; the reference
    // suppresses the host cursor while it is open.
    if let Some(menu) = &app.menu {
        overlay_painted = true;
        // The source composes overlays by rebuilding the wire frame as
        // ratatui cells (wire.rs u16_to_modifier drops extended underline
        // bits); it does not dim the background for the context menu.
        for cell in &mut buf.content {
            cell.modifier = Modifier::from_bits_truncate(cell.modifier.bits() & !0xf000);
        }
        let max_right = match &menu.target {
            crate::chrome::menu::ContextMenuTarget::Studio(sm)
                if sm.clamp_outside_dock && !app.plain =>
            {
                area.x + area.width.saturating_sub(app.dock_reserve(area.width))
            }
            _ => area.x + area.width,
        };
        if let Some(rendered) =
            crate::chrome::menu::render_context_menu_in(buf, menu, max_right, &app.shell.palette)
        {
            app.hit_map.context_menu_rows = rendered.rows;
        }
        app.want_cursor.pos = None;
        app.want_cursor.anchor = None;
    } else {
        app.hit_map.context_menu_rows.clear();
    }
    // Derived from herdr protocol/render_ansi.rs::frame_with_drawn_cursor.
    if app.shell.draw_host_cursor
        && let Some(position) = app.want_cursor.pos.take()
    {
        app.last_visible_cursor = Some(position);
        buf[position].modifier ^= Modifier::REVERSED;
        app.want_cursor.anchor = Some(position);
    }
    (mode_bar, overlay_painted)
}
