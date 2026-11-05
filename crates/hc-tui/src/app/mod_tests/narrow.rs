use super::*;
use crate::app::pane::Pane;
use crate::chrome::render::ClientMobileTarget;

#[tokio::test]
async fn closing_mobile_help_uses_published_focus_after_inventory_removal() {
    for removed in [false, true] {
        let mut app = app_with(&["t1"]);
        let (tx, rx) = mpsc::channel(64);
        app.pane = Pane::new(tx, Arc::new(|_| Box::pin(std::future::pending())));
        app.pane_rx = rx;
        app.focused = Some("t1".into());
        app.refresh_projection();
        assert!(app.projection.snapshot.focused_workspace_id.is_some());
        app.mode = Mode::Navigate;
        app.help = true;
        app.mobile_switcher_suspended = true;
        if removed {
            app.view.inventory.replace(2, Vec::new());
            app.reconcile_published_view();
            assert_eq!(app.focused.as_deref(), Some("t1")); // Cache survives Navigate.
            assert!(app.projection.snapshot.focused_workspace_id.is_none());
        }
        app.help = false;
        app.resume_mobile_switcher_if_ready();
        app.sync_keyboard_report_all();
        assert_eq!(
            app.mode,
            if removed {
                Mode::Navigate
            } else {
                Mode::Normal
            }
        );
        assert_eq!(app.host_reports_all_keys, removed);
    }
}

#[tokio::test]
async fn mobile_menu_starts_at_the_source_boundary_and_routes_read_only_actions() {
    for width in [65, 64, 40] {
        let mut app = app();
        app.mode = Mode::Navigate;
        app.sched.mark_dirty(Instant::now());
        let mut term = Terminal::new(TestBackend::new(width, 24)).unwrap();
        app.maybe_draw(&mut term, Instant::now()).unwrap();
        let menu = app
            .hit_map
            .mobile_targets
            .iter()
            .filter_map(|(_, target)| {
                if let ClientMobileTarget::Menu(index) = target {
                    Some(*index)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        if width == 65 {
            assert!(menu.is_empty());
            continue;
        }
        assert_eq!(menu, [0, 1, 2, 3]);
        let text = term
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        for label in ["menu", "settings", "keybinds", "reload config", "detach"] {
            assert!(text.contains(label), "missing {label} at {width}");
        }
        app.on_mobile_target(&ClientMobileTarget::Menu(1));
        assert!(app.help);
        assert_eq!(app.mode, Mode::Navigate);
        assert!(app.mobile_switcher_suspended);
        app.help = false;
        app.resume_mobile_switcher_if_ready();
        assert!(!app.mobile_switcher_suspended);
        assert_eq!(app.mode, Mode::Navigate); // No focused workspace in this fixture.
        app.on_mobile_target(&ClientMobileTarget::Menu(3));
        assert!(app.quitting);
    }
}
