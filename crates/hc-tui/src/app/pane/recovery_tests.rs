use super::*;

#[tokio::test]
async fn fragmented_initial_paint_restores_modes_before_queued_input() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    pane.paste(b"literal");
    let mut history = vec![b'x'; 16 * 1024];
    history[0] = 0x18;
    raw_paint(&mut pane, &mut rx, &mut core, &history).await;
    assert!(matches!(&pane.state, State::Live(live) if live.await_paint));
    assert!(
        timeout(
            Duration::from_millis(40),
            read_kind(&mut core, CH, Kind::Data)
        )
        .await
        .is_err()
    );
    raw_paint(&mut pane, &mut rx, &mut core, b"\x1b[?2004h\x1b[?90").await;
    assert!(matches!(&pane.state, State::Live(live) if live.await_paint));
    raw_paint(&mut pane, &mut rx, &mut core, b"01l").await;
    assert_eq!(
        read_kind(&mut core, CH, Kind::Data).await.payload,
        b"\x1b[200~literal\x1b[201~"
    );
}

#[tokio::test]
async fn lost_inventory_discards_down_input_and_gone_refuses_more() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"ready").await;
    drop(core);
    while !matches!(pane.state, State::Down { .. }) {
        step(&mut pane, &mut rx).await;
    }
    pane.key(&key('x'));
    pane.paste(b"old program");
    assert!(pane.input_backlog().0 > 0);
    pane.on_view(&view_with(None, Some("holder_lost"), 80, 24));
    assert_eq!(pane.input_backlog().0, 0);
    pane.paste(b"still old");
    assert_eq!(pane.input_backlog().0, 0);
    assert!(pane.take_input_rejection().is_some());
}

#[tokio::test]
async fn disconnected_undispatched_tap_is_cancelled_as_a_pair() {
    let (mut pane, _, _) = rig();
    pane.state = State::Down {
        spec: spec(Role::Active),
        reason: "disconnected".into(),
        attempts: 1,
        retry_at: Instant::now() + Duration::from_secs(10),
    };
    let mut press = key('x');
    press.dispatch_id = 7;
    pane.key(&press);
    pane.paste(b"keep literal paste");
    pane.cancel_undispatched_key(&press);
    assert_eq!(pane.pending.len(), 1);
    assert!(matches!(pane.pending.front(), Some(Pending::Paste(_))));
    assert!(pane.take_input_rejection().is_some());
}

#[tokio::test]
async fn focus_loss_while_opening_is_delivered_after_handshake() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    pane.focused(false);
    let mut core = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    assert_eq!(read_kind(&mut core, CH, Kind::Focus).await.payload, [0]);
}

#[tokio::test]
async fn an_ambiguous_encoded_prefix_rejects_the_whole_input_batch() {
    let (mut pane, _, _) = rig();
    pane.state = State::Down {
        spec: spec(Role::Active),
        reason: "disconnected".into(),
        attempts: 1,
        retry_at: Instant::now(),
    };
    pane.input_encoded(b"n".to_vec());
    pane.key(&crate::app::input::TerminalKey::new(KeyInput {
        key: Key::Enter,
        mods: Mods::NONE,
        action: Action::Press,
    }));
    assert_eq!(pane.input_backlog().0, 2);
    pane.fire_retry(Instant::now());
    assert_eq!(pane.input_backlog().0, 0);
    assert!(pane.take_input_rejection().is_some());
}

#[tokio::test]
async fn reply_incarnation_change_rejects_input_before_inventory_catches_up() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut first = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut first, b"old").await;
    drop(first);
    while !matches!(pane.state, State::Down { .. }) {
        step(&mut pane, &mut rx).await;
    }
    pane.paste(b"old program input");
    pane.fire_retry(Instant::now() + Duration::from_secs(5));
    let (mut second, _) = accept.recv().await.unwrap();
    let mut replacement = testcore::minimal_terminal();
    replacement["pid"] = json!(999);
    second
        .serve_attach(testcore::attach_result(CH, replacement, true))
        .await;
    step_until_live(&mut pane, &mut rx).await;
    assert_eq!(pane.input_backlog().0, 0);
    assert!(pane.take_input_rejection().is_some());
}

#[tokio::test]
async fn resize_between_initial_fragments_is_applied_at_the_mode_barrier() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    raw_paint(&mut pane, &mut rx, &mut core, b"\x18initial fragment").await;
    core.send_stream(CH, Kind::Resize, &[40, 0, 10, 0]).await;
    while pane.vt_grid != (40, 10) {
        step(&mut pane, &mut rx).await;
    }
    raw_paint(&mut pane, &mut rx, &mut core, b"\x1b[?9001l").await;
    assert_eq!(pane.vt().unwrap().size().unwrap(), (40, 10));
}

#[tokio::test]
async fn live_focus_control_cannot_take_a_writer_slot_before_an_older_key() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"ready").await;
    while !matches!(&pane.state, State::Live(live) if live.out.capacity() == OUT_QUEUE) {
        step(&mut pane, &mut rx).await;
    }
    let State::Live(live) = &pane.state else {
        panic!("live")
    };
    for _ in 0..64 {
        live.out
            .try_send(Out::Input(b"x".to_vec()))
            .unwrap_or_else(|_| panic!("fixture queue"));
    }
    pane.key(&key('k'));
    assert_eq!(pane.input_backlog().0, 1);
    tokio::task::yield_now().await; // writer slots free; progress deliberately remains unread
    pane.focused(false);
    rx.close(); // keep writer progress from blocking the controlled wire proof
    let mut input = Vec::new();
    loop {
        let frame = timeout(T, core.read_stream(CH)).await.unwrap();
        if frame.kind == Kind::Data {
            input.extend_from_slice(&frame.payload);
        }
        if frame.kind == Kind::Focus && frame.payload == [0] {
            break;
        }
    }
    let mut expected = b"x".repeat(64);
    expected.push(b'k');
    assert_eq!(input, expected);
}
