use super::*;

fn mouse(action: hc_vt::MouseAction) -> hc_vt::MouseInput {
    hc_vt::MouseInput {
        action,
        button: Some(hc_vt::MouseButton::Left),
        mods: Mods::NONE,
        position: hc_vt::MousePosition::Cell { column: 1, row: 1 },
    }
}

#[tokio::test]
async fn mouse_taps_never_queue_in_down_or_survive_retry() {
    let (mut pane, mut events, _accepted) = rig();
    pane.want(Some(spec(Role::Active)));
    let original = pane.generation();
    assert!(pane.mouse(mouse(hc_vt::MouseAction::Press)));
    pane.on_event(
        PaneEv::Failed {
            generation: original,
            why: "offline".into(),
            unsupported: false,
        },
        Instant::now(),
    );
    assert!(!pane.mouse(mouse(hc_vt::MouseAction::Release)));
    assert!(!pane.mouse(mouse(hc_vt::MouseAction::Press)));
    pane.fire_retry(Instant::now() + Duration::from_secs(20));
    assert_ne!(pane.generation(), original);
    assert_eq!(pane.input_backlog().0, 0);
    assert!(pane.take_input_rejection().is_some());
    events.close();
}

#[tokio::test]
async fn observer_handshake_has_no_display_claim() {
    let (mut pane, _events, mut accepted) = rig();
    let facts = HostFacts {
        cell_px: Some((10, 20)),
        ..Default::default()
    };
    pane.set_host_facts(facts);
    pane.want(Some(spec(Role::Observe)));
    let (_core, params) = timeout(T, accepted.recv()).await.unwrap().unwrap();
    assert!(params.observe);
    assert!(params.display.is_none());
}

#[tokio::test]
async fn cancelling_shutdown_aborts_the_owned_drain_and_writer() {
    let (mut pane, _events, _accepted) = rig();
    let writer = tokio::spawn(std::future::pending::<Result<(), String>>());
    let writer_abort = writer.abort_handle();
    let reader = tokio::spawn(std::future::pending::<()>());
    let reader_abort = reader.abort_handle();
    let done = tokio::spawn(std::future::pending::<Result<(), String>>());
    let done_abort = done.abort_handle();
    pane.closing.push(Closing {
        route: 1,
        terminal: "t1".into(),
        done,
        writer_abort: writer_abort.clone(),
        reader_abort: reader_abort.clone(),
    });
    assert!(
        timeout(Duration::from_millis(5), pane.shutdown())
            .await
            .is_err()
    );
    timeout(T, async {
        while !writer_abort.is_finished()
            || !done_abort.is_finished()
            || !reader_abort.is_finished()
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(writer.await.unwrap_err().is_cancelled());
    assert!(reader.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn dropping_a_drain_before_its_first_poll_aborts_its_real_reader() {
    let (mut pane, mut events, mut accepted) = rig();
    pane.want(Some(spec(Role::Observe)));
    let mut _silent_peer = serve(&mut accepted, true).await;
    step_until_live(&mut pane, &mut events).await;
    paint(&mut pane, &mut events, &mut _silent_peer, b"ready").await;
    let State::Live(live) = &pane.state else {
        unreachable!()
    };
    let reader = live.reader.abort_handle();
    let writer = live.writer.abort_handle();
    let route = pane.park_outgoing().unwrap();
    pane.unpark(route);
    let drain = pane.closing.last().unwrap().done.abort_handle();
    // No await here: on this current-thread runtime the drain has not polled.
    drop(pane);
    timeout(T, async {
        while !reader.is_finished() || !writer.is_finished() || !drain.is_finished() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
