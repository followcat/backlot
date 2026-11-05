use super::*;

async fn serve_quiet(accept: &mut mpsc::Receiver<(FakeCore, AttachParams)>) -> FakeCore {
    let (mut core, params) = timeout(T, accept.recv()).await.unwrap().unwrap();
    assert!(params.quiet_human_release);
    core.read_hello().await;
    core.answer_hello(true, testcore::METHODS, &["quiet_human_release"])
        .await;
    let request = core.read_request().await;
    assert_eq!(request["params"]["quiet_human_release"], true);
    let mut result = testcore::attach_result(CH, testcore::minimal_terminal(), false);
    result["quiet_human_release"] = json!(true);
    core.reply(&request["id"], result).await;
    core
}

#[tokio::test]
async fn live_releases_are_quiet_but_release_shaped_ordinary_input_is_data() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve_quiet(&mut accept).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"\x1b[=31uA").await;
    let mut press = key('b');
    press.input.mods = Mods::CTRL;
    pane.key(&press);
    assert_eq!(
        read_kind(&mut core, CH, Kind::Data).await.payload,
        b"\x1b[98;5u"
    );
    pane.key(&press.with_action(Action::Release));
    assert_eq!(
        read_kind(&mut core, CH, Kind::QuietHumanRelease)
            .await
            .payload,
        b"\x1b[98;5:3u"
    );
    pane.input_encoded(b"\x1b[98;5:3u".to_vec());
    assert_eq!(
        read_kind(&mut core, CH, Kind::Data).await.payload,
        b"\x1b[98;5:3u"
    );
}

#[tokio::test]
async fn parked_release_stays_on_its_negotiated_wire() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut original = serve_quiet(&mut accept).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut original, b"\x1b[=31uA").await;
    let mut press = key('b');
    press.input.mods = Mods::CTRL;
    pane.key(&press);
    read_kind(&mut original, CH, Kind::Data).await;
    let route = pane.park_outgoing().unwrap();
    pane.want(None);
    assert!(pane.lease_input(route, &press.with_action(Action::Release)));
    let frame = read_kind(&mut original, CH, Kind::QuietHumanRelease).await;
    assert_eq!(frame.payload, b"\x1b[98;5:3u");
}

#[tokio::test]
async fn shutdown_drains_synthetic_quiet_releases_before_close() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve_quiet(&mut accept).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"\x1b[=31uA").await;
    let mut press = key('b');
    press.input.mods = Mods::CTRL;
    pane.key(&press);
    read_kind(&mut core, CH, Kind::Data).await;
    rx.close();
    let peer = tokio::spawn(async move {
        let frame = read_kind(&mut core, CH, Kind::QuietHumanRelease).await;
        assert_eq!(frame.payload, b"\x1b[98;5:3u");
        read_kind(&mut core, CH, Kind::Close).await;
        core.send_stream(CH, Kind::Close, b"").await;
    });
    timeout(T, pane.shutdown()).await.unwrap();
    timeout(T, peer).await.unwrap().unwrap();
    assert_eq!(pane.close_failure(), None);
}

#[test]
fn source_tags_survive_frozen_key_and_mouse_retries() {
    let mouse = hc_vt::MouseInput {
        action: hc_vt::MouseAction::Release,
        button: Some(hc_vt::MouseButton::Left),
        mods: Mods::NONE,
        position: hc_vt::MousePosition::Cell { column: 1, row: 1 },
    };
    assert!(matches!(
        Out::human(&Pending::Mouse(1, mouse), vec![b'x']),
        Out::Release(_)
    ));
    let key = release('a');
    assert!(matches!(
        Out::human(&Pending::Prepared(vec![], key), vec![b'x']),
        Out::Release(_)
    ));
    assert!(matches!(
        Out::human(&Pending::Encoded(vec![], None, true), vec![b'x']),
        Out::Release(_)
    ));
    assert!(matches!(
        Out::human(
            &Pending::Encoded(vec![], None, false),
            b"\x1b[97;1:3u".to_vec()
        ),
        Out::Input(_)
    ));
}
