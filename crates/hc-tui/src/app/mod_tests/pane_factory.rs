//! One pane factory: every pane the app owns — the opening one, the
//! focused pane, an observer, a popup, a peek, a float and its hidden
//! lease — takes the `[advanced]` attach history and VT budget from the
//! loaded config. A pane born from `Pane::new` directly keeps the 5000-line
//! and 10 MB defaults instead of the user's values.

use super::*;
use crate::app::float::{FloatEntry, FloatKey};
use crate::app::popup::PopupPending;

const HISTORY: u64 = 1234;
const BYTES: usize = 3_000_000;

/// An app whose `[advanced]` values differ from every default.
fn app_with_advanced() -> App {
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
    let mut config = crate::config::Config::default();
    config.advanced.attach_history_lines = HISTORY;
    config.advanced.scrollback_limit_bytes = BYTES;
    let live = config.live_keybinds();
    let shell = crate::chrome::ClientShellConfig::from_config(
        &config,
        None,
        None,
        std::path::PathBuf::new(),
    );
    let mut app = App::new(watcher, open, live, shell, Arc::new(Probe::default()));
    app.set_plain(true);
    app
}

/// One live terminal, so peek and the float owners have an inventory.
fn with_one_terminal(app: &mut App) {
    let mut term = crate::sidebar::fixture::term("t1");
    term.pid = Some(1);
    term.created_ms = 1;
    app.view = crate::sidebar::fixture::view(vec![term], vec![]);
    app.refresh_projection();
}

#[tokio::test]
async fn every_pane_the_app_owns_takes_the_configured_budgets() {
    let mut app = app_with_advanced();
    assert_eq!(
        app.pane.budgets(),
        (HISTORY, BYTES),
        "the pane built with the app"
    );

    with_one_terminal(&mut app);
    assert_eq!(
        app.new_pane().budgets(),
        (HISTORY, BYTES),
        "the factory itself"
    );

    app.open_peek("t1".into());
    let peek = app.peek.as_ref().expect("peek opened");
    assert_eq!(peek.pane.budgets(), (HISTORY, BYTES), "peek");

    let key = FloatKey {
        owner: "space-t1".into(),
        command: "alt+f".into(),
    };
    app.hide_float(
        key.clone(),
        FloatEntry {
            terminal: "t9".into(),
            width: None,
            height: None,
            restore: true,
            created_ms: 9,
            seen: false,
            kill_sent: false,
        },
    );
    let lease = app.size_leases.get("t9").expect("hidden float lease");
    assert_eq!(lease.budgets(), (HISTORY, BYTES), "hidden float's lease");

    app.show_float(key, "t8".into(), None, None, 8);
    let float = app.popup.as_ref().expect("float shown");
    assert_eq!(float.pane.budgets(), (HISTORY, BYTES), "shown float");

    app.popup = None;
    app.popup_pending = Some(PopupPending {
        width: None,
        height: None,
        persistent: false,
        owner: "space-t1".into(),
        command: "alt+f".into(),
        install_machine: None,
    });
    app.on_popup_spawned("t7".into(), 7);
    let popup = app.popup.as_ref().expect("popup attached");
    assert_eq!(popup.pane.budgets(), (HISTORY, BYTES), "popup");
}
