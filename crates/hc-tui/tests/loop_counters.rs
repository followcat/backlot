//! Loop counters the diagnostics snapshot reports, driven through the real
//! run loop against the fake core's sessions.

use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Duration;

use hc_proto::watch::{OpenRpc, WatchParams, Watcher};
use hc_tui::app::{App, Probe, UiEvent};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::json;
use tokio::sync::mpsc;

#[path = "common/loopcore.rs"]
mod loopcore;

/// Two terminals on one tab: t1 is focused (its active attach plus a size
/// lease), t2 is an observer (its observer plus a size lease).
async fn run_two_panes(publish_panes: bool) -> Arc<Probe> {
    let (sub_tx, mut subs) = mpsc::channel(4);
    let (attach_tx, mut attaches) = mpsc::channel(8);
    let rpc: OpenRpc = Arc::new(|| Box::pin(std::future::pending()));
    let watcher = Watcher::with_openers(rpc, loopcore::sub_opens(sub_tx), WatchParams::default());
    let config = hc_tui::config::Config::default();
    let shell = hc_tui::chrome::ClientShellConfig::from_config(
        &config,
        None,
        None,
        std::path::PathBuf::new(),
    );
    let probe = Arc::new(Probe {
        publish_panes,
        ..Probe::default()
    });
    let mut app = App::new(
        watcher,
        loopcore::attach_opens(attach_tx),
        config.live_keybinds(),
        shell,
        Arc::clone(&probe),
    );
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let (ui_tx, mut ui) = mpsc::channel(8);
    let (_reply_tx, mut replies) = mpsc::channel(1);
    let counters = Arc::clone(&probe);
    let driver = tokio::spawn(async move {
        let _session = loopcore::Session::connect(
            &mut subs,
            &mut attaches,
            4,
            json!({
                "seq": 1,
                "terminals": [
                    loopcore::terminal_json("t1", "one", 1),
                    loopcore::terminal_json("t2", "one", 2),
                ],
            }),
        )
        .await;
        let settled = loopcore::settled_draws(&counters).await;
        ui_tx
            .send(UiEvent::InputRejected("counted input"))
            .await
            .unwrap();
        loopcore::draws_beyond(&counters, settled).await;
        drop(ui_tx);
    });
    tokio::time::timeout(
        Duration::from_secs(15),
        app.run(&mut terminal, &mut ui, &mut replies),
    )
    .await
    .expect("the loop must stay responsive")
    .unwrap();
    driver.await.unwrap();
    probe
}

#[tokio::test]
async fn wakes_dirt_draws_and_panes_are_counted_through_the_loop() {
    let probe = run_two_panes(true).await;
    let wakes: u64 = probe.wake_kinds.iter().map(|k| k.load(Relaxed)).sum();
    let total = probe.wakeups.load(Relaxed);
    assert!(
        wakes <= total && total - wakes <= 1,
        "wakes by kind {wakes} against {total} wakeups"
    );
    assert!(
        probe.wake_kinds[1].load(Relaxed) >= 1,
        "no input wake counted"
    );
    assert!(
        probe.dirty_marks[1].load(Relaxed) >= 1,
        "input dirt not counted"
    );
    assert!(probe.draws.load(Relaxed) >= 1, "no draw counted");
    let panes: Vec<u64> = probe.panes.iter().map(|p| p.load(Relaxed)).collect();
    assert_eq!(
        panes,
        vec![1, 1, 2, 0],
        "active, observer, size lease, popup"
    );
}

#[tokio::test]
async fn panes_are_not_published_unless_diagnostics_ask() {
    let probe = run_two_panes(false).await;
    let panes: Vec<u64> = probe.panes.iter().map(|p| p.load(Relaxed)).collect();
    assert_eq!(panes, vec![0, 0, 0, 0]);
    assert!(probe.wakeups.load(Relaxed) > 0);
}
