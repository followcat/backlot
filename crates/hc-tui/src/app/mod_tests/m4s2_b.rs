use super::m4s2::*;
use super::*;
use crate::config::ToastDelivery;
use crate::terminal_notify::TerminalNotificationBackend;
use serde_json::json;

#[tokio::test]
async fn prefix_o_focuses_the_toast_target() {
    let mut app = app_alerts();
    publish_word(&mut app, "t1", "blocked", 1);
    assert!(app.notifications.visible.is_some());
    app.focus_visible_notification();
    assert_eq!(app.focused.as_deref(), Some("t1"));
    assert!(app.notifications.visible.is_none());
}

#[tokio::test]
async fn the_toast_card_and_mobile_banner_render() {
    let mut app = app_alerts();
    publish_word(&mut app, "t1", "blocked", 1);
    let lines = screen_lines(&draw_to(&mut app, 120, 40));
    assert!(
        lines.iter().any(|l| l.contains("t1 needs attention")),
        "toast card absent: {lines:?}"
    );
    let lines = screen_lines(&draw_to(&mut app, 50, 20));
    assert!(
        lines.iter().any(|l| l.contains("t1 waiting")),
        "mobile banner absent: {lines:?}"
    );
}

#[tokio::test]
async fn terminal_delivery_writes_osc9_and_tmux_wraps_it() {
    for (backend, in_tmux, needle) in [
        (TerminalNotificationBackend::Ghostty, false, "\x1b]9;"),
        (TerminalNotificationBackend::Kitty, false, "\x1b]99;"),
        (TerminalNotificationBackend::Ghostty, true, "\x1bPtmux;"),
    ] {
        let mut app = app_alerts();
        app.shell.toast_delivery = ToastDelivery::Terminal;
        app.notify_backend = Some(backend);
        app.in_tmux = in_tmux;
        let (bytes, sink) = captured();
        app.set_host_control(sink);
        publish_word(&mut app, "t1", "blocked", 1);
        let bytes = bytes.lock().unwrap();
        assert!(
            bytes.iter().any(|b| b.starts_with(needle.as_bytes())),
            "{backend:?}/{in_tmux}: no OSC bytes in {bytes:?}"
        );
    }
}

#[tokio::test]
async fn system_falls_back_to_the_terminal_sequence_and_off_is_silent() {
    let mut app = app_alerts();
    app.shell.toast_delivery = ToastDelivery::System;
    app.notify_backend = Some(TerminalNotificationBackend::Ghostty);
    let (bytes, sink) = captured();
    app.set_host_control(sink);
    publish_word(&mut app, "t1", "blocked", 1);
    assert!(
        bytes
            .lock()
            .unwrap()
            .iter()
            .any(|b| b.starts_with(b"\x1b]9;"))
    );

    let mut app = app_alerts();
    app.shell.toast_delivery = ToastDelivery::Off;
    app.shell.sound.enabled = false;
    let (bytes, sink) = captured();
    app.set_host_control(sink);
    publish_word(&mut app, "t1", "blocked", 1);
    let bytes = bytes.lock().unwrap();
    assert!(
        !bytes
            .iter()
            .any(|b| b == b"\x07" || b.starts_with(b"\x1b]9") || b.starts_with(b"\x1b]99")),
        "off delivery emitted {bytes:?}"
    );
    assert_eq!(notification_count(&app), 0);
}

#[tokio::test]
async fn an_unwatched_completion_rings_the_bell_once() {
    let mut app = app_alerts();
    app.shell.toast_delivery = ToastDelivery::Off;
    let (bytes, sink) = captured();
    app.set_host_control(sink);
    publish_word(&mut app, "t1", "working", 1);
    publish_word(&mut app, "t1", "done", 2);
    let bytes = bytes.lock().unwrap();
    let bells: Vec<_> = bytes.iter().filter(|b| b.as_slice() == b"\x07").collect();
    assert_eq!(bells.len(), 1, "{bytes:?}");
}

#[tokio::test]
async fn drained_pane_bells_reach_the_outer_terminal() {
    let mut app = app_alerts();
    let (bytes, sink) = captured();
    app.set_host_control(sink);
    app.pane.pending_bells = 3;
    app.drain_bells();
    assert_eq!(
        bytes.lock().unwrap().as_slice(),
        &[b"\x07\x07\x07".to_vec()]
    );
    assert_eq!(app.pane.take_bells(), 0);
}

#[tokio::test]
async fn a_program_copy_reaches_the_outer_terminal_once() {
    let mut app = app_alerts();
    let (bytes, sink) = captured();
    app.set_host_control(sink);
    // The focused pane's input wire and its observer see the same output.
    app.pane.pending_clipboard.push(b"hello".to_vec());
    let mut observer = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
    observer.pending_clipboard.push(b"hello".to_vec());
    app.observers.insert("t1".into(), observer);
    app.drain_clipboard_writes();
    assert_eq!(
        bytes.lock().unwrap().as_slice(),
        &[b"\x1b]52;c;aGVsbG8=\x07".to_vec()]
    );
    assert!(app.pane.take_clipboard_writes().is_empty());
}

#[tokio::test]
async fn a_program_copy_survives_an_unpainted_observer() {
    let mut app = app_alerts();
    let (bytes, sink) = captured();
    app.set_host_control(sink);
    app.open_attach = std::sync::Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
    app.pane.want(Some(Spec {
        terminal: "t1".into(),
        role: Role::Active,
    }));
    // The observer is still opening: its first paint clears captures, so
    // the active wire is then the only live copy of the write.
    app.pane.pending_clipboard.push(b"hello".to_vec());
    app.observers.insert(
        "t1".into(),
        pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone()),
    );
    app.drain_clipboard_writes();
    assert_eq!(
        bytes.lock().unwrap().as_slice(),
        &[b"\x1b]52;c;aGVsbG8=\x07".to_vec()]
    );
}

#[tokio::test]
async fn created_terminals_carry_the_herdr_env_marker() {
    // herdr injects HERDR_ENV=1 into every spawned pane; programs that
    // forward clipboard writes through the terminal key on it.
    let mut app = app_with(&["t1"]);
    app.focused = Some("t1".into());
    app.refresh_projection();
    let creation = app.creation(None, false).unwrap();
    assert_eq!(creation.params.env.get("HERDR_ENV"), Some(&"1".into()));
    assert!(
        creation.params.rm,
        "shell spawns remove their record on exit"
    );
    assert!(creation.params.keep);
    let (creation, _) = app.workspace_creation().unwrap();
    assert_eq!(creation.params.env.get("HERDR_ENV"), Some(&"1".into()));
    assert!(creation.params.rm);
    assert!(creation.params.keep);
}

#[tokio::test]
async fn seen_writes_the_turn_once_per_turn_and_retries_a_failure() {
    let mut app = app_alerts();
    app.focused = Some("t1".into());
    publish_word(&mut app, "t1", "idle", 5);
    assert!(app.seen_looked.contains_key("t1"));
    let mut rx = operations(&mut app);
    let server = tokio::spawn(async move {
        // First conn: the write lands and the core accepts it.
        let mut core = rx.recv().await.unwrap();
        core.read_hello().await;
        core.answer_hello(true, &["terminal.label"], &[]).await;
        let first = core.read_request().await;
        assert_eq!(first["method"], "terminal.label");
        assert_eq!(
            first["params"],
            json!({"terminal": "t1", "labels": {"herdr.seen": "5"}})
        );
        core.reply(&first["id"], json!({})).await;
    });
    server.await.unwrap();
    drain_seen(&mut app).await;
    // The same turn again: no second write — `looked` holds.
    app.share_seen();
    assert!(app.seen_inflight.is_empty());
    // A failed result clears the marker so the next pass retries.
    app.on_seen_result(super::seen::SeenResult {
        terminal: "t1".into(),
        turn: 5,
        result: Err("core rejected the label".into()),
    });
    assert!(!app.seen_looked.contains_key("t1"));
    app.share_seen();
    assert!(app.seen_inflight.contains(&("t1".to_owned(), 5)));
}

#[tokio::test]
async fn seen_skips_a_lower_turn_and_the_wordless() {
    let mut app = app_alerts();
    app.focused = Some("t1".into());
    app.refresh_projection();
    {
        let mut terms: Vec<_> = app.view.inventory.terminals().values().cloned().collect();
        let term = terms.iter_mut().find(|t| t.address == "t1").unwrap();
        term.labels.insert("herdr.seen".into(), "9".into());
        term.status = Some(crate::sidebar::fixture::status("idle"));
        term.turn = 5;
        let seq = app.view.inventory.seq() + 1;
        app.view.inventory_mut().replace(seq, terms);
    }
    app.share_seen();
    assert!(app.seen_inflight.is_empty(), "the core's turn 9 wins");
    assert_eq!(app.seen_looked.get("t1"), Some(&9));
}

#[tokio::test]
async fn seen_skips_the_invisible_and_the_dead() {
    let mut app = app_alerts();
    // `t1` is not on the focused tab — never seen-written.
    publish_word(&mut app, "t1", "idle", 5);
    assert!(app.seen_inflight.is_empty() && !app.seen_looked.contains_key("t1"));
    // A finished terminal is not live — no write either.
    app.focused = Some("t1".into());
    app.refresh_projection();
    {
        let mut terms: Vec<_> = app.view.inventory.terminals().values().cloned().collect();
        let term = terms.iter_mut().find(|t| t.address == "t1").unwrap();
        term.state = "finished".into();
        let seq = app.view.inventory.seq() + 1;
        app.view.inventory_mut().replace(seq, terms);
    }
    app.share_seen();
    assert!(!app.seen_inflight.iter().any(|(id, _)| id == "t1"));
}
