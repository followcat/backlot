//! Off-tab observer credit pacing end to end (`r2-load-lag.md` R2-T5):
//! four off-tab `Role::Observe` attaches stream 100 small DATA chunks
//! over ~550ms; their attach actors coalesce the burst onto a shared
//! 500ms grid, so `App::run` wakes a bounded few times instead of once
//! per chunk — while bells and OSC 52 writes still drain through, and
//! a pane coming back on-screen shows its retained output promptly.
//! Same parked, injected transports as `app_loop.rs` — no sockets or
//! sessions.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hc_proto::watch::{OpenRpc, WatchParams, Watcher};
use hc_tui::app::{App, Probe};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::json;
use tokio::sync::mpsc;

#[path = "common/loopcore.rs"]
mod loopcore;

type Rig = (
    App,
    Arc<Probe>,
    mpsc::Receiver<loopcore::fakecore::DuplexCore>,
    mpsc::Receiver<(
        loopcore::fakecore::DuplexCore,
        hc_proto::terminal::AttachParams,
    )>,
);

fn app() -> Rig {
    let (sub_tx, subs) = mpsc::channel(4);
    let (attach_tx, attaches) = mpsc::channel(16);
    let rpc: OpenRpc = Arc::new(|| Box::pin(std::future::pending()));
    let watcher = Watcher::with_openers(rpc, loopcore::sub_opens(sub_tx), WatchParams::default());
    let mut config = hc_tui::config::Config::default();
    // The OSC 52 drain must not raise the copy-feedback card: a card is
    // a real draw, and this test prices background streams at zero.
    config.ui.toast.clipboard.enabled = false;
    let shell = hc_tui::chrome::ClientShellConfig::from_config(
        &config,
        None,
        None,
        std::path::PathBuf::new(),
    );
    let probe = Arc::new(Probe::default());
    let mut app = App::new(
        watcher,
        loopcore::attach_opens(attach_tx),
        config.live_keybinds(),
        shell,
        probe.clone(),
    );
    // The opening page is not what this loop test measures: dismiss it
    // (and its one-time animation) as Esc would, so idle draws are the
    // shell's own.
    app.dismiss_opening();
    (app, probe, subs, attaches)
}

#[tokio::test]
async fn off_tab_observers_pace_data_wakeups_on_the_shared_tick() {
    let (mut app, probe, mut subs, mut attaches) = app();
    let writes = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let sink = writes.clone();
    app.set_host_control(Box::new(move |bytes| {
        sink.lock().unwrap().push(bytes.to_vec());
        Ok(())
    }));
    // 120x40 leaves the re-shown pane taller than the 24-row VT — the
    // paced tail must land inside the drawn window, not clipped below.
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let (ui_tx, mut ui) = mpsc::channel(8);
    let (_reply_tx, mut replies) = mpsc::channel(1);
    let counters = probe.clone();
    let driver = tokio::spawn(async move {
        // t1's active wire, observers t2..t5, five size leases — all
        // five terminals start on the focused tab "one".
        let mut session = loopcore::Session::connect(
            &mut subs,
            &mut attaches,
            10,
            json!({
                "seq": 1,
                "terminals": [
                    loopcore::terminal_json("t1", "one", 1),
                    loopcore::terminal_json("t2", "one", 2),
                    loopcore::terminal_json("t3", "one", 3),
                    loopcore::terminal_json("t4", "one", 4),
                    loopcore::terminal_json("t5", "one", 5),
                ],
            }),
        )
        .await;
        let t1 = session.active_wire("t1");
        let t2 = session.wire("t2", true);
        let t3 = session.wire("t3", true);
        let t4 = session.wire("t4", true);
        let t5 = session.wire("t5", true);
        // Release the initial `?9001` paint everywhere so the observers
        // are fully painted before going off-tab.
        session.send_data(t1, b"\x1b[?9001lt1 ready\r\n").await;
        session.send_data(t2, b"\x1b[?9001lt2 ready\r\n").await;
        session.send_data(t3, b"\x1b[?9001lt3 ready\r\n").await;
        session.send_data(t4, b"\x1b[?9001lt4 ready\r\n").await;
        session.send_data(t5, b"\x1b[?9001lt5 ready\r\n").await;
        // t2..t5 move off the focused tab; their observers stay cached.
        for (seq, (id, tab)) in [
            ("t2", "two"),
            ("t3", "three"),
            ("t4", "four"),
            ("t5", "five"),
        ]
        .iter()
        .enumerate()
        {
            session.move_to_tab(id, tab, seq as u64 + 2).await;
        }
        loopcore::settled_draws(&counters).await;
        let wakeups_before = counters.wakeups.load(Ordering::Relaxed);
        let draws_before = counters.draws.load(Ordering::Relaxed);
        // 25 chunks per off-tab observer (~20ms cadence, ~550ms): 100
        // small writes that unpaced would wake the loop once per chunk.
        for round in 0..25u8 {
            for (wire, id) in [(t2, "t2"), (t3, "t3"), (t4, "t4"), (t5, "t5")] {
                session
                    .send_data(wire, format!("{id} line {round:02}\r\n").as_bytes())
                    .await;
            }
            if round == 5 {
                session.send_data(t3, b"\x07").await;
            }
            if round == 10 {
                session.send_data(t4, b"\x1b]52;c;aGVsbG8=\x07").await;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        session.send_data(t2, b"t2-paced-tail\r\n").await;
        // Past one flush window: every paced buffer has delivered.
        tokio::time::sleep(Duration::from_millis(800)).await;
        let draws_mid = counters.draws.load(Ordering::Relaxed);
        let wakeups = counters.wakeups.load(Ordering::Relaxed) - wakeups_before;
        let log = writes.lock().unwrap().clone();
        // Bring t2 back onto the visible tab: its observer unpaces and
        // the retained screen — tail included — paints promptly.
        session.move_to_tab("t2", "one", 20).await;
        loopcore::draws_beyond(&counters, draws_mid).await;
        loopcore::settled_draws(&counters).await;
        drop(ui_tx);
        (draws_before, draws_mid, wakeups, log)
    });
    tokio::time::timeout(
        Duration::from_secs(30),
        app.run(&mut terminal, &mut ui, &mut replies),
    )
    .await
    .expect("the loop must stay responsive")
    .unwrap();
    let (draws_before, draws_mid, wakeups, log) = driver.await.unwrap();
    assert_eq!(
        draws_mid, draws_before,
        "off-tab observer streams redrew the screen"
    );
    assert!(
        wakeups <= 15,
        "100 chunks across 4 off-tab panes woke the loop {wakeups} times"
    );
    assert!(
        log.iter().any(|bytes| bytes == b"\x07"),
        "t3's bell never reached the host terminal: {log:?}"
    );
    assert!(
        log.iter().any(|bytes| bytes == b"\x1b]52;c;aGVsbG8=\x07"),
        "t4's OSC 52 write never reached the host terminal: {log:?}"
    );
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(
        text.contains("t2-paced-tail"),
        "t2's retained screen lost its paced tail: {text}"
    );
}
