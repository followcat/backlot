//! Visible size holders have no terminal-input or focus authority.
use super::*;

#[tokio::test]
async fn size_holder_resizes_without_input_focus_emulator_or_replies() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.allocated(44, 37);
    pane.want(Some(spec(Role::SizeLease)));
    let (mut core, params) = timeout(T, accept.recv()).await.unwrap().unwrap();
    assert!(!params.observe);
    assert_eq!((params.cols, params.rows), (44, 37));
    core.serve_attach(testcore::attach_result(
        CH,
        testcore::minimal_terminal(),
        true,
    ))
    .await;
    step_until_live(&mut pane, &mut rx).await;
    let resize = resize_only(&mut core).await;
    assert_eq!(resize.payload.as_slice(), &[44, 0, 37, 0]);
    assert!(!pane.key(&key('x')));
    pane.paste(b"unwanted");
    pane.focused(true);
    raw_paint(&mut pane, &mut rx, &mut core, b"\x1b[6n\x1b[c\x1b[?9001l").await;
    assert!(pane.vt().is_none());
    pane.allocated(84, 47);
    resize_only(&mut core).await;
    no_kind(
        &mut core,
        CH,
        &[Kind::Data, Kind::AutomaticInput, Kind::Focus],
        50,
    )
    .await;
}

async fn resize_only(core: &mut FakeCore) -> hc_proto::frame::Frame {
    timeout(T, async {
        loop {
            let frame = core.read_stream(CH).await;
            assert!(
                ![Kind::Data, Kind::AutomaticInput, Kind::Focus].contains(&frame.kind),
                "forbidden size-holder frame: {frame:?}"
            );
            if frame.kind == Kind::Resize {
                return frame;
            }
        }
    })
    .await
    .expect("resize deadline")
}

#[tokio::test]
async fn allocation_changed_while_opening_is_sent_before_focus_even_when_reply_matches() {
    for role in [Role::Active, Role::SizeLease] {
        let (mut pane, mut rx, mut accept) = rig();
        pane.want(Some(spec(role)));
        pane.allocated(38, 23);
        let (mut core, params) = timeout(T, accept.recv()).await.unwrap().unwrap();
        assert_eq!((params.cols, params.rows), (80, 24));
        let mut terminal = testcore::minimal_terminal();
        terminal["cols"] = json!(38);
        terminal["rows"] = json!(23);
        core.serve_attach(testcore::attach_result(CH, terminal, true))
            .await;
        step_until_live(&mut pane, &mut rx).await;
        let resize = resize_only(&mut core).await;
        assert_eq!(resize.payload.as_slice(), &[38, 0, 23, 0]);
        if role == Role::Active {
            read_kind(&mut core, CH, Kind::Focus).await;
        } else {
            no_kind(
                &mut core,
                CH,
                &[Kind::Focus, Kind::Data, Kind::AutomaticInput],
                30,
            )
            .await;
        }
    }
}

#[tokio::test]
async fn visible_size_holder_recovery_is_passive_until_a_real_allocation_change() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::SizeLease)));
    let _first = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    pane.on_event(
        PaneEv::Ended {
            generation: pane.generation(),
        },
        Instant::now(),
    );
    pane.fire_retry(Instant::now() + std::time::Duration::from_secs(1));
    let (mut next, params) = timeout(T, accept.recv()).await.unwrap().unwrap();
    assert!(!params.observe);
    assert!(params.resumed);
    assert_eq!(pane.spec().unwrap().role, Role::SizeLease);
    let mut terminal = testcore::minimal_terminal();
    terminal["cols"] = json!(40);
    terminal["rows"] = json!(20);
    next.serve_attach(testcore::attach_result(CH, terminal, true))
        .await;
    step_until_live(&mut pane, &mut rx).await;
    no_kind(
        &mut next,
        CH,
        &[Kind::Resize, Kind::Focus, Kind::Data, Kind::AutomaticInput],
        30,
    )
    .await;
    pane.allocated(100, 30); // Real allocation change may reclaim the size.
    assert_eq!(
        resize_only(&mut next).await.payload.as_slice(),
        &[100, 0, 30, 0]
    );
    pane.want(None);
    pane.want(Some(spec(Role::SizeLease))); // Revisiting the tab is new activity.
    let (_entered, params) = timeout(T, accept.recv()).await.unwrap().unwrap();
    assert!(!params.resumed);
    assert_eq!((params.cols, params.rows), (100, 30));
}

#[tokio::test]
async fn live_allocation_syncs_registered_size_even_when_another_viewer_already_matches() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"ready").await;
    core.send_stream(CH, Kind::Resize, &[38, 0, 23, 0]).await;
    timeout(T, async {
        while !pane.has_size_mismatch() {
            step(&mut pane, &mut rx).await;
        }
    })
    .await
    .unwrap();
    assert!(pane.notice().is_some_and(|notice| notice.take_control));
    pane.allocated(38, 23);
    let resize = read_kind(&mut core, CH, Kind::Resize).await;
    assert_eq!(resize.payload.as_slice(), &[38, 0, 23, 0]);
    // Core need not send a redundant RESIZE when its global grid is unchanged.
    assert!(!pane.has_size_mismatch());
    assert!(pane.notice().is_none());
    assert!(
        !pane
            .status_label(Instant::now())
            .contains("owned by another viewer")
    );
}
