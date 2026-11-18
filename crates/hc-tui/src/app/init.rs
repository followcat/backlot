use super::*;

impl App {
    /// An app over `watcher` (already watching) and `open` for attaches.
    pub fn new(
        watcher: Watcher,
        open: OpenAttach,
        live: crate::config::LiveKeybindConfig,
        shell: ClientShellConfig,
        probe: Arc<Probe>,
    ) -> App {
        let (pane_tx, pane_rx) = mpsc::channel(64);
        let (seen_tx, seen_req_rx) = mpsc::channel(64);
        let (seen_res_tx, seen_rx) = mpsc::channel(64);
        let (panel_action_tx, panel_rx) = mpsc::channel(16);
        let (hwood_tx, hwood_rx) = mpsc::channel(16);
        let (waiting_tx, waiting_req_rx) = mpsc::channel(8);
        let (waiting_res_tx, waiting_rx) = mpsc::channel(64);
        let (image_read_done_tx, image_read_done_rx) = mpsc::channel(8);
        let (update_tx, update_rx) = mpsc::channel(16);
        let view = View::default();
        let projection = crate::project::project(&view, None);
        // Saved per-endpoint chrome choices win over config; each
        // `Some` also marks its manual flag so a reload keeps it —
        // the reference's `ClientShellState::new` preference block.
        let preferences = shell.preferences.clone();
        let sidebar_collapsed = preferences
            .sidebar_collapsed
            .unwrap_or(shell.sidebar_start_collapsed);
        let (min_width, max_width) = crate::config::validated_sidebar_bounds(
            shell.sidebar_min_width,
            shell.sidebar_max_width,
        )
        .unwrap_or((18, 36));
        let sidebar_width = preferences
            .sidebar_width
            .unwrap_or(shell.sidebar_width)
            .clamp(min_width, max_width);
        let sidebar_section_split = preferences
            .sidebar_section_split
            .filter(|split| split.is_finite())
            .map(|split| split.clamp(0.1, 0.9))
            .unwrap_or(0.5);
        let mut shell = shell;
        if let Some(sort) = preferences.agent_panel_sort {
            shell.agent_panel_sort = sort;
        }
        let mut remote_collapsed_groups = std::collections::HashMap::new();
        for saved in preferences.remote_collapsed_groups {
            if saved.profile_id.is_empty() {
                continue;
            }
            remote_collapsed_groups
                .entry(ClientEndpointId::Remote(saved.profile_id))
                .or_insert_with(std::collections::HashSet::new)
                .extend(saved.collapsed_groups);
        }
        let collapsed_groups = preferences
            .collapsed_groups
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        let sidebar_collapsed_manual = preferences.sidebar_collapsed.is_some();
        let sidebar_width_manual = preferences.sidebar_width.is_some();
        let sidebar_section_split_manual = preferences.sidebar_section_split.is_some();
        let agent_panel_sort_manual = preferences.agent_panel_sort.is_some();
        let mouse_capture = shell.mouse_capture;
        App {
            notice: None,
            core_start: None,
            update_check: None,
            update_latest: None,
            update_state: crate::update_state::UpdateCheckState::default(),
            update_state_path: None,
            update_toast_hit: ratatui::layout::Rect::default(),
            update_notes_fetching: false,
            notes_fetch: update_flow::default_notes_fetch(),
            update_tx,
            update_rx,
            update_modal: None,
            install: None,
            install_generation: 0,
            seams: install_flow::default_seams(),
            restart_ready: None,
            restart: None,
            running_exe: None,
            install_source: crate::install_source::InstallSource::Direct,
            background_failure: None,
            next_tab_status: None,
            last_window_title: None,
            take_control_hit: ratatui::layout::Rect::default(),
            watcher,
            pane: pane::Pane::new(pane_tx.clone(), open.clone()),
            pane_rx,
            pane_tx,
            open_attach: open,
            observers: Default::default(),
            size_leases: Default::default(),
            observer_mru: Default::default(),
            layout_seen: None,
            operations: None,
            pending_actions: 0,
            layout_confirmed: None,
            prefix: input::Prefix::default(),
            mode: Mode::Normal,
            keyboard_locked: false,
            previous_pane: None,
            last_focus: None,
            dialog: None,
            dialog_hits: Default::default(),
            menu: None,
            right_click_panes: Default::default(),
            help: false,
            mobile_switcher_suspended: false,
            help_state: crate::chrome::help::HelpState::default(),
            help_hits: crate::chrome::help::HelpRender::default(),
            help_scroll_grab: None,
            navigator: None,
            navigator_hits: crate::chrome::navigator_render::NavigatorRender::default(),
            settings: None,
            settings_hits: crate::chrome::settings_render::SettingsRender::default(),
            popup: None,
            popup_pending: None,
            command_overlay: None,
            peek: None,
            panel: crate::panel::Panel::new(),
            panel_config: crate::panel::PanelConfig::default(),
            panel_config_path: std::path::PathBuf::new(),
            panel_key_combo: crate::config::parse_key_combo("y"),
            panel_hits: crate::panel::PanelRender::default(),
            panel_anim_deadline: None,
            // Never spawn here: tests build `App` through this path and
            // must stay process-free. cli installs `spawn_runner` on the
            // real binary before the loop starts.
            panel_runner: std::sync::Arc::new(|_| {
                Box::pin(async { Err("panel actions unavailable".into()) })
            }),
            remote_runner: None,
            panel_action_tx,
            panel_rx,
            panel_endpoint_args: Vec::new(),
            plain: false,
            studio: crate::studio::Studio::default(),
            opening: crate::opening::Opening::default(),
            opening_anim_deadline: None,
            hollywood: crate::hollywood::Hollywood::new(),
            hollywood_key: None,
            has_hollywood_cfg: false,
            hollywood_overseer: None,
            setup_runner_hint: None,
            hollywood_hits: crate::hollywood::HollywoodRender::default(),
            // Never spawn here — cli installs `spawn_runner` like the
            // panel's before the loop starts.
            hollywood_runner: std::sync::Arc::new(|_| {
                Box::pin(async { Err("hollywood actions unavailable".into()) })
            }),
            hwood_tx,
            hwood_rx,
            sched: sched::Scheduler::default(),
            focused: None,
            before_navigate: None,
            live,
            shell,
            projection,
            navigate_target: None,
            shell_state: ShellState {
                collapsed_groups,
                remote_collapsed_groups,
                sidebar_collapsed,
                sidebar_section_split,
                ..ShellState::default()
            },
            sidebar_width,
            sidebar_width_manual,
            sidebar_collapsed_manual,
            sidebar_section_split_manual,
            agent_panel_sort_manual,
            chrome_drag: None,
            pane_mouse_gesture: None,
            selection: None,
            copy_mode: None,
            popup_selection: None,
            popup_gesture: None,
            word_selection: None,
            selection_autoscroll: None,
            selection_autoscroll_deadline: None,
            selection_highlight_clear_deadline: None,
            copy_feedback: None,
            copy_feedback_deadline: None,
            last_pane_click: None,
            workspace_press: None,
            tab_press: None,
            tab_drag_index: None,
            tab_dragged: false,
            interaction_core: None,
            last_divider_click: None,
            image_paste_remote: false,
            image_paste_over_ssh: false,
            image_paste_suppress: Default::default(),
            popup_key_suppress: Default::default(),
            floats: Default::default(),
            image_paste_reader: Arc::new(crate::clipboard::read_clipboard_image),
            image_drop_reader: Arc::new(crate::clipdrop::read_image_file),
            image_read: None,
            image_read_seq: 0,
            image_read_task: None,
            image_read_done_tx,
            image_read_done_rx,
            image_read_hold: std::time::Duration::from_secs(3),
            image_read_replay: false,
            hit_map: ShellHitMap::default(),
            host_reports_all_keys: false,
            host_reports_key_releases: false,
            host_erase_is_ctrl_h: false,
            probe_keyboard_flags: None,
            probe_device_attributes: false,
            host_sgr_pixel_mouse: None,
            host_pixel_mouse_active: false,
            host_mouse_capture_active: mouse_capture,
            host_mouse_pixels: None,
            host_appearance: None,
            host_appearance_explicit: false,
            host_control: Box::new(|_| Ok(())),
            pane_leases: KeyLeases::default(),
            next_dispatch_id: 1,
            view,
            project_cache: Default::default(),
            seen: 0,
            area: (80, 24),
            want_cursor: draw::CursorWish {
                pos: None,
                anchor: None,
                shape: hc_vt::CursorShape::Default,
                blinking: true,
            },
            last_visible_cursor: None,
            quitting: false,
            host_facts: crate::host::HostFacts::default(),
            presentation: presentation::Presentation::default(),
            notifications: notifications::Notifications::default(),
            notifications_seeded: false,
            notification_hit: ratatui::layout::Rect::default(),
            notify_backend: crate::terminal_notify::detect_backend(),
            in_tmux: crate::terminal_notify::in_tmux(),
            seen_looked: Default::default(),
            seen_inflight: Default::default(),
            seen_tx,
            seen_res_tx,
            seen_rx,
            seen_req_rx: Some(seen_req_rx),
            seen_task: None,
            waiting: Default::default(),
            waiting_tx,
            waiting_res_tx,
            waiting_rx,
            waiting_req_rx: Some(waiting_req_rx),
            waiting_task: None,
            waiting_due: None,
            reap_tx: None,
            reap_task: None,
            reaped_exited: Default::default(),
            outer_focused: None,
            probe,
        }
    }
}
