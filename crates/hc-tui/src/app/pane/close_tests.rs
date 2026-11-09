use super::*;

#[tokio::test]
async fn a_forwarded_press_replaces_the_same_code_on_an_older_parked_wire() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut original = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut original, b"\x1b[=31uA").await;
    let mut press = key('b');
    press.input.mods = Mods::CTRL;
    pane.key(&press);
    assert_eq!(
        read_kind(&mut original, CH, Kind::Data).await.payload,
        b"\x1b[98;5u"
    );
    let old = pane.park_outgoing().unwrap();
    pane.want(Some(Spec {
        terminal: "t8".into(),
        role: Role::Active,
    }));
    let mut replacement = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut replacement, b"\x1b[=31uB").await;
    assert!(pane.parked_held(old));
    no_kind(&mut original, CH, &[Kind::Data], 80).await; // Switching is not detach.
    press.input.mods = Mods::META;
    pane.key(&press); // Unencodable, but the source server replaces its old entry.
    assert!(!pane.parked_held(old));
    pane.unpark(old);
    let peer = tokio::spawn(async move {
        loop {
            let frame = original.read_stream(CH).await;
            assert_ne!(
                frame.kind,
                Kind::Data,
                "superseded press received a stale release"
            );
            if frame.kind == Kind::Close {
                original.send_stream(CH, Kind::Close, b"").await;
                break;
            }
        }
    });
    rx.close();
    let next = tokio::spawn(async move {
        read_kind(&mut replacement, CH, Kind::Close).await;
        replacement.send_stream(CH, Kind::Close, b"").await;
    });
    timeout(Duration::from_secs(15), pane.shutdown())
        .await
        .unwrap();
    peer.await.unwrap();
    next.await.unwrap();
    assert_eq!(pane.close_failure(), None);
}

#[tokio::test]
async fn shutdown_releases_delivered_keys_using_the_last_forwarded_chord() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"\x1b[=31uA").await;
    let chord = |ch, mods| {
        crate::app::input::TerminalKey::new(KeyInput {
            key: Key::Char(ch),
            mods,
            action: Action::Press,
        })
    };
    pane.key(&chord('b', Mods::CTRL));
    pane.key(&chord('c', Mods::CTRL));
    pane.key(&chord('c', Mods::META)); // Source tracks it despite no encoded bytes.
    rx.close();
    let peer = tokio::spawn(async move {
        let mut input = Vec::new();
        loop {
            let frame = core.read_stream(CH).await;
            match frame.kind {
                Kind::Data => input.extend_from_slice(&frame.payload),
                Kind::Close => {
                    core.send_stream(CH, Kind::Close, b"").await;
                    return input;
                }
                _ => {}
            }
        }
    });
    timeout(Duration::from_secs(15), pane.shutdown())
        .await
        .unwrap();
    assert_eq!(pane.close_failure(), None);
    assert_eq!(peer.await.unwrap(), b"\x1b[98;5u\x1b[99;5u\x1b[98;5:3u");
}

#[tokio::test]
async fn live_shutdown_drains_a_full_queue_and_release_before_close() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"\x1b[>3uA").await;
    for _ in 0..80 {
        pane.input_encoded(b"x".to_vec());
    }
    pane.key(&release('x'));
    rx.close(); // Production app stops consuming UI progress when it exits.
    let peer = tokio::spawn(async move {
        let mut input = Vec::new();
        loop {
            let frame = core.read_stream(CH).await;
            match frame.kind {
                Kind::Data => input.extend_from_slice(&frame.payload),
                Kind::Close => {
                    core.send_stream(CH, Kind::Close, b"").await;
                    return input;
                }
                _ => {}
            }
        }
    });
    timeout(Duration::from_secs(15), pane.shutdown())
        .await
        .unwrap();
    assert_eq!(pane.close_failure(), None);
    let mut expected = b"x".repeat(80);
    expected.extend_from_slice(b"\x1b[120;1:3u");
    assert_eq!(peer.await.unwrap(), expected);
}

#[tokio::test]
async fn repeated_stalled_closes_are_count_bounded_and_visible() {
    let (mut pane, mut rx, mut accept) = rig();
    let mut peers = Vec::new();
    for _ in 0..9 {
        pane.want(Some(spec(Role::Active)));
        let mut core = serve(&mut accept, true).await;
        timeout(T, async {
            while !matches!(pane.state, State::Live(_)) {
                step(&mut pane, &mut rx).await;
            }
        })
        .await
        .unwrap();
        paint(&mut pane, &mut rx, &mut core, b"ready").await;
        let route = pane.park_outgoing().unwrap();
        pane.want(None);
        pane.push_parked_pending(route, vec![b'x'; 2 * 1024 * 1024]);
        pane.unpark(route);
        peers.push(core); // Each owned fake peer deliberately stops reading.
    }
    assert!(pane.closing.len() <= 8);
    assert!(
        pane.notice()
            .is_some_and(|notice| notice.title == "Input drain failed")
    );
    assert_eq!(
        pane.close_failure(),
        None,
        "an evicted drain loses queued input; it never fails the client"
    );
    rx.close();
    timeout(Duration::from_secs(15), pane.shutdown())
        .await
        .unwrap();
    assert!(pane.closing.is_empty());
    drop(peers);
}

#[tokio::test]
async fn many_parked_routes_at_detach_evict_a_drain_without_failing() {
    // Held keys pin every visited wire, so detach starts one drain per
    // parked route back to back: the ninth exceeds the close budget and
    // the oldest is dropped. That is route-local loss, not a failure —
    // the client exits clean (herdr revokes and recovers the same way).
    let (mut pane, mut rx, mut accept) = rig();
    let mut peers = Vec::new();
    for _ in 0..9 {
        pane.want(Some(spec(Role::Active)));
        let mut core = serve(&mut accept, true).await;
        timeout(T, async {
            while !matches!(pane.state, State::Live(_)) {
                step(&mut pane, &mut rx).await;
            }
        })
        .await
        .unwrap();
        paint(&mut pane, &mut rx, &mut core, b"ready").await;
        pane.park_outgoing().unwrap();
        peers.push(core);
    }
    rx.close();
    for mut core in peers {
        tokio::spawn(async move {
            loop {
                let frame = core.read_stream(CH).await;
                if frame.kind == Kind::Close {
                    core.send_stream(CH, Kind::Close, b"").await;
                    break;
                }
            }
        });
    }
    timeout(Duration::from_secs(15), pane.shutdown())
        .await
        .unwrap();
    assert!(pane.closing.is_empty());
    assert_eq!(pane.close_failure(), None);
}

#[tokio::test]
async fn retained_frame_disconnect_and_size_control_notice_clear_on_recovery() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"retained output").await;
    core.send_stream(CH, Kind::Resize, &[40, 0, 10, 0]).await;
    timeout(T, async {
        while !pane.notice().is_some_and(|notice| notice.take_control) {
            step(&mut pane, &mut rx).await;
        }
    })
    .await
    .unwrap();
    pane.take_control();
    let resize = read_kind(&mut core, CH, Kind::Resize).await;
    assert_eq!(resize.payload, [80, 0, 24, 0]);
    core.send_stream(CH, Kind::Resize, &resize.payload).await;
    timeout(T, async {
        while pane.notice().is_some() {
            step(&mut pane, &mut rx).await;
        }
    })
    .await
    .unwrap();
    drop(core);
    timeout(T, async {
        while !matches!(pane.state, State::Down { .. }) {
            step(&mut pane, &mut rx).await;
        }
    })
    .await
    .unwrap();
    assert!(vt_text(&mut pane).contains("retained output"));
    assert_eq!(pane.notice().unwrap().title, "Terminal disconnected");
    pane.fire_retry(Instant::now() + Duration::from_secs(10));
    let mut core = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"fresh output").await;
    assert!(pane.notice().is_none());
}

#[tokio::test]
async fn observer_keys_are_rejected_and_never_replayed_after_role_change() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Observe)));
    let mut core = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"observer").await;
    pane.key(&key('x'));
    assert!(pane.take_input_rejection().is_some());
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"active").await;
    no_kind(&mut core, CH, &[Kind::Data], 80).await;
}

#[tokio::test]
async fn switch_drains_original_pending_input_and_parked_automatic_reply() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"ready").await;
    // Owned fake transport only: force its wire and writer queue full.
    if let State::Live(live) = &pane.state {
        live.out
            .try_send(Out::Input(vec![b'z'; 2 * 1024 * 1024]))
            .ok()
            .unwrap();
    }
    tokio::time::sleep(Duration::from_millis(30)).await;
    for _ in 0..OUT_QUEUE {
        pane.input_encoded(b"x".to_vec());
    }
    pane.input_encoded(b"tail".to_vec());
    assert!(pane.input_backlog().0 > 0);
    let route = pane.park_outgoing().unwrap();
    pane.want(None);
    core.send_stream(CH, Kind::Data, b"\x1b[c").await;
    timeout(T, async {
        while pane.parked[0].pending_reply.is_empty() {
            step(&mut pane, &mut rx).await;
        }
    })
    .await
    .unwrap();
    pane.unpark(route);
    rx.close();
    let peer = tokio::spawn(async move {
        let mut data = Vec::new();
        let mut replies = Vec::new();
        loop {
            let frame = core.read_stream(CH).await;
            match frame.kind {
                Kind::Data => data.extend_from_slice(&frame.payload),
                Kind::AutomaticInput => replies.extend_from_slice(&frame.payload),
                Kind::Close => {
                    core.send_stream(CH, Kind::Close, b"").await;
                    return (data, replies);
                }
                _ => {}
            }
        }
    });
    timeout(Duration::from_secs(15), pane.shutdown())
        .await
        .unwrap();
    assert_eq!(pane.close_failure(), None);
    let (data, replies) = peer.await.unwrap();
    let mut expected = vec![b'z'; 2 * 1024 * 1024];
    expected.extend(vec![b'x'; OUT_QUEUE]);
    expected.extend_from_slice(b"tail");
    assert_eq!(data, expected);
    assert!(replies.starts_with(b"\x1b[?") && replies.ends_with(b"c"));
}

#[tokio::test]
async fn a_retired_wires_deferred_drain_error_never_reaches_the_new_target() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"ready").await;
    let route = pane.park_outgoing().unwrap();
    pane.unpark(route);
    assert!(pane.closing.iter().any(|c| c.route == route));
    // The t7 drain is still in flight while the pane retargets to t9.
    pane.want(Some(Spec {
        terminal: "t9".into(),
        role: Role::Active,
    }));
    let mut replacement = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut replacement, b"ready").await;
    pane.on_event(
        PaneEv::RouteClosed {
            route,
            error: Some("the attach stream ended".into()),
        },
        Instant::now(),
    );
    assert_eq!(
        pane.close_failure(),
        None,
        "a dead wire's drain failure must not surface on the new target"
    );
    // The same deferred failure on a same-terminal reincarnation does.
    let live_route = pane.park_outgoing().unwrap();
    pane.unpark(live_route);
    pane.want(Some(Spec {
        terminal: "t9".into(),
        role: Role::Active,
    }));
    let reincarnated = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    pane.on_event(
        PaneEv::RouteClosed {
            route: live_route,
            error: Some("the attach stream ended".into()),
        },
        Instant::now(),
    );
    assert_eq!(pane.notice().unwrap().title, "Input drain failed");
    rx.close();
    drop(core);
    drop(replacement);
    drop(reincarnated);
    pane.shutdown().await;
}
