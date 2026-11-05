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
