//! Exercise the real application loop with parked, injected transports.
//! No sockets or sessions are opened.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use hc_proto::watch::{OpenRpc, OpenSub, WatchParams, Watcher};
use hc_tui::app::{App, Probe};
use ratatui::{Terminal, backend::TestBackend};
use tokio::sync::mpsc;

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
    let app = App::new(
        watcher,
        Arc::new(|_| Box::pin(std::future::pending())),
        config.live_keybinds(),
        shell,
        probe.clone(),
        Box::new(|_, _| {}),
    );
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
