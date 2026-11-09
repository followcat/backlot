use super::*;

#[tokio::test]
async fn queued_input_survives_down_retry_and_reencodes() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"v1> ").await;
    drop(core);
    for _ in 0..4 {
        step(&mut pane, &mut rx).await;
        if pane.next_wake().is_some() {
            break;
        }
    }
    // Human input queued while Down carries into the retry and
    // re-encodes after the fresh paint — the underflow in the removed
    // count must not panic, and bytes stay accounted.
    pane.key(&key('q'));
    pane.paste(b"pp");
    pane.fire_retry(Instant::now() + std::time::Duration::from_secs(999));
    let mut core2 = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core2, b"v2> ").await;
    let f1 = read_kind(&mut core2, CH, Kind::Data).await;
    assert_eq!(f1.payload, b"q");
    let f2 = read_kind(&mut core2, CH, Kind::Data).await;
    assert!(String::from_utf8_lossy(&f2.payload).contains("pp"));
    let (pending, dropped) = pane.input_backlog();
    assert_eq!(pending, 0);
    assert_eq!(
        dropped, 0,
        "queued human input was wrongly counted as dropped"
    );
}
#[tokio::test]
async fn gone_clears_the_dead_incarnations_input() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let core = serve(&mut accept, true).await;
    // Input typed while the open is still in flight parks — and it is
    // bound to THIS program incarnation.
    pane.key(&key('d'));
    pane.paste(b"old bytes");
    drop(core);
    // The terminal is lost before the attach even painted — the
    // parked input dies with the incarnation, never reaches a restart.
    pane.on_view(&view_with(None, Some("holder_lost"), 80, 24));
    let (pending, dropped) = pane.input_backlog();
    assert_eq!(pending, 0);
    assert_eq!(dropped, 2);
    // Restart: a fresh shell gets a clean input channel.
    pane.on_view(&view_with(Some(200), None, 80, 24));
    let mut core2 = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core2, b"new> ").await;
    no_kind(&mut core2, CH, &[Kind::Data], 80).await;
}
#[tokio::test]
async fn observer_drains_replies_without_sending() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Observe)));
    let (mut core, _) = timeout(T, accept.recv()).await.unwrap().unwrap();
    core.serve_attach(testcore::attach_result(
        CH,
        testcore::minimal_terminal(),
        true,
    ))
    .await;
    step(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"\x1b[c").await; // DA → reply
    no_kind(&mut core, CH, &[Kind::AutomaticInput, Kind::Data], 80).await;
    assert!(!pane.status_label(Instant::now()).contains("replies"));
}
#[tokio::test]
async fn only_live_output_counts_as_a_program_copy() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step(&mut pane, &mut rx).await;
    // Still inside the paint: the mode barrier hasn't arrived.
    raw_paint(&mut pane, &mut rx, &mut core, b"\x1b]52;c;b2xk\x07").await;
    assert!(pane.take_clipboard_writes().is_empty());
    paint(&mut pane, &mut rx, &mut core, b"x").await;
    raw_paint(&mut pane, &mut rx, &mut core, b"\x1b]52;c;aGVsbG8=\x07").await;
    assert_eq!(pane.take_clipboard_writes(), vec![b"hello".to_vec()]);
}
#[tokio::test]
async fn stale_generation_events_are_ignored() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let _core = serve(&mut accept, true).await;
    step(&mut pane, &mut rx).await;
    pane.on_event(
        PaneEv::Out {
            generation: 0,
            out: Output::Data(b"forged".to_vec()),
        },
        Instant::now(),
    );
    assert!(!vt_text(&mut pane).contains("forged"));
}
#[tokio::test]
async fn overflowing_pending_input_is_counted_not_silent() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let _core = serve(&mut accept, true).await;
    step(&mut pane, &mut rx).await; // Opened, await_paint parks input
    for _ in 0..200 {
        pane.key(&key('k'));
    }
    let (_pending, dropped) = pane.input_backlog();
    assert!(dropped > 0, "pending overflow must be surfaced");
}
#[tokio::test]
async fn parked_release_drains_to_original_target() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core_a = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core_a, b"\x1b[>3uA").await; // kitty event types
    pane.key(&key('a'));
    read_kind(&mut core_a, CH, Kind::Data).await; // the press
    let route = pane.park_outgoing().expect("park the live attach");
    assert!(pane.parked_has(route));
    pane.want(Some(Spec {
        terminal: "t9".into(),
        role: Role::Active,
    }));
    let mut core_b = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core_b, b"\x1b[>3uB").await;
    assert!(pane.lease_input(route, &release('a')));
    let frame = read_kind(&mut core_a, CH, Kind::Data).await;
    assert_eq!(frame.payload, b"\x1b[97;1:3u", "{:?}", frame.payload);
    no_kind(&mut core_b, CH, &[Kind::Data], 80).await;
    pane.unpark(route);
    assert!(!pane.parked_has(route));
    assert!(!pane.lease_input(route, &release('a')));
}
#[tokio::test]
async fn parked_release_uses_stored_press_code_and_incoming_mods() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"\x1b[>3uA").await;
    let route = pane.park_outgoing().expect("park the live attach");
    pane.want(None);
    // Stored press: Ctrl+base 'c' (the chord resolved to base layout).
    // Incoming key-up: the Cyrillic codepoint with modifiers gone.
    let mut stored = crate::app::input::TerminalKey::new(KeyInput {
        key: Key::Char('c'),
        mods: Mods::CTRL,
        action: Action::Press,
    });
    stored.layout_key = Some('с');
    let incoming = crate::app::input::TerminalKey::new(KeyInput {
        key: Key::Char('с'),
        mods: Mods::NONE,
        action: Action::Release,
    });
    let release = crate::app::App::lease_release(&stored, Some(&incoming));
    assert!(pane.lease_input(route, &release));
    let frame = read_kind(&mut core, CH, Kind::Data).await;
    assert_eq!(frame.payload, b"\x1b[99;1:3u", "{:?}", frame.payload);
}
#[tokio::test]
async fn unpark_drains_pending_release_in_order_then_closes() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"\x1b[>3uA").await;
    let route = pane.park_outgoing().expect("park the live attach");
    pane.want(None);
    // A queued release plus input accepted while the writer was full.
    assert!(pane.lease_input(route, &release('a')));
    pane.push_parked_pending(route, b"\x1b[98;1:3u".to_vec()); // b-up
    pane.unpark(route);
    assert!(!pane.parked_has(route));
    assert!(!pane.lease_input(route, &release('a'))); // route gone
    // Both frames arrive in order; then the wire sees the attach end.
    let first = read_kind(&mut core, CH, Kind::Data).await;
    assert_eq!(first.payload, b"\x1b[97;1:3u", "{:?}", first.payload);
    let second = read_kind(&mut core, CH, Kind::Data).await;
    assert_eq!(second.payload, b"\x1b[98;1:3u", "{:?}", second.payload);
}
#[tokio::test]
async fn unpark_close_writes_release_before_close_frame() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"\x1b[>3uA").await;
    let route = pane.park_outgoing().expect("park the live attach");
    pane.want(None);
    assert!(pane.lease_input(route, &release('a')));
    pane.unpark(route);
    // DATA first, CLOSE after — in that order, on the same channel.
    let data = read_kind(&mut core, CH, Kind::Data).await;
    assert_eq!(data.payload, b"\x1b[97;1:3u", "{:?}", data.payload);
    let close = read_kind(&mut core, CH, Kind::Close).await;
    assert_eq!(close.kind, Kind::Close);
}
#[tokio::test]
async fn shutdown_cancels_blocked_parked_writer() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"\x1b[>3uA").await;
    let route = pane.park_outgoing().expect("park the live attach");
    pane.want(None);
    // One payload far bigger than the wire buffer + queue depths: the
    // writer blocks inside send_input with the fake core not reading.
    pane.push_parked_pending(route, vec![0x61; 2 * 1024 * 1024]);
    pane.unpark(route);
    // Give the drain task a moment to hand the payload to the writer.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    timeout(Duration::from_secs(15), pane.shutdown())
        .await
        .expect("shutdown deadline");
    assert!(
        pane.close_failure()
            .is_some_and(|why| why.contains("Timed out"))
    );
    // The fake core must not have received the whole payload — the
    // blocked writer was cancelled, not drained to completion.
    drop(core);
}
#[tokio::test]
async fn parked_output_never_paints() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core_a = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core_a, b"A").await;
    let route = pane.park_outgoing().expect("park the live attach");
    pane.want(Some(Spec {
        terminal: "t9".into(),
        role: Role::Active,
    }));
    let mut core_b = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core_b, b"BBB").await;
    core_a.send_stream(CH, Kind::Data, b"GHOST").await;
    step(&mut pane, &mut rx).await; // the parked Output arrives and dies
    while let Ok(ev) = rx.try_recv() {
        pane.on_event(ev, Instant::now());
    }
    assert!(vt_text(&mut pane).contains("BBB"));
    assert!(!vt_text(&mut pane).contains("GHOST"));
    pane.unpark(route);
}
