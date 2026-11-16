//! A spec change on the *same* terminal — a role switch — is still a
//! spec change: human input queued for the old wire's role dies with
//! it. It can never sit in `pending` while observing, nor flush as
//! phantom input on a later attach, and the retained screen survives
//! the whole time.
use super::*;

#[tokio::test]
async fn role_change_on_same_terminal_drops_queued_pending_input() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let _first = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await; // Live, still awaiting paint
    // Typed for the active wire before its first paint: the gate
    // keeps it queued.
    pane.key(&key('a'));
    pane.paste(b"typed for the active wire");
    assert_eq!(pane.input_backlog().0, 2);
    // Same terminal, a different role: still a spec change.
    pane.want(Some(spec(Role::Observe)));
    assert_eq!(pane.input_backlog().0, 0);
    let (mut observer, params) = timeout(T, accept.recv()).await.unwrap().unwrap();
    assert!(params.observe);
    observer
        .serve_attach(testcore::attach_result(
            CH,
            testcore::minimal_terminal(),
            true,
        ))
        .await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut observer, b"observed").await;
    assert!(vt_text(&mut pane).contains("observed"));
    // The retained screen survives a same-terminal reattach, and the
    // first wire's queued bytes never come back as phantom input.
    pane.want(Some(spec(Role::Active)));
    let mut active = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut active, b"active again").await;
    assert!(vt_text(&mut pane).contains("active again"));
    no_kind(&mut active, CH, &[Kind::Data], 40).await;
}

#[tokio::test]
async fn queued_input_behind_a_full_writer_dies_on_the_role_change() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut first = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut first, b"ready").await;
    // A press dispatched on this wire is held there; its release dies
    // with the wire — plain `want` parks nothing.
    pane.key(&key('a'));
    assert_eq!(read_kind(&mut first, CH, Kind::Data).await.payload, b"a");
    let held_route = pane.generation();
    while !matches!(&pane.state, State::Live(live) if live.out.capacity() == OUT_QUEUE) {
        step(&mut pane, &mut rx).await;
    }
    let State::Live(live) = &pane.state else {
        panic!("live")
    };
    for _ in 0..OUT_QUEUE {
        live.out
            .try_send(Out::Input(b"x".to_vec()))
            .unwrap_or_else(|_| panic!("fixture queue"));
    }
    // A control at the full writer's head keeps the key behind it a
    // semantic `Pending::Key` — accepted, never yet encoded.
    pane.focused(false);
    pane.key(&key('k'));
    assert_eq!(pane.input_backlog().0, 1);
    pane.want(Some(spec(Role::Observe)));
    assert_eq!(pane.input_backlog().0, 0);
    assert!(!pane.lease_input(held_route, &release('a')));
    let (mut observer, params) = timeout(T, accept.recv()).await.unwrap().unwrap();
    assert!(params.observe);
    observer
        .serve_attach(testcore::attach_result(
            CH,
            testcore::minimal_terminal(),
            true,
        ))
        .await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut observer, b"view").await;
    no_kind(&mut observer, CH, &[Kind::Data], 40).await;
    // Back to active on the same terminal: only fresh state syncs —
    // no stranded key or release reaches the replacement wire.
    pane.want(Some(spec(Role::Active)));
    let mut active = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut active, b"back").await;
    no_kind(&mut active, CH, &[Kind::Data], 40).await;
}

#[tokio::test]
async fn active_to_size_lease_drops_queued_pending_input_too() {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let _first = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await; // Live, still awaiting paint
    pane.key(&key('a'));
    assert_eq!(pane.input_backlog().0, 1);
    // A focused pane demoted to a silent size holder keeps the
    // terminal but drops its typing rights — queued input dies too.
    pane.want(Some(spec(Role::SizeLease)));
    assert_eq!(pane.input_backlog().0, 0);
    let (mut lease, params) = timeout(T, accept.recv()).await.unwrap().unwrap();
    assert!(!params.observe);
    lease
        .serve_attach(testcore::attach_result(
            CH,
            testcore::minimal_terminal(),
            true,
        ))
        .await;
    step_until_live(&mut pane, &mut rx).await;
    no_kind(&mut lease, CH, &[Kind::Data, Kind::Focus], 40).await;
    pane.want(Some(spec(Role::Active)));
    let mut active = serve(&mut accept, true).await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut active, b"focused again").await;
    no_kind(&mut active, CH, &[Kind::Data], 40).await;
}
