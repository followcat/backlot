//! Exercise the real application loop with parked, injected transports.
//! No sockets or sessions are opened.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hc_proto::watch::{OpenRpc, OpenSub, WatchParams, Watcher};
use hc_tui::app::{App, Probe};
use ratatui::{Terminal, backend::TestBackend};
use tokio::sync::mpsc;

#[path = "common/loopcore.rs"]
mod loopcore;

fn app() -> (App, Arc<Probe>) {
    app_with_config(&hc_tui::config::Config::default())
}

fn app_with_config(config: &hc_tui::config::Config) -> (App, Arc<Probe>) {
    let rpc: OpenRpc = Arc::new(|| Box::pin(std::future::pending()));
    let sub: OpenSub = Arc::new(|_| Box::pin(std::future::pending()));
    let watcher = Watcher::with_openers(rpc, sub, WatchParams::default());
    let shell = hc_tui::chrome::ClientShellConfig::from_config(
        config,
        None,
        None,
        std::path::PathBuf::new(),
    );
    let probe = Arc::new(Probe::default());
    let mut app = App::new(
        watcher,
        Arc::new(|_| Box::pin(std::future::pending())),
        config.live_keybinds(),
        shell,
        probe.clone(),
    );
    // The opening page is not what these loop tests measure: dismiss
    // it (and its one-time animation) as Esc would.
    app.dismiss_opening();
    (app, probe)
}

async fn idle_counters(close_replies: bool) -> (u64, u64) {
    let (mut app, probe) = app();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let (_ui_tx, mut ui) = mpsc::channel(1);
    let (reply_tx, mut replies) = mpsc::channel(1);
    let _reply_guard = (!close_replies).then_some(reply_tx);
    let _ = tokio::time::timeout(
        Duration::from_millis(200),
        app.run(&mut terminal, &mut ui, &mut replies),
    )
    .await;
    (
        probe.wakeups.load(Ordering::Relaxed),
        probe.draws.load(Ordering::Relaxed),
    )
}

#[tokio::test]
async fn settled_app_does_not_poll_or_redraw_periodically() {
    let (wakeups, draws) = idle_counters(false).await;
    assert!(wakeups <= 16, "idle app woke {wakeups} times");
    assert!(draws <= 4, "idle app drew {draws} times");
}

#[tokio::test]
async fn closing_host_replies_does_not_create_a_busy_loop() {
    let (wakeups, draws) = idle_counters(true).await;
    assert!(wakeups <= 16, "closed reply channel woke {wakeups} times");
    assert!(draws <= 4, "closed reply channel caused {draws} draws");
}

#[tokio::test]
async fn sidebar_divider_drag_changes_the_drawn_geometry() {
    use hc_tui::raw_events::{Decoded, events_from_framed_chunks};

    let (mut app, probe) = app();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let (ui_tx, mut ui) = mpsc::channel(8);
    let (_reply_tx, mut replies) = mpsc::channel(1);
    let producer = tokio::spawn(async move {
        // The divider hit rectangle exists only after the first frame.
        while probe.draws.load(Ordering::Relaxed) == 0 {
            tokio::task::yield_now().await;
        }
        for bytes in [
            &b"\x1b[<0;26;11M"[..],
            &b"\x1b[<32;36;11M"[..],
            &b"\x1b[<0;36;11m"[..],
        ] {
            let Decoded::Ui(event) = events_from_framed_chunks(vec![bytes.to_vec()])
                .pop()
                .expect("complete mouse report")
            else {
                panic!("mouse report was treated as a host reply");
            };
            ui_tx.send(event).await.unwrap();
        }
        // Closing the input ends the run, which returns only once the
        // drag's final geometry has been drawn.
        drop(ui_tx);
    });
    tokio::time::timeout(
        Duration::from_secs(2),
        app.run(&mut terminal, &mut ui, &mut replies),
    )
    .await
    .expect("mouse input and shutdown must stay responsive")
    .unwrap();
    producer.await.unwrap();
    assert_eq!(terminal.backend().buffer()[(35, 10)].symbol(), "│");
    assert_ne!(terminal.backend().buffer()[(25, 10)].symbol(), "│");
}

#[tokio::test]
async fn late_host_appearance_updates_the_drawn_auto_theme() {
    let mut config = hc_tui::config::Config::default();
    config.theme.auto_switch = true;
    let expected = hc_tui::chrome::ClientShellConfig::from_config(
        &config,
        Some(hc_tui::chrome::palette::HostAppearance::Light),
        None,
        std::path::PathBuf::new(),
    );
    let (mut app, probe) = app_with_config(&config);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let (ui_tx, mut ui) = mpsc::channel(1);
    let (reply_tx, mut replies) = mpsc::channel(1);
    let producer = tokio::spawn(async move {
        while probe.draws.load(Ordering::Relaxed) == 0 {
            tokio::task::yield_now().await;
        }
        reply_tx.send(hc_tui::host::HostReply::Light).await.unwrap();
        while probe.draws.load(Ordering::Relaxed) < 2 {
            tokio::task::yield_now().await;
        }
        drop(ui_tx);
    });
    tokio::time::timeout(
        Duration::from_secs(2),
        app.run(&mut terminal, &mut ui, &mut replies),
    )
    .await
    .expect("host reply and shutdown must stay responsive")
    .unwrap();
    producer.await.unwrap();
    assert_eq!(
        terminal.backend().buffer()[(0, 0)].fg,
        expected.palette.overlay0
    );
}

#[tokio::test]
async fn rejected_input_uses_the_reference_notice_surface() {
    let (mut app, probe) = app();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let (ui_tx, mut ui) = mpsc::channel(1);
    let (_reply_tx, mut replies) = mpsc::channel(1);
    let producer = tokio::spawn(async move {
        while probe.draws.load(Ordering::Relaxed) == 0 {
            tokio::task::yield_now().await;
        }
        ui_tx
            .send(hc_tui::app::UiEvent::InputRejected(
                "Paste exceeds the input limit",
            ))
            .await
            .unwrap();
        while probe.draws.load(Ordering::Relaxed) < 2 {
            tokio::task::yield_now().await;
        }
        drop(ui_tx);
    });
    tokio::time::timeout(
        Duration::from_secs(2),
        app.run(&mut terminal, &mut ui, &mut replies),
    )
    .await
    .unwrap()
    .unwrap();
    producer.await.unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(text.contains("Input rejected"), "{text}");
    assert!(text.contains("Paste exceeds the input limit"), "{text}");
}

/// A state change processed inside the frame cap's window defers its
/// draw; if the input source ends in that window `run` must still paint
/// it before returning — the channel closing must not drop the last
/// frame.
#[tokio::test]
async fn closing_input_still_draws_the_last_coalesced_state() {
    let (mut app, probe) = app();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let (ui_tx, mut ui) = mpsc::channel(8);
    let (_reply_tx, mut replies) = mpsc::channel(1);
    let producer = tokio::spawn(async move {
        while probe.draws.load(Ordering::Relaxed) == 0 {
            tokio::task::yield_now().await;
        }
        // Lands in the frame slot the first draw just opened, so its
        // draw is deferred to a deadline the closed channel outruns.
        ui_tx
            .send(hc_tui::app::UiEvent::InputRejected("the closing frame"))
            .await
            .unwrap();
        drop(ui_tx);
    });
    tokio::time::timeout(
        Duration::from_secs(2),
        app.run(&mut terminal, &mut ui, &mut replies),
    )
    .await
    .expect("input close and shutdown must stay responsive")
    .unwrap();
    producer.await.unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(text.contains("the closing frame"), "{text}");
}

#[tokio::test]
async fn rejected_notice_expires_during_idle_without_periodic_redraws() {
    let (mut app, probe) = app();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let (ui_tx, mut ui) = mpsc::channel(1);
    let (_reply_tx, mut replies) = mpsc::channel(1);
    let counters = probe.clone();
    let producer = tokio::spawn(async move {
        ui_tx
            .send(hc_tui::app::UiEvent::InputRejected(
                "Paste exceeds the input limit",
            ))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(3150)).await;
        drop(ui_tx);
    });
    tokio::time::timeout(
        Duration::from_secs(4),
        app.run(&mut terminal, &mut ui, &mut replies),
    )
    .await
    .unwrap()
    .unwrap();
    producer.await.unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(!text.contains("Input rejected"), "{text}");
    assert!(counters.draws.load(Ordering::Relaxed) <= 4);
    assert!(counters.wakeups.load(Ordering::Relaxed) <= 20);
}

/// A `%H:%M` clock re-arms a one-second tick, but the formatted text
/// only changes on a wall-clock minute boundary. Unchanged ticks must
/// not redraw: the measured window opens only after the draws settle —
/// a straggling startup frame must not count. The allowance is the
/// number of minute boundaries the window actually crossed, so the
/// check stays strict no matter where the wall clock sits.
#[tokio::test]
async fn tab_bar_datetime_unchanged_minutes_skip_idle_redraws() {
    let mut config = hc_tui::config::Config::default();
    config.ui.tab_bar_right = vec![hc_tui::config::TabBarRightEntryConfig::Datetime {
        format: "%H:%M".into(),
    }];
    let (mut app, probe) = app_with_config(&config);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let (ui_tx, mut ui) = mpsc::channel(1);
    let (_reply_tx, mut replies) = mpsc::channel(1);
    let counters = probe.clone();
    let producer = tokio::spawn(async move {
        let settled = loopcore::settled_draws(&counters).await;
        assert!(settled > 0, "the initial frame never drew");
        let start_minute = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            / 60;
        tokio::time::sleep(Duration::from_millis(2200)).await;
        let end_minute = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            / 60;
        let later = counters.draws.load(Ordering::Relaxed);
        drop(ui_tx);
        (settled, later, end_minute - start_minute)
    });
    tokio::time::timeout(
        Duration::from_secs(6),
        app.run(&mut terminal, &mut ui, &mut replies),
    )
    .await
    .expect("the loop must stay responsive")
    .unwrap();
    let (settled, later, boundaries) = producer.await.unwrap();
    assert!(
        later - settled <= boundaries,
        "idle clock ticks drew {} extra frames across {} minute boundaries",
        later - settled,
        boundaries
    );
}

/// Observer output for a terminal that is not on the focused tab is
/// retained state, not a visible change; likewise a closed replies
/// channel. Neither may arm a redraw.
#[tokio::test]
async fn off_tab_observer_output_and_closed_replies_do_not_trigger_redraws() {
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
    let probe = Arc::new(Probe::default());
    let mut app = App::new(
        watcher,
        loopcore::attach_opens(attach_tx),
        config.live_keybinds(),
        shell,
        probe.clone(),
    );
    // Idle draws here must be the shell's, not the opening page's
    // one-time animation.
    app.dismiss_opening();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let (ui_tx, mut ui) = mpsc::channel(8);
    let (reply_tx, mut replies) = mpsc::channel(1);
    let counters = probe.clone();
    let driver = tokio::spawn(async move {
        // The snapshot publishes t1+t2 on one tab; the active pane,
        // t2's observer and both size leases attach — 4 conns.
        let mut session = loopcore::Session::connect(
            &mut subs,
            &mut attaches,
            4,
            serde_json::json!({
                "seq": 1,
                "terminals": [
                    loopcore::terminal_json("t1", "one", 1),
                    loopcore::terminal_json("t2", "one", 2),
                ],
            }),
        )
        .await;
        let baseline = loopcore::settled_draws(&counters).await;
        assert!(baseline > 0, "the initial frame never drew");
        // Paint t2 once while it still shares the focused tab — a real
        // visible change, proving the wire works end to end.
        let t2 = session.wire("t2", true);
        session.send_data(t2, b"\x1b[?9001lt2 line 1\r\n").await;
        let painted = loopcore::settled_draws(&counters).await;
        assert!(painted > baseline, "visible t2 output did not draw");
        // Move t2 onto a different tab: its observer stays attached but
        // now feeds a background pane.
        session.move_to_tab("t2", "two", 2).await;
        loopcore::draws_beyond(&counters, painted).await;
        let moved = loopcore::settled_draws(&counters).await;
        // Off-tab DATA chunks plus the replies channel closing: pure
        // retained-state and liveness updates, zero redraws.
        for chunk in 0..4u8 {
            session
                .send_data(t2, format!("t2 later {chunk}\r\n").as_bytes())
                .await;
            tokio::time::sleep(Duration::from_millis(60)).await;
        }
        drop(reply_tx);
        tokio::time::sleep(Duration::from_millis(300)).await;
        let end = counters.draws.load(Ordering::Relaxed);
        drop(ui_tx);
        (moved, end)
    });
    tokio::time::timeout(
        Duration::from_secs(15),
        app.run(&mut terminal, &mut ui, &mut replies),
    )
    .await
    .expect("the loop must stay responsive")
    .unwrap();
    let (moved, end) = driver.await.unwrap();
    assert_eq!(
        end, moved,
        "off-tab output or closed replies redrew the screen"
    );
}
