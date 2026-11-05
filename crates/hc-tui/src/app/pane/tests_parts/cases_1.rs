use super::*;

#[tokio::test]
async fn first_paint_builds_vt_and_credits() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step(&mut pane, &mut rx).await; // Opened
    paint(&mut pane, &mut rx, &mut core, b"prompt$ ").await;
    assert!(vt_text(&mut pane).contains("prompt$ "));
    // Credit flows only after the VT consumed the bytes.
    let credit = read_kind(&mut core, CH, Kind::Credit).await;
    assert_eq!(credit.kind, Kind::Credit);
}
#[tokio::test]
async fn input_waits_for_fresh_paint_then_encodes() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"old paint").await;
    drop(core); // die — the retained screen must survive
    for _ in 0..4 {
        step(&mut pane, &mut rx).await;
        if pane.next_wake().is_some() {
            break;
        }
    }
    assert!(vt_text(&mut pane).contains("old paint"));
    pane.fire_retry(Instant::now() + std::time::Duration::from_secs(999));
    let mut core2 = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await; // Opened — await_paint
    // Queued human input waits: a key plus a paste.
    pane.key(&key('x'));
    pane.paste(b"pasted");
    no_kind(&mut core2, CH, &[Kind::Data], 80).await;
    paint(&mut pane, &mut rx, &mut core2, b"fresh> ").await;
    // Both flush once, in order, under the fresh VT's modes.
    let f1 = read_kind(&mut core2, CH, Kind::Data).await;
    assert_eq!(f1.payload, b"x");
    let f2 = read_kind(&mut core2, CH, Kind::Data).await;
    assert!(String::from_utf8_lossy(&f2.payload).contains("pasted"));
}
#[tokio::test]
async fn pending_input_never_crosses_terminals() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let _core_a = serve(&mut accept, true).await;
    step(&mut pane, &mut rx).await; // Opened
    pane.key(&key('a')); // parked for A's incarnation
    pane.want(Some(Spec {
        terminal: "t9".into(),
        role: Role::Active,
    }));
    let (mut core_b, _) = timeout(T, accept.recv()).await.unwrap().unwrap();
    core_b
        .serve_attach(testcore::attach_result(
            CH,
            {
                let mut t = testcore::minimal_terminal();
                t["address"] = json!("t9");
                t
            },
            true,
        ))
        .await;
    step(&mut pane, &mut rx).await; // Opened
    paint(&mut pane, &mut rx, &mut core_b, b"# ").await;
    // B's wire must never see A's parked 'a'.
    no_kind(&mut core_b, CH, &[Kind::Data], 80).await;
}
#[tokio::test]
async fn retained_screen_survives_down() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"keep me").await;
    drop(core); // wire EOF
    step(&mut pane, &mut rx).await; // Ended
    assert!(pane.status_label(Instant::now()).contains("ended"));
    assert!(vt_text(&mut pane).contains("keep me"));
    // The re-opening attempt still shows the last paint.
    pane.fire_retry(Instant::now() + std::time::Duration::from_secs(999));
    assert!(vt_text(&mut pane).contains("keep me"));
}
#[tokio::test]
async fn failed_open_retries_with_resumed_and_carries_attempts() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    async fn fail(core: &mut FakeCore) {
        core.serve_hello().await;
        let req = core.read_request().await;
        core.send(&hc_proto::frame::Frame::data(
            0,
            serde_json::to_vec(&json!({"jsonrpc":"2.0","id":req["id"],"error":{"code":-32000,"message":"closed","data":{"kind":"closed","hint":"gone"}}})).unwrap(),
        ).encode()).await;
    }
    // Failure one: the first backoff slot.
    let (mut core, p1) = timeout(T, accept.recv()).await.unwrap().unwrap();
    assert!(!p1.resumed);
    fail(&mut core).await;
    let now1 = Instant::now();
    step(&mut pane, &mut rx).await; // Failed → Down
    let delay1 = pane.next_wake().unwrap() - now1;
    pane.fire_retry(Instant::now() + std::time::Duration::from_secs(999));
    // Failure two: the retry carries the attempt count — backoff grows.
    let (mut core2, p2) = timeout(T, accept.recv()).await.unwrap().unwrap();
    assert!(p2.resumed, "wire recovery must not stamp fresh ownership");
    fail(&mut core2).await;
    let now2 = Instant::now();
    step(&mut pane, &mut rx).await; // Failed → Down again
    let delay2 = pane.next_wake().unwrap() - now2;
    assert!(
        delay2 > delay1,
        "backoff must grow: {delay1:?} → {delay2:?}"
    );
    assert!(delay2 <= std::time::Duration::from_secs(8));
    assert_eq!(pane.attaches, 2);
}
#[tokio::test]
async fn unsupported_attach_disables_visibly_without_retry() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    // hello without terminal.attach → the client open itself reports
    // CallError::Unsupported before the attach call ever reaches us.
    let (mut core, _p) = timeout(T, accept.recv()).await.unwrap().unwrap();
    core.read_hello().await;
    core.answer_hello(true, &["hello", "terminal.list"], &[])
        .await;
    step(&mut pane, &mut rx).await; // Failed(unsupported) → Gone
    assert!(pane.next_wake().is_none(), "unsupported must not retry");
    assert!(pane.status_label(Instant::now()).contains("unsupported"));
    // Same-generation view publishes keep republishing a live row —
    // the feature stays disabled; nothing reattaches.
    pane.on_view(&live_view(80, 24));
    pane.on_view(&live_view(80, 24));
    assert!(
        timeout(std::time::Duration::from_millis(80), accept.recv())
            .await
            .is_err()
    );
    assert_eq!(pane.attaches, 1);
}
#[tokio::test]
async fn resumed_open_never_claims_and_flags_mismatch() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.allocated(100, 30);
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await; // grid 80x24 ≠ 100x30
    step(&mut pane, &mut rx).await; // user-initiated: claims 100x30
    let r = read_kind(&mut core, CH, Kind::Resize).await;
    assert_eq!(r.kind, Kind::Resize);
    drop(core);
    // Queued WriterProgress from the resize send may arrive first;
    // step until the wire's end parks the pane Down.
    for _ in 0..4 {
        step(&mut pane, &mut rx).await;
        if pane.next_wake().is_some() {
            break;
        }
    }
    pane.fire_retry(Instant::now() + std::time::Duration::from_secs(999));
    let mut core2 = serve(&mut accept, true).await; // resumed
    step_until_live(&mut pane, &mut rx).await; // Opened (+stale reports)
    // No synthetic RESIZE may appear on the resumed wire.
    no_kind(&mut core2, CH, &[Kind::Resize], 80).await;
    assert!(
        pane.status_label(Instant::now())
            .contains("owned by another viewer")
    );
}
#[tokio::test]
async fn closed_frame_parks_with_sanitized_reason() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step(&mut pane, &mut rx).await;
    core.send_stream(CH, Kind::Close, br#"{"reason":"detached\nby\tyou"}"#)
        .await;
    while !matches!(pane.state, State::Down { .. }) {
        step(&mut pane, &mut rx).await;
    }
    let label = pane.status_label(Instant::now());
    assert!(label.contains("closed:"), "{label}");
    assert!(!label.contains('\n') && !label.contains('\t'), "{label}");
}
#[tokio::test]
async fn lost_parks_gone_and_restart_reattaches_resumed() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let core = serve(&mut accept, true).await;
    step(&mut pane, &mut rx).await;
    drop(core);
    step(&mut pane, &mut rx).await; // Ended → Down
    // A real `lost`: pid absent plus a holder reason.
    pane.on_view(&view_with(None, Some("holder_lost"), 80, 24));
    assert!(pane.next_wake().is_none());
    assert!(pane.status_label(Instant::now()).contains("lost"));
    // A restart (fresh pid): recovery is automatic — resumed, never a
    // fresh ownership claim.
    pane.on_view(&view_with(Some(200), None, 80, 24));
    let (_core2, p2) = timeout(T, accept.recv()).await.unwrap().unwrap();
    assert!(p2.resumed);
}
#[tokio::test]
async fn observer_sends_nothing_and_tracks_stream_resize() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Observe)));
    let (mut core, params) = timeout(T, accept.recv()).await.unwrap().unwrap();
    assert!(params.observe);
    core.serve_attach(testcore::attach_result(
        CH,
        testcore::minimal_terminal(),
        true,
    ))
    .await;
    step(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"obs").await;
    pane.key(&key('x'));
    no_kind(&mut core, CH, &[Kind::Data, Kind::Resize, Kind::Focus], 80).await;
    core.send_stream(CH, Kind::Resize, &[60, 0, 20, 0]).await; // LE u16s
    step(&mut pane, &mut rx).await;
    assert_eq!(pane.vt().unwrap().frame(0).unwrap().cols, 60);
}
#[tokio::test]
async fn legacy_inventory_resize_applies_and_negotiated_ignores() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, false).await; // no negotiated RESIZE
    step(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"x").await;
    pane.on_view(&live_view(50, 10));
    assert_eq!(pane.vt().unwrap().frame(0).unwrap().cols, 50);
    // A negotiated path takes grid news only from the stream.
    let (mut pane2, mut rx2, mut accept2) = rig();
    pane2.want(Some(spec(Role::Active)));
    let mut core2 = serve(&mut accept2, true).await;
    step(&mut pane2, &mut rx2).await;
    paint(&mut pane2, &mut rx2, &mut core2, b"x").await;
    pane2.on_view(&live_view(50, 10));
    assert_eq!(pane2.vt().unwrap().frame(0).unwrap().cols, 80);
}
#[tokio::test]
async fn automatic_reply_goes_out_as_reply_not_input() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step(&mut pane, &mut rx).await;
    // Primary DA: the VT must answer — on the reply kind, never as
    // human input.
    paint(&mut pane, &mut rx, &mut core, b"\x1b[c").await;
    read_kind(&mut core, CH, Kind::AutomaticInput).await;
}
#[tokio::test]
async fn writer_progress_survives_a_full_event_channel() {
    let (ev_tx, mut ev_rx) = mpsc::channel(4);
    let (acc_tx, mut acc_rx) = mpsc::channel(16);
    let open: OpenAttach = Arc::new(move |params: AttachParams| {
        let acc = acc_tx.clone();
        Box::pin(async move {
            let (core, read, write) = testcore::pair(64 * 1024);
            let p2 = params.clone();
            acc.send((core, params)).await.expect("accept");
            Attach::open(read, write, None, &p2).await
        })
    });
    let mut pane = Pane::new(ev_tx.clone(), open);
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut acc_rx, true).await;
    step(&mut pane, &mut ev_rx).await; // Opened
    paint(&mut pane, &mut ev_rx, &mut core, b"x").await;
    // Fill the app event channel so a try_send progress report would
    // be lost; the writer's next send must still wake the pane.
    while ev_tx
        .try_send(PaneEv::WriterProgress { generation: 999 })
        .is_ok()
    {}
    pane.key(&key('z'));
    core.read_stream(CH).await; // the input frame itself lands
    // Draining reveals the queued WriterProgress for generation 1 —
    // try_send would have dropped it and parked nothing else.
    let mut saw = false;
    while let Ok(ev) = timeout(std::time::Duration::from_secs(2), ev_rx.recv()).await {
        if let Some(PaneEv::WriterProgress { generation: 1 }) = ev {
            saw = true;
            break;
        }
        if ev.is_none() {
            break;
        }
    }
    assert!(saw, "writer progress must survive a full event channel");
}
