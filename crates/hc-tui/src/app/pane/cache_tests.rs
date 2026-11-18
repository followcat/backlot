//! Cached-screen handovers: a VT that stayed live across a switch is
//! continued by the next attach instead of being replaced by a history
//! replay, and every doubt falls back to a cold open.

use super::*;
use crate::config::{DEFAULT_ATTACH_HISTORY_LINES, DEFAULT_SCROLLBACK_LIMIT_BYTES};

/// The screen-only replay the core sends for a warm attach: cancel the
/// parser, soft reset, every viewer mode off, home, erase the display,
/// then the screen. Copied from herdr-core `Terminal::paint(0)`.
const WARM_PREFIX: &[u8] = b"\x18\x1b[!p\x1b[H\x1b[2J";

/// Numbered lines that outgrow a 24-row screen, so they sit in scrollback.
fn numbered(lines: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for line in 0..lines {
        out.extend_from_slice(format!("line {line:03} kept\r\n").as_bytes());
    }
    out
}

fn scrollback_rows(pane: &mut Pane) -> usize {
    let vt = pane.vt().expect("a painted pane keeps a VT");
    vt.total_rows().unwrap() - usize::from(vt.size().unwrap().1)
}

/// A painted active pane whose screen is worth handing over.
async fn painted_active() -> (
    Pane,
    mpsc::Receiver<PaneEv>,
    mpsc::Receiver<(FakeCore, AttachParams)>,
    FakeCore,
) {
    let (mut pane, mut rx, mut accept) = rig();
    pane.want(Some(spec(Role::Active)));
    let mut core = serve(&mut accept, true).await;
    step(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core, b"first screen").await;
    (pane, rx, accept, core)
}

/// Drives the pane until it asks for another attach — the cold reopen
/// paths answer a reply or a first paint with a fresh, history-full open.
async fn next_attach(
    pane: &mut Pane,
    rx: &mut mpsc::Receiver<PaneEv>,
    accept: &mut mpsc::Receiver<(FakeCore, AttachParams)>,
) -> (FakeCore, AttachParams) {
    timeout(T, async {
        loop {
            tokio::select! {
                event = rx.recv() => pane.on_event(event.expect("pane event"), Instant::now()),
                request = accept.recv() => return request.expect("attach request"),
            }
        }
    })
    .await
    .expect("the pane never reopened its attach")
}

#[tokio::test]
async fn a_seeded_screen_asks_for_the_screen_only_and_continues_its_vt() {
    let (mut active, mut rx, _accept, mut core) = painted_active().await;
    raw_paint(&mut active, &mut rx, &mut core, &numbered(60)).await;
    let before_rows = scrollback_rows(&mut active);
    assert!(before_rows >= 30, "the fixture must fill scrollback");
    let before_id = active.vt().unwrap().id();

    // Leaving the pane: the VT is handed to the observer that inherits
    // the terminal, with a warm ticket because the wire was live.
    let retained = active
        .take_retained()
        .expect("a painted pane hands its screen over");
    assert!(retained.warm.is_some(), "a live wire lends a warm ticket");

    let (mut observer, mut orx, mut oaccept) = rig();
    observer.want_retained(Some(spec(Role::Observe)), Some(retained));
    let (mut ocore, params) = timeout(T, oaccept.recv()).await.unwrap().unwrap();
    assert!(params.observe);
    assert_eq!(params.history, 0, "a warm attach asks for the screen only");
    assert_eq!(
        observer.vt().map(|vt| vt.id()),
        Some(before_id),
        "the retained screen is on display while the attach opens"
    );
    ocore
        .serve_attach(testcore::attach_result(
            CH,
            testcore::minimal_terminal(),
            true,
        ))
        .await;
    step_until_live(&mut observer, &mut orx).await;
    // The core's screen-only paint lands in the same VT.
    let mut paint_bytes = WARM_PREFIX.to_vec();
    paint_bytes.extend_from_slice(b"warm screen");
    paint(&mut observer, &mut orx, &mut ocore, &paint_bytes).await;
    assert_eq!(observer.vt().map(|vt| vt.id()), Some(before_id));
    assert!(
        scrollback_rows(&mut observer) >= before_rows,
        "the cached scrollback must survive the warm paint"
    );
    assert!(vt_text(&mut observer).contains("warm screen"));
}

#[tokio::test]
async fn a_resumed_attach_replays_history_instead_of_continuing_a_cached_vt() {
    let (mut active, mut rx, _accept, mut core) = painted_active().await;
    raw_paint(&mut active, &mut rx, &mut core, &numbered(40)).await;
    let before_id = active.vt().unwrap().id();
    let retained = active.take_retained().expect("a screen to hand over");

    // Wire recovery may have dropped bytes: a resumed attach must not
    // trust the cached VT, whatever it was promised.
    let (mut pane, mut prx, mut accept) = rig();
    pane.install_retained(retained);
    pane.spawn(spec(Role::Observe), true, 0);
    let (mut core, params) = timeout(T, accept.recv()).await.unwrap().unwrap();
    assert_eq!(params.history, DEFAULT_ATTACH_HISTORY_LINES);
    core.serve_attach(testcore::attach_result(
        CH,
        testcore::minimal_terminal(),
        true,
    ))
    .await;
    step_until_live(&mut pane, &mut prx).await;
    paint(&mut pane, &mut prx, &mut core, b"cold screen").await;
    assert_ne!(
        pane.vt().map(|vt| vt.id()),
        Some(before_id),
        "a resumed open builds a fresh VT"
    );
    assert_eq!(
        pane.vt().unwrap().scrollback_max_bytes(),
        Some(DEFAULT_SCROLLBACK_LIMIT_BYTES)
    );
}

#[tokio::test]
async fn a_changed_incarnation_withdraws_the_warm_promise() {
    // The cached screen is offered to a pane whose attach reply reports
    // another incarnation: same address, different program.
    let (mut active, mut rx, _accept, mut core) = painted_active().await;
    paint(&mut active, &mut rx, &mut core, b"old program").await;
    let ticket = active.warm_ticket().expect("a live VT lends warmth");
    drop(active.take_retained());

    let (mut pane, mut prx, mut pacc) = rig();
    pane.expect_warm(Some(ticket));
    pane.want(Some(spec(Role::Observe)));
    let (mut core, params) = timeout(T, pacc.recv()).await.unwrap().unwrap();
    assert_eq!(params.history, 0);
    let mut terminal = testcore::minimal_terminal();
    terminal["created_ms"] = json!(1700000000001u64);
    core.serve_attach(testcore::attach_result(CH, terminal, true))
        .await;
    // The reply proves a restarted terminal: the pane reopens cold.
    let (mut core2, params2) = next_attach(&mut pane, &mut prx, &mut pacc).await;
    assert_eq!(params2.history, DEFAULT_ATTACH_HISTORY_LINES);
    core2
        .serve_attach(testcore::attach_result(
            CH,
            testcore::minimal_terminal(),
            true,
        ))
        .await;
    step_until_live(&mut pane, &mut prx).await;
    assert!(
        pane.vt().is_none(),
        "a re-opened pane keeps no borrowed screen"
    );
    paint(&mut pane, &mut prx, &mut core2, b"new program").await;
    assert!(vt_text(&mut pane).contains("new program"));
}

#[tokio::test]
async fn a_full_screen_app_keeps_its_screen_across_a_warm_switch() {
    let (mut active, mut rx, _accept, mut core) = painted_active().await;
    // The program enters the alternate screen and paints it.
    raw_paint(
        &mut active,
        &mut rx,
        &mut core,
        b"\x1b[?1049h\x1b[H\x1b[2Jfull-screen app",
    )
    .await;
    assert!(active.vt().unwrap().alt_screen().unwrap());
    let retained = active.take_retained().expect("a screen to hand over");

    let (mut observer, mut orx, mut oaccept) = rig();
    observer.want_retained(Some(spec(Role::Observe)), Some(retained));
    let (mut ocore, params) = timeout(T, oaccept.recv()).await.unwrap().unwrap();
    assert_eq!(params.history, 0);
    ocore
        .serve_attach(testcore::attach_result(
            CH,
            testcore::minimal_terminal(),
            true,
        ))
        .await;
    step_until_live(&mut observer, &mut orx).await;
    // The core's replay leaves the alternate screen, resets, re-enters it
    // and paints the screen — the case `Terminal::paint` handles for a
    // full-screen program.
    let mut bytes = WARM_PREFIX.to_vec();
    bytes.extend_from_slice(b"\x1b[?1049h\x1b[H\x1b[2Jfull-screen app");
    paint(&mut observer, &mut orx, &mut ocore, &bytes).await;
    let vt = observer.vt().expect("the warm VT survives");
    assert!(
        vt.alt_screen().unwrap(),
        "the app must stay on the alt screen"
    );
    assert!(vt_text(&mut observer).contains("full-screen app"));
}

#[tokio::test]
async fn a_warm_attach_whose_screen_never_arrives_reopens_cold() {
    let ticket = WarmVt {
        incarnation: (1700000000000, None),
    };
    let (mut pane, mut rx, mut accept) = rig();
    pane.expect_warm(Some(ticket));
    pane.want(Some(spec(Role::Active)));
    let (mut core, params) = timeout(T, accept.recv()).await.unwrap().unwrap();
    assert_eq!(params.history, 0);
    core.serve_attach(testcore::attach_result(
        CH,
        testcore::minimal_terminal(),
        true,
    ))
    .await;
    step_until_live(&mut pane, &mut rx).await;
    // No observer handed a screen over: a screen-only paint would land in
    // nothing, so the pane asks for full history again.
    core.send_stream(CH, Kind::Data, b"\x1b[H\x1b[2Jscreen")
        .await;
    let (mut core2, params2) = next_attach(&mut pane, &mut rx, &mut accept).await;
    assert_eq!(params2.history, DEFAULT_ATTACH_HISTORY_LINES);
    core2
        .serve_attach(testcore::attach_result(
            CH,
            testcore::minimal_terminal(),
            true,
        ))
        .await;
    step_until_live(&mut pane, &mut rx).await;
    paint(&mut pane, &mut rx, &mut core2, b"cold screen").await;
    assert!(vt_text(&mut pane).contains("cold screen"));
}
