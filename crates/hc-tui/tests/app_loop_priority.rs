//! The loop's arm order is a contract: queued UI events always outrank
//! host replies and pane DATA, and the startup host-reply storm folds
//! into one facts application. Same parked, injected transports as
//! `app_loop.rs` — no sockets or sessions.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::Duration;

use hc_proto::watch::{OpenRpc, OpenSub, WatchParams, Watcher};
use hc_tui::app::{App, Probe, UiEvent};
use hc_tui::host::HostReply;
use hc_vt::Rgb;
use ratatui::{Terminal, backend::TestBackend};
use tokio::sync::mpsc;

#[path = "common/loopcore.rs"]
mod loopcore;

fn app() -> (App, Arc<Probe>) {
    let rpc: OpenRpc = Arc::new(|| Box::pin(std::future::pending()));
    let sub: OpenSub = Arc::new(|_| Box::pin(std::future::pending()));
    let watcher = Watcher::with_openers(rpc, sub, WatchParams::default());
    let config = hc_tui::config::Config::default();
    let shell = hc_tui::chrome::ClientShellConfig::from_config(
        &config,
        None,
        None,
        std::path::PathBuf::new(),
    );
    let probe = Arc::new(Probe::default());
    let app = App::new(
        watcher,
        Arc::new(|_| Box::pin(std::future::pending())),
        config.live_keybinds(),
        shell,
        probe.clone(),
    );
    (app, probe)
}

/// Poll the run loop once: a single poll drains every armed source and
/// parks only when the `select!` has nothing left ready.
fn poll_once<F: Future>(run_fut: &mut Pin<Box<F>>) -> Poll<F::Output> {
    run_fut
        .as_mut()
        .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
}

/// Alternate scheduler yields and run polls `rounds` times — spawned
/// attach forwarders and the watcher task get their turns, and each
/// poll consumes everything queued so far.
async fn settle<F: Future>(run_fut: &mut Pin<Box<F>>, rounds: usize) {
    for _ in 0..rounds {
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        assert!(
            matches!(poll_once(run_fut), Poll::Pending),
            "the run ended while its input source was still open"
        );
    }
}

/// The startup probe storm — OSC 10/11 plus `OSC 4;0..=255` — reaches
/// the loop as one queued burst. The reply arm must fold it into a
/// single facts application (at most two when a first reply pops
/// before the rest land), not one application per reply.
#[tokio::test]
async fn startup_host_reply_storm_batches_into_at_most_two_fact_updates() {
    let (mut app, probe) = app();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let (ui_tx, mut ui) = mpsc::channel(8);
    let (reply_tx, mut replies) = mpsc::channel(300);
    let rgb = |v: u8| Rgb { r: v, g: v, b: v };
    for reply in std::iter::once(HostReply::Foreground(rgb(1)))
        .chain(std::iter::once(HostReply::Background(rgb(2))))
        .chain((0u16..=255).map(|i| HostReply::Palette(i as u8, rgb(i as u8))))
    {
        reply_tx.send(reply).await.unwrap();
    }
    let mut run_fut = Box::pin(app.run(&mut terminal, &mut ui, &mut replies));
    settle(&mut run_fut, 4).await;
    let applied = probe.host_facts.load(Ordering::Relaxed);
    assert!(
        (1..=2).contains(&applied),
        "the 258-reply storm applied host facts {applied} times"
    );
    // A palette entry the storm already carried is a no-op the second
    // time — proof its slot holds exactly that color.
    reply_tx.send(HostReply::Palette(7, rgb(7))).await.unwrap();
    settle(&mut run_fut, 2).await;
    assert_eq!(
        probe.host_facts.load(Ordering::Relaxed),
        applied,
        "re-applying an already-known palette entry was not a no-op"
    );
    drop(ui_tx);
    assert!(
        matches!(poll_once(&mut run_fut), Poll::Ready(Ok(()))),
        "closing the input source must end the run"
    );
}

/// Input priority is a contract: a Focus event enqueued behind an
/// off-tab DATA chunk and a queued host reply is still handled first,
/// every round — an unordered `select!` wins that race about once in
/// three rounds, so twelve rounds pin the bias down. Typed input
/// behind a DATA burst must reach the active wire without starving.
#[tokio::test]
async fn key_and_ui_events_precede_queued_host_replies_and_data_bursts() {
    let (sub_tx, subs) = mpsc::channel(4);
    let (attach_tx, attaches) = mpsc::channel(8);
    let rpc: OpenRpc = Arc::new(|| Box::pin(std::future::pending()));
    let watcher = Watcher::with_openers(rpc, loopcore::sub_opens(sub_tx), WatchParams::default());
    let config = hc_tui::config::Config::default();
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
    // The host-control tap: a gained focus re-queries the color scheme
    // ("focus"), a Dark/Light report re-queries colors ("theme"), and
    // forwarded pane BELs write raw bell bytes ("bell").
    let writes = Arc::new(Mutex::new(Vec::<&'static str>::new()));
    let sink = writes.clone();
    app.set_host_control(Box::new(move |bytes| {
        let tag = if bytes.starts_with(b"\x1b[?996n") {
            Some("focus")
        } else if bytes.starts_with(b"\x1b]10;?") {
            Some("theme")
        } else if bytes.iter().all(|b| *b == b'\x07') {
            Some("bell")
        } else {
            None
        };
        if let Some(tag) = tag {
            sink.lock().unwrap().push(tag);
        }
        Ok(())
    }));
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let (ui_tx, mut ui) = mpsc::channel(8);
    let (reply_tx, mut replies) = mpsc::channel(8);
    let mut run_fut = Box::pin(app.run(&mut terminal, &mut ui, &mut replies));
    // t1 focused on tab "one", t2 moved to tab "two" — the snapshot
    // publishes both, the active pane plus observers and size leases
    // attach: five conns, like the app_loop off-tab test.
    let session_task = tokio::spawn(async move {
        let mut subs = subs;
        let mut attaches = attaches;
        loopcore::Session::connect(
            &mut subs,
            &mut attaches,
            5,
            serde_json::json!({
                "seq": 1,
                "terminals": [
                    loopcore::terminal_json("t1", "one", 1),
                    loopcore::terminal_json("t2", "one", 2),
                ],
            }),
        )
        .await
    });
    let mut session = {
        let mut connected = None;
        for _ in 0..200 {
            if session_task.is_finished() {
                connected = Some(session_task.await.unwrap());
                break;
            }
            settle(&mut run_fut, 1).await;
        }
        connected.expect("the session never connected")
    };
    session.move_to_tab("t2", "two", 2).await;
    // The `?9001` marker on t1's active wire releases its paint gate —
    // queued human input stays parked until it lands.
    session
        .send_data(session.active_wire("t1"), b"\x1b[?9001lt1 ready\r\n")
        .await;
    settle(&mut run_fut, 4).await;
    let t2_observe = session.wire("t2", true);
    for round in 0..12 {
        let start = writes.lock().unwrap().len();
        session.send_data(t2_observe, b"\x07").await;
        // Let the spawned attach task land the chunk in `pane_rx`
        // before the reply and the key press queue up behind it.
        for _ in 0..12 {
            tokio::task::yield_now().await;
        }
        reply_tx.send(HostReply::Light).await.unwrap();
        ui_tx.send(UiEvent::Focus(true)).await.unwrap();
        settle(&mut run_fut, 2).await;
        let log = writes.lock().unwrap();
        assert_eq!(
            &log[start..],
            ["focus", "theme", "bell"].as_slice(),
            "round {round} lost input priority: {:?}",
            &log[start..]
        );
    }
    // Typed text queued behind an off-tab burst still reaches the
    // focused terminal's wire — input priority without starvation.
    session.send_data(t2_observe, b"burst-1\r\n").await;
    session.send_data(t2_observe, b"burst-2\r\n").await;
    for _ in 0..12 {
        tokio::task::yield_now().await;
    }
    ui_tx.send(UiEvent::Text("k".into())).await.unwrap();
    let start = writes.lock().unwrap().len();
    settle(&mut run_fut, 2).await;
    let t1_active = session.active_wire("t1");
    session.expect_input(t1_active, b"k").await;
    assert!(
        writes.lock().unwrap()[start..].is_empty(),
        "an off-tab DATA burst wrote to the host terminal"
    );
    drop(ui_tx);
    // EOF the fake-core wires so shutdown detaches resolve without
    // waiting out the close deadline.
    drop(session);
    tokio::time::timeout(Duration::from_secs(30), &mut run_fut)
        .await
        .expect("closing the input source must end the run")
        .expect("run must exit cleanly");
}
