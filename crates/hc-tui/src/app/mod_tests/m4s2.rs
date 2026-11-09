//! M4 slice 2: synthesized notifications, delivery modes and `herdr.seen`.
use super::*;
use crate::config::ToastDelivery;
use crate::terminal_notify::TerminalNotificationBackend;
use crate::testcore;
use serde_json::json;
use std::sync::Mutex;

fn pending(app: &mut App) {
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
}

/// Routes operations at a fake core answering as `core-fake-1`.
fn operations(app: &mut App) -> mpsc::Receiver<testcore::FakeCore> {
    let (accepted, rx) = mpsc::channel(4);
    let (rpc, _sub) = testcore::openers(accepted);
    app.set_operations(rpc);
    app.view.core.as_mut().unwrap().id = "core-fake-1".into();
    rx
}

/// `t1` lives in `space-a`/`tab-a`, `t2` in `space-b`/`tab-b`; the focus
/// stays on `t2` so `t1` is an unseen pane on an inactive tab. The first
/// reconcile seeds word baselines silently.
fn app_alerts() -> App {
    let mut app = app();
    let mut t1 = crate::sidebar::fixture::agent("t1", "idle");
    t1.labels.insert("herdr.space".into(), "space-a".into());
    t1.labels.insert("herdr.tab".into(), "tab-a".into());
    let mut t2 = crate::sidebar::fixture::agent("t2", "idle");
    t2.labels.insert("herdr.space".into(), "space-b".into());
    t2.labels.insert("herdr.tab".into(), "tab-b".into());
    app.view = crate::sidebar::fixture::view(vec![t1, t2], vec![]);
    app.focused = Some("t2".into());
    pending(&mut app);
    app.reconcile_published_view();
    app.shell.toast_delivery = ToastDelivery::Herdr;
    app.shell.toast_delay_seconds = 0;
    app
}

/// Stamp `term.activity` with a `completion_ordinal` like the core does —
/// without one an `idle`/`done` word projects `Idle` (nothing unseen).
fn stamp_activity(term: &mut hc_proto::terminal::Terminal, records: serde_json::Value) {
    term.activity = Some(
        serde_json::from_value(json!({
            "core_id": "core-fake-1", "epoch": "boot", "scope": 1, "revision": 1,
            "records": records,
        }))
        .unwrap(),
    );
}

/// Republish `address` at `word`/`turn` and reconcile — one watcher cycle.
fn publish_word(app: &mut App, address: &str, word: &str, turn: u64) {
    let mut terms: Vec<_> = app.view.inventory.terminals().values().cloned().collect();
    let term = terms
        .iter_mut()
        .find(|t| t.address == address)
        .expect("test pane");
    term.status = Some(crate::sidebar::fixture::status(word));
    term.turn = turn;
    stamp_activity(
        term,
        json!({"": {"state_ordinal": turn, "completion_ordinal": turn}}),
    );
    let seq = app.view.inventory.seq() + 1;
    app.view.inventory.replace(seq, terms);
    app.reconcile_published_view();
}

fn visible_title(app: &App) -> Option<String> {
    app.notifications
        .visible
        .as_ref()
        .map(|n| n.event.title.clone())
}

fn notification_count(app: &App) -> usize {
    usize::from(app.notifications.visible.is_some())
        + app.notifications.queued.len()
        + app.notifications.pending.len()
}

fn captured() -> (Arc<Mutex<Vec<Vec<u8>>>>, HostControlSink) {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let sink = {
        let captured = captured.clone();
        Box::new(move |bytes: &[u8]| {
            captured.lock().unwrap().push(bytes.to_vec());
            Ok(())
        }) as HostControlSink
    };
    (captured, sink)
}

/// Drain queued seen results the way the runtime's select arm does.
async fn drain_seen(app: &mut App) {
    while let Ok(result) =
        tokio::time::timeout(std::time::Duration::from_millis(200), app.seen_rx.recv()).await
    {
        match result {
            Some(result) => app.on_seen_result(result),
            None => break,
        }
    }
}

#[tokio::test]
async fn entering_blocked_alerts_an_unseen_pane() {
    let mut app = app_alerts();
    publish_word(&mut app, "t1", "blocked", 1);
    assert_eq!(visible_title(&app).as_deref(), Some("t1 needs attention"));
}

#[tokio::test]
async fn busy_to_done_alerts_an_unseen_pane() {
    let mut app = app_alerts();
    publish_word(&mut app, "t1", "working", 1);
    publish_word(&mut app, "t1", "done", 2);
    assert_eq!(visible_title(&app).as_deref(), Some("t1 finished"));
}

#[tokio::test]
async fn busy_to_error_alerts_but_idle_to_done_does_not() {
    let mut app = app_alerts();
    publish_word(&mut app, "t1", "working", 1);
    publish_word(&mut app, "t1", "error", 2);
    assert_eq!(visible_title(&app).as_deref(), Some("t1 needs attention"));

    let mut app = app_alerts();
    // `idle → done` never left `working`, so the core's turn stays put —
    // without a bump or a busy exit there is no completion to report.
    publish_word(&mut app, "t1", "done", 0);
    assert_eq!(notification_count(&app), 0);
}

#[tokio::test]
async fn blocked_variants_and_working_continuation_stay_quiet() {
    let mut app = app_alerts();
    publish_word(&mut app, "t1", "blocked", 1);
    assert_eq!(notification_count(&app), 1);
    publish_word(&mut app, "t1", "error", 2);
    // blocked → error is not a new alert; the earlier toast may linger.
    assert_eq!(notification_count(&app), 1);

    let mut app = app_alerts();
    publish_word(&mut app, "t1", "working", 1);
    publish_word(&mut app, "t1", "working", 2);
    assert_eq!(notification_count(&app), 0);
}

#[tokio::test]
async fn sub_agents_never_alert() {
    let mut app = app_alerts();
    {
        let mut terms: Vec<_> = app.view.inventory.terminals().values().cloned().collect();
        let term = terms.iter_mut().find(|t| t.address == "t1").unwrap();
        term.parent = Some("t9".into());
        let seq = app.view.inventory.seq() + 1;
        app.view.inventory.replace(seq, terms);
    }
    app.reconcile_published_view();
    publish_word(&mut app, "t1", "working", 1);
    publish_word(&mut app, "t1", "done", 2);
    assert_eq!(notification_count(&app), 0);
}

#[tokio::test]
async fn a_pane_finishing_while_watched_neither_toasts_nor_rings() {
    let mut app = app_alerts();
    let (bytes, sink) = captured();
    app.set_host_control(sink);
    publish_word(&mut app, "t2", "working", 1);
    publish_word(&mut app, "t2", "done", 2);
    assert_eq!(notification_count(&app), 0);
    let bytes = bytes.lock().unwrap();
    assert!(
        !bytes
            .iter()
            .any(|b| b == b"\x07" || b.starts_with(b"\x1b]9") || b.starts_with(b"\x1b]99")),
        "watched completion emitted {bytes:?}"
    );
}

#[tokio::test]
async fn one_alert_per_word_transition() {
    let mut app = app_alerts();
    publish_word(&mut app, "t1", "blocked", 2);
    assert_eq!(notification_count(&app), 1);
    // The word diff is the dedup: a republished identical word can't
    // re-alert; a fresh transition in the same turn still can.
    publish_word(&mut app, "t1", "blocked", 2);
    assert_eq!(notification_count(&app), 1);
    publish_word(&mut app, "t1", "working", 2);
    publish_word(&mut app, "t1", "done", 2);
    assert_eq!(notification_count(&app), 1);
    assert_eq!(visible_title(&app).as_deref(), Some("t1 finished"));
}

#[tokio::test]
async fn blocked_to_done_drops_the_stale_completion() {
    // The reference's event lands one snapshot early: Finished validated
    // against a still-Blocked pane is stale, so `blocked → idle` never
    // toasts. hc-tui mirrors that with the pre-transition word.
    let mut app = app_alerts();
    publish_word(&mut app, "t1", "blocked", 1);
    assert_eq!(visible_title(&app).as_deref(), Some("t1 needs attention"));
    publish_word(&mut app, "t1", "idle", 1);
    assert_eq!(notification_count(&app), 0);
}

#[tokio::test]
async fn a_completion_compressed_into_a_turn_bump_still_toasts() {
    // A working exit inside a watcher gap leaves the word untouched but
    // still climbs `turn` — the completion must alert anyway.
    let mut app = app_alerts();
    publish_word(&mut app, "t1", "done", 1);
    assert_eq!(visible_title(&app).as_deref(), Some("t1 finished"));
    // Without a bump the identical word stays silent — the word diff is
    // still the dedup.
    publish_word(&mut app, "t1", "done", 1);
    assert_eq!(notification_count(&app), 1);
}

/// Republish `address` with a named record at `word` — like the
/// chronology `build/test` reports.
fn publish_record(app: &mut App, address: &str, record: &str, word: &str, turn: u64) {
    let mut terms: Vec<_> = app.view.inventory.terminals().values().cloned().collect();
    let term = terms
        .iter_mut()
        .find(|t| t.address == address)
        .expect("test pane");
    term.records
        .insert(record.to_owned(), crate::sidebar::fixture::status(word));
    term.turn = turn;
    stamp_activity(
        term,
        json!({record: {"state_ordinal": turn, "completion_ordinal": turn}}),
    );
    let seq = app.view.inventory.seq() + 1;
    app.view.inventory.replace(seq, terms);
    app.reconcile_published_view();
}

#[tokio::test]
async fn a_named_record_completion_alerts() {
    let mut app = app_alerts();
    publish_record(&mut app, "t1", "build/test", "working", 1);
    publish_record(&mut app, "t1", "build/test", "done", 2);
    assert_eq!(visible_title(&app).as_deref(), Some("t1 finished"));
}

#[tokio::test]
async fn a_finished_event_is_dropped_when_the_pane_is_working_again() {
    let mut app = app_alerts();
    app.shell.toast_delay_seconds = 60;
    let now = Instant::now();
    publish_word(&mut app, "t1", "working", 1);
    publish_word(&mut app, "t1", "done", 2);
    assert_eq!(notification_count(&app), 1, "the alert is settling");
    publish_word(&mut app, "t1", "working", 3);
    app.tick_notifications(now + std::time::Duration::from_secs(61));
    assert_eq!(notification_count(&app), 0);
}

#[tokio::test]
async fn queue_promotes_after_the_ttl() {
    let mut app = app_alerts();
    let now = Instant::now();
    // A third pane on another inactive tab.
    {
        let mut terms: Vec<_> = app.view.inventory.terminals().values().cloned().collect();
        let mut t3 = crate::sidebar::fixture::agent("t3", "working");
        t3.labels.insert("herdr.space".into(), "space-c".into());
        t3.labels.insert("herdr.tab".into(), "tab-c".into());
        terms.push(t3);
        let seq = app.view.inventory.seq() + 1;
        app.view.inventory.replace(seq, terms);
    }
    app.reconcile_published_view();
    publish_word(&mut app, "t1", "blocked", 1);
    publish_word(&mut app, "t3", "done", 1);
    assert_eq!(visible_title(&app).as_deref(), Some("t1 needs attention"));
    assert_eq!(app.notifications.queued.len(), 1);
    app.tick_notifications(now + std::time::Duration::from_secs(9));
    assert_eq!(visible_title(&app).as_deref(), Some("t3 finished"));
}

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
        app.view.inventory.replace(seq, terms);
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
        app.view.inventory.replace(seq, terms);
    }
    app.share_seen();
    assert!(!app.seen_inflight.iter().any(|(id, _)| id == "t1"));
}
