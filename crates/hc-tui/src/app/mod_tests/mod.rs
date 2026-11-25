//! Read-only application tests and shared fixtures.
mod alt_arrows;
mod alt_arrows_b;
mod argv_hygiene;
mod cases_1;
mod cases_2;
mod chrome_marks;
mod closing;
mod delete_studio;
mod delete_studio_preconfirm;
mod empty_core_workspace;
mod endpoint_nav;
mod endpoint_nav_b;
mod float_draw;
mod float_takeover;
mod floats;
mod floats_lifecycle;
mod help;
mod help_launcher;
mod hollywood;
mod image_paste;
mod image_paste_off_loop;
mod image_paste_upload;
mod install_card;
mod layout;
mod layout_regressions;
mod lifecycle_archive;
mod lifecycle_fixtures;
mod lifecycle_launcher;
mod lifecycle_role_menu;
mod lifecycle_studio_menu;
mod live_folder;
mod m3a;
mod m3a_review;
mod m4s1;
mod m4s2;
mod m4s2_b;
mod m4s3;
mod m4s4;
mod m4s5;
mod machine_versions;
mod mouse_selection;
mod narrow;
mod new_studio;
mod observer_policy_tests;
mod opening;
mod outward_cards;
mod overlay_reset;
mod own_theme;
mod page_keys;
mod pane_cache;
mod pane_exit_focus;
mod pane_factory;
mod panel;
mod preferences;
mod r3_followups;
mod r3_s4;
mod r3_s4_b;
mod reload;
mod remote_children;
mod reveal_focus;
mod review_regressions;
mod server_client;
mod server_doc_fake;
mod server_studio_ops;
mod server_studio_stale;
mod settings_studio;
mod shared_rpc;
mod sidebar_edge;
mod single_machine_sidebar;
mod stage_toast;
mod standby;
mod studio;
mod studio_cache;
mod studio_machines;
mod studio_mouse;
mod studio_notify;
mod sync_output;
mod update_card;
mod update_source;
mod version_badge;
mod view_sync;
mod w1_left_side;
mod w1b_left_keys;
mod waiting;

use super::*;
use ratatui::backend::TestBackend;

struct FailBackend;
impl Backend for FailBackend {
    type Error = std::io::Error;
    fn draw<'a, I>(&mut self, _c: I) -> std::io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a ratatui::buffer::Cell)>,
    {
        Err(std::io::Error::other("draw exploded"))
    }
    fn hide_cursor(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    fn show_cursor(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    fn get_cursor_position(&mut self) -> std::io::Result<ratatui::layout::Position> {
        Ok((0, 0).into())
    }
    fn set_cursor_position<P: Into<ratatui::layout::Position>>(
        &mut self,
        _p: P,
    ) -> std::io::Result<()> {
        Ok(())
    }
    fn clear(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    fn clear_region(&mut self, _: ratatui::backend::ClearType) -> std::io::Result<()> {
        Ok(())
    }
    fn size(&self) -> std::io::Result<ratatui::layout::Size> {
        Ok((80, 24).into())
    }
    fn window_size(&mut self) -> std::io::Result<ratatui::backend::WindowSize> {
        Ok(ratatui::backend::WindowSize {
            columns_rows: (80, 24).into(),
            pixels: (800, 480).into(),
        })
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl crate::ansi::FrameCursor for FailBackend {}

/// A backend whose frames never reach the terminal: `emitted` is always
/// false, the way `AnsiBackend` reports an identical-frame skip.
struct QuietBackend;
impl Backend for QuietBackend {
    type Error = std::io::Error;
    fn draw<'a, I>(&mut self, _c: I) -> std::io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a ratatui::buffer::Cell)>,
    {
        Ok(())
    }
    fn hide_cursor(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    fn show_cursor(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    fn get_cursor_position(&mut self) -> std::io::Result<ratatui::layout::Position> {
        Ok((0, 0).into())
    }
    fn set_cursor_position<P: Into<ratatui::layout::Position>>(
        &mut self,
        _p: P,
    ) -> std::io::Result<()> {
        Ok(())
    }
    fn clear(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    fn clear_region(&mut self, _: ratatui::backend::ClearType) -> std::io::Result<()> {
        Ok(())
    }
    fn size(&self) -> std::io::Result<ratatui::layout::Size> {
        Ok((80, 24).into())
    }
    fn window_size(&mut self) -> std::io::Result<ratatui::backend::WindowSize> {
        Ok(ratatui::backend::WindowSize {
            columns_rows: (80, 24).into(),
            pixels: (800, 480).into(),
        })
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl crate::ansi::FrameCursor for QuietBackend {
    fn emitted(&self) -> bool {
        false
    }
}

fn app() -> App {
    app_with_plain(true)
}

/// `app()`'s shape with the `--plain` flag chosen: `true` is the
/// fixtures' baseline, `false` the launch default a real run gets.
fn app_with_plain(plain: bool) -> App {
    let (accepted, _rx) = mpsc::channel(1);
    let (rpc, sub) = crate::testcore::openers(accepted);
    let watcher = Watcher::with_openers(
        rpc,
        sub,
        hc_proto::watch::WatchParams {
            traversal: Some(crate::project::agent_traversal),
            ..hc_proto::watch::WatchParams::default()
        },
    );
    app_over(watcher, plain)
}

/// An app over `watcher` — the shape `app()` builds, for tests that
/// drive a real actor against a fake core.
fn app_over(watcher: Watcher, plain: bool) -> App {
    let open: OpenAttach = Arc::new(|_| unreachable!("no attach in this test"));
    let config = crate::config::Config::default();
    let live = config.live_keybinds();
    let shell = crate::chrome::ClientShellConfig::from_config(
        &config,
        None,
        None,
        std::path::PathBuf::new(),
    );
    let mut app = App::new(watcher, open, live, shell, Arc::new(Probe::default()));
    // U2: unadorned `backlot` is studio-first — the mechanics most
    // tests exercise are the plain multiplexer's, so the fixture
    // runs `--plain`; studio fixtures clear the flag themselves.
    if plain {
        app.set_plain(true);
    }
    app
}

fn app_with_spaces(spaced: &[(&str, &str)]) -> App {
    let tabbed: Vec<(&str, &str, &str)> = spaced
        .iter()
        .map(|(a, space)| (*a, *space, "tab"))
        .collect();
    app_with_tabs(&tabbed)
}

fn app_with_tabs(tabbed: &[(&str, &str, &str)]) -> App {
    let mut app = app();
    let terms: Vec<_> = tabbed
        .iter()
        .enumerate()
        .map(|(i, (a, space, tab))| {
            let mut t = crate::sidebar::fixture::term(a);
            t.pid = Some(1);
            t.created_ms = i as u64;
            t.labels.insert("herdr.space".into(), (*space).into());
            t.labels.insert("herdr.tab".into(), (*tab).into());
            t
        })
        .collect();
    let mut view = View {
        link: hc_proto::watch::Link::Online,
        rpc: hc_proto::watch::Link::Online,
        core: Some(hc_proto::hello::CoreInfo {
            id: "id-host-a".into(),
            name: "host-a".into(),
            hostname: None,
        }),
        machines: Some(vec![]),
        ..Default::default()
    };
    view.inventory_mut().replace(1, terms);
    app.view = view;
    app.refresh_projection();
    app
}

/// A focused live agent on an Online view: `address` in space-a/tab-a,
/// word `idle`, `turn` — the core id the fake wires answer as.
fn publish_agent(app: &mut App, address: &str, turn: u64) {
    let mut term = crate::sidebar::fixture::agent(address, "idle");
    term.labels.insert("herdr.space".into(), "space-a".into());
    term.labels.insert("herdr.tab".into(), "tab-a".into());
    term.turn = turn;
    app.view = crate::sidebar::fixture::view(vec![term], vec![]);
    app.view.core.as_mut().unwrap().id = "core-fake-1".into();
    app.focused = Some(address.into());
    app.reconcile_published_view();
}

fn app_with(addresses: &[&str]) -> App {
    let spaced: Vec<(&str, String)> = addresses
        .iter()
        .map(|a| (*a, format!("space-{a}")))
        .collect();
    let pairs: Vec<(&str, &str)> = spaced.iter().map(|(a, s)| (*a, s.as_str())).collect();
    app_with_spaces(&pairs)
}

fn draw_to(app: &mut App, w: u16, h: u16) -> ratatui::backend::TestBackend {
    let backend = TestBackend::new(w, h);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| draw::draw(app, f, Instant::now())).unwrap();
    term.backend().clone()
}

fn screen_lines(b: &TestBackend) -> Vec<String> {
    let (w, h) = (b.size().unwrap().width, b.size().unwrap().height);
    let buf = b.buffer();
    (0..h)
        .map(|y| {
            (0..w)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

// ---- Migrated draw/selection semantics (from the retired
// tests/snapshots.rs + selection tests) — asserted through the real
// `draw` path and projection, not the removed row model. ----

fn app_with_terms(terms: Vec<hc_proto::terminal::Terminal>) -> App {
    let mut app = app();
    let mut view = hc_proto::watch::View {
        link: hc_proto::watch::Link::Online,
        rpc: hc_proto::watch::Link::Online,
        core: Some(hc_proto::hello::CoreInfo {
            id: "id-host-a".into(),
            name: "host-a".into(),
            hostname: None,
        }),
        machines: Some(vec![]),
        ..Default::default()
    };
    view.inventory_mut().replace(1, terms);
    app.view = view;
    app.refresh_projection();
    app
}

fn term_key(key: hc_vt::Key, mods: hc_vt::Mods, action: hc_vt::Action) -> input::TerminalKey {
    input::TerminalKey::new(hc_vt::KeyInput { key, mods, action })
}
