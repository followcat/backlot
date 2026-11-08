//! Read-only application tests and shared fixtures.
mod cases_1;
mod cases_2;
mod endpoint_nav;
mod layout;
mod layout_regressions;
mod m3a;
mod m3a_review;
mod m4s1;
mod m4s2;
mod m4s3;
mod m4s4;
mod m4s5;
mod narrow;
mod reload;
mod review_regressions;

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

fn app() -> App {
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
    let open: OpenAttach = Arc::new(|_| unreachable!("no attach in this test"));
    let config = crate::config::Config::default();
    let live = config.live_keybinds();
    let shell = crate::chrome::ClientShellConfig::from_config(
        &config,
        None,
        None,
        std::path::PathBuf::new(),
    );
    App::new(
        watcher,
        open,
        live,
        shell,
        Arc::new(Probe::default()),
        Box::new(|_, _| {}),
    )
}

fn app_with_spaces(spaced: &[(&str, &str)]) -> App {
    let mut app = app();
    let terms: Vec<_> = spaced
        .iter()
        .enumerate()
        .map(|(i, (a, space))| {
            let mut t = crate::sidebar::fixture::term(a);
            t.pid = Some(1);
            t.created_ms = i as u64;
            t.labels.insert("herdr.space".into(), (*space).into());
            t.labels.insert("herdr.tab".into(), "tab".into());
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
    view.inventory.replace(1, terms);
    app.view = view;
    app.refresh_projection();
    app
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
    view.inventory.replace(1, terms);
    app.view = view;
    app.refresh_projection();
    app
}

fn term_key(key: hc_vt::Key, mods: hc_vt::Mods, action: hc_vt::Action) -> input::TerminalKey {
    input::TerminalKey::new(hc_vt::KeyInput { key, mods, action })
}
