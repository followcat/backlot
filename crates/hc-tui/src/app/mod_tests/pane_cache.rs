//! Cached screens across tab switches: switching back adopts the
//! observer's VT — the wire that kept it alive hands it over at the first
//! paint — instead of downloading history again. Every doubt (no observer,
//! evicted, failed) leaves the attach cold with the configured history.

use super::*;
use crate::config::DEFAULT_ATTACH_HISTORY_LINES;
use crate::testcore;
use hc_proto::client::Attach;
use hc_proto::frame::Kind;
use hc_proto::terminal::AttachParams;
use tokio::time::timeout;

/// The real loop's order on a focus change: retarget the pane, let it
/// reconcile against the view, then (after the frame) open the observers.
pub(super) fn visit(app: &mut App, id: &str) {
    app.focused = Some(id.into());
    app.refresh_projection();
    app.sync_target();
    let view = app.view.clone();
    app.pane.on_view(&view);
    app.sync_observers();
}

pub(super) fn with_openers(app: &mut App) -> mpsc::Receiver<(testcore::FakeCore, AttachParams)> {
    let (accepted, requests) = mpsc::channel(64);
    app.open_attach = Arc::new(move |params: AttachParams| {
        let accepted = accepted.clone();
        Box::pin(async move {
            let (core, read, write) = testcore::pair(64 * 1024);
            accepted.send((core, params.clone())).await.unwrap();
            Attach::open(read, write, None, &params).await
        })
    });
    // The focused pane was built with the default opener; rebuild it over
    // this one so its attaches land on the request channel.
    let pane = app.new_pane();
    app.pane = pane;
    requests
}

/// The next attach the app asks for, driving pane events while it waits.
pub(super) async fn next_attach(
    app: &mut App,
    requests: &mut mpsc::Receiver<(testcore::FakeCore, AttachParams)>,
) -> (testcore::FakeCore, AttachParams) {
    timeout(testcore::DEADLINE, async {
        loop {
            tokio::select! {
                request = requests.recv() => return request.expect("attach request"),
                event = app.pane_rx.recv() => {
                    app.pane_event(event.expect("pane event"), Instant::now());
                }
            }
        }
    })
    .await
    .expect("attach request deadline")
}

/// Replies to an attach for `params.terminal` and returns the wire.
pub(super) async fn serve(
    mut core: testcore::FakeCore,
    params: &AttachParams,
    channel: u32,
) -> testcore::FakeCore {
    core.serve_attach(testcore::attach_result(
        channel,
        serde_json::to_value(crate::sidebar::fixture::term(&params.terminal)).unwrap(),
        true,
    ))
    .await;
    core
}

/// Drives the app loop until `done` holds for the focused pane.
pub(super) async fn drive_until(app: &mut App, mut done: impl FnMut(&App) -> bool) {
    timeout(testcore::DEADLINE, async {
        while !done(app) {
            let event = app.pane_rx.recv().await.expect("pane event");
            app.pane_event(event, Instant::now());
        }
    })
    .await
    .expect("paint deadline");
}

/// The screen-only replay the core sends for a warm attach.
const WARM: &[u8] = b"\x18\x1b[!p\x1b[H\x1b[2J";

/// Scrollback plus screen as plain text, one row per line.
fn shown_text(app: &App) -> String {
    app.pane.vt_ref().map(pane_text).unwrap_or_default()
}

fn observer_text(observer: &pane::Pane) -> String {
    observer.vt_ref().map(pane_text).unwrap_or_default()
}

fn pane_text(vt: &hc_vt::Vt) -> String {
    let rows = vt.total_rows().unwrap();
    let range = vt.screen_text_rows_range(0, rows).unwrap();
    range
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .filter_map(|cell| cell.graphemes.first().and_then(|g| char::from_u32(*g)))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn switching_back_adopts_the_cached_screen_instead_of_history() {
    let mut app = app_with(&["t1", "t2"]);
    let mut requests = with_openers(&mut app);

    // t1 focused: its first active attach has nothing cached to continue.
    visit(&mut app, "t1");
    let (core1, params1) = next_attach(&mut app, &mut requests).await;
    assert_eq!(params1.terminal, "t1");
    assert!(!params1.observe);
    assert_eq!(params1.history, DEFAULT_ATTACH_HISTORY_LINES);
    let mut core1 = serve(core1, &params1, 1).await;
    core1
        .send_stream(1, Kind::Data, b"\x1b[?9001lt1 screen")
        .await;
    drive_until(&mut app, |app| app.pane.has_fresh_paint()).await;
    let t1_screen = app.pane.vt_ref().unwrap().id();

    // Leave t1: its screen goes to the observer that inherits it, and
    // that observer's attach asks for the screen only.
    visit(&mut app, "t2");
    let (observer_core, observer_params) = next_attach(&mut app, &mut requests).await;
    assert_eq!(observer_params.terminal, "t1");
    assert!(observer_params.observe);
    assert_eq!(
        observer_params.history, 0,
        "leaving must not download history"
    );
    let mut observer_core = serve(observer_core, &observer_params, 2).await;
    let (t2_core, t2_params) = next_attach(&mut app, &mut requests).await;
    assert_eq!(t2_params.terminal, "t2");
    assert!(!t2_params.observe);
    assert_eq!(t2_params.history, DEFAULT_ATTACH_HISTORY_LINES);
    let mut t2_core = serve(t2_core, &t2_params, 3).await;
    t2_core
        .send_stream(3, Kind::Data, b"\x1b[?9001lt2 screen")
        .await;
    let mut warm_paint = WARM.to_vec();
    warm_paint.extend_from_slice(b"t1 warm screen\x1b[?9001l");
    observer_core.send_stream(2, Kind::Data, &warm_paint).await;
    drive_until(&mut app, |app| app.pane.has_fresh_paint()).await;
    drive_until(&mut app, |app| {
        app.observers.get("t1").is_some_and(|o| o.has_fresh_paint())
    })
    .await;

    // Back to t1. `sync_target` opens the observer for the tab just left
    // first, then retargets the pane: both ask for the screen only.
    visit(&mut app, "t1");
    let (t2_observer, t2_observer_params) = next_attach(&mut app, &mut requests).await;
    assert_eq!(t2_observer_params.terminal, "t2");
    assert!(t2_observer_params.observe);
    assert_eq!(t2_observer_params.history, 0);
    let _t2_observer = serve(t2_observer, &t2_observer_params, 4).await;
    let (back_core, back_params) = next_attach(&mut app, &mut requests).await;
    assert_eq!(back_params.terminal, "t1");
    assert!(!back_params.observe);
    assert_eq!(
        back_params.history, 0,
        "a warm switch asks for the screen only"
    );
    let mut back_core = serve(back_core, &back_params, 5).await;
    back_core
        .send_stream(5, Kind::Data, &[WARM, b"t1 again\x1b[?9001l"].concat())
        .await;
    drive_until(&mut app, |app| app.pane.has_fresh_paint()).await;
    assert_eq!(
        app.pane.vt_ref().map(|vt| vt.id()),
        Some(t1_screen),
        "the switch continued the cached VT"
    );
    assert!(
        !app.observers.contains_key("t1"),
        "the observer handed its wire and screen over"
    );
}

/// Every numbered line printed across two switches is still there, in
/// order — the cached scrollback is continued, never replayed over.
#[tokio::test]
async fn numbered_output_survives_a_switch_and_a_warm_revisit() {
    fn numbered(range: std::ops::Range<usize>) -> Vec<u8> {
        let mut out = Vec::new();
        for line in range {
            out.extend_from_slice(format!("line {line:03}\r\n").as_bytes());
        }
        out
    }
    let mut app = app_with(&["t1", "t2"]);
    let mut requests = with_openers(&mut app);

    visit(&mut app, "t1");
    let (core1, params1) = next_attach(&mut app, &mut requests).await;
    let mut core1 = serve(core1, &params1, 1).await;
    core1.send_stream(1, Kind::Data, b"\x1b[?9001l").await;
    drive_until(&mut app, |app| app.pane.has_fresh_paint()).await;
    core1.send_stream(1, Kind::Data, &numbered(0..20)).await;
    drive_until(&mut app, |app| shown_text(app).contains("line 019")).await;

    // Leave t1. The observer continues the same screen, so output that
    // arrives while the switch is in flight is kept — not downloaded
    // again from the core's history.
    visit(&mut app, "t2");
    let (t1_observer, t1_observer_params) = next_attach(&mut app, &mut requests).await;
    let mut t1_observer = serve(t1_observer, &t1_observer_params, 2).await;
    let (t2_core, t2_params) = next_attach(&mut app, &mut requests).await;
    let mut t2_core = serve(t2_core, &t2_params, 3).await;
    t2_core.send_stream(3, Kind::Data, b"\x1b[?9001lt2").await;
    drive_until(&mut app, |app| app.pane.has_fresh_paint()).await;
    t1_observer
        .send_stream(2, Kind::Data, &numbered(20..40))
        .await;
    drive_until(&mut app, |app| {
        app.observers
            .get("t1")
            .is_some_and(|observer| observer_text(observer).contains("line 039"))
    })
    .await;

    // Back to t1: the warm paint carries the screen only — here the 24
    // rows the core's screen holds at attach time — and the program
    // continues on the active wire.
    visit(&mut app, "t1");
    let (t2_observer, t2_observer_params) = next_attach(&mut app, &mut requests).await;
    let _t2_observer = serve(t2_observer, &t2_observer_params, 4).await;
    let (back_core, back_params) = next_attach(&mut app, &mut requests).await;
    assert_eq!(back_params.terminal, "t1");
    assert_eq!(back_params.history, 0);
    let mut back_core = serve(back_core, &back_params, 5).await;
    let mut paint = WARM.to_vec();
    for line in 16..39 {
        paint.extend_from_slice(format!("line {line:03}\r\n").as_bytes());
    }
    paint.extend_from_slice(b"line 039\x1b[?9001l");
    back_core.send_stream(5, Kind::Data, &paint).await;
    drive_until(&mut app, |app| app.pane.has_fresh_paint()).await;
    back_core
        .send_stream(5, Kind::Data, &numbered(40..60))
        .await;
    drive_until(&mut app, |app| shown_text(app).contains("line 059")).await;

    let text = shown_text(&app);
    let mut seen = Vec::new();
    for token in text.split("line ") {
        let digits: String = token.chars().take_while(char::is_ascii_digit).collect();
        if digits.len() == 3
            && let Ok(line) = digits.parse::<usize>()
        {
            seen.push(line);
        }
    }
    let mut unique = seen.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique,
        (0..60).collect::<Vec<_>>(),
        "every numbered line must survive both switches"
    );
    // Duplicates are the overlap window's cost and are reported, never
    // hidden: the sequence must stay non-decreasing either way.
    assert!(
        seen.windows(2).all(|pair| pair[0] <= pair[1]),
        "scrollback order must be preserved: {seen:?}"
    );
}

/// An evicted observer leaves nothing to adopt: the revisit opens cold
/// with the configured history.
#[tokio::test]
async fn an_evicted_screen_is_reopened_cold_with_history() {
    let mut app = app_with(&["t1", "t2"]);
    let mut requests = with_openers(&mut app);
    visit(&mut app, "t1");
    let (core1, params1) = next_attach(&mut app, &mut requests).await;
    let mut core1 = serve(core1, &params1, 1).await;
    core1
        .send_stream(1, Kind::Data, b"\x1b[?9001lt1 screen")
        .await;
    drive_until(&mut app, |app| app.pane.has_fresh_paint()).await;

    visit(&mut app, "t2");
    let (observer_core, observer_params) = next_attach(&mut app, &mut requests).await;
    assert!(observer_params.observe);
    let _observer = serve(observer_core, &observer_params, 2).await;
    let (t2_core, t2_params) = next_attach(&mut app, &mut requests).await;
    let mut t2_core = serve(t2_core, &t2_params, 3).await;
    t2_core
        .send_stream(3, Kind::Data, b"\x1b[?9001lt2 screen")
        .await;
    drive_until(&mut app, |app| app.pane.has_fresh_paint()).await;

    // The LRU dropped t1's observer.
    app.observers.remove("t1");
    app.observer_mru.retain(|id| id != "t1");
    visit(&mut app, "t1");
    // The tab just left gains an observer; t1 has nothing cached, so its
    // active attach replays history.
    let (t2_observer, t2_observer_params) = next_attach(&mut app, &mut requests).await;
    assert_eq!(t2_observer_params.terminal, "t2");
    assert!(t2_observer_params.observe);
    let _t2_observer = serve(t2_observer, &t2_observer_params, 4).await;
    let (cold, cold_params) = next_attach(&mut app, &mut requests).await;
    assert_eq!(cold_params.terminal, "t1");
    assert_eq!(cold_params.history, DEFAULT_ATTACH_HISTORY_LINES);
    drop(cold);
}
