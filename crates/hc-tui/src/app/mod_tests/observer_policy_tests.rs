//! Off-tab observer policy: the bounded LRU drops an evicted pane's
//! attach immediately — zero background feeds while evicted — and a
//! revisit opens a fresh `observe` wire that repaints from core
//! `history` rather than retaining anything itself.

use super::*;
use crate::testcore;
use hc_proto::client::Attach;
use hc_proto::frame::Kind;
use tokio::time::timeout;

fn visit(app: &mut App, id: &str) {
    app.focused = Some(id.into());
    app.refresh_projection();
    app.sync_observers();
}

async fn serve_observer(
    requests: &mut mpsc::Receiver<(testcore::FakeCore, hc_proto::terminal::AttachParams)>,
    terminal: &str,
    channel: u32,
) -> (testcore::FakeCore, hc_proto::terminal::AttachParams) {
    let (mut core, params) = timeout(testcore::DEADLINE, requests.recv())
        .await
        .expect("attach accept deadline")
        .expect("attach accept");
    assert_eq!(params.terminal, terminal);
    assert!(params.observe);
    core.serve_attach(testcore::attach_result(
        channel,
        serde_json::to_value(crate::sidebar::fixture::term(terminal)).unwrap(),
        true,
    ))
    .await;
    (core, params)
}

async fn drive_until_fresh_paint(app: &mut App, id: &str) {
    timeout(testcore::DEADLINE, async {
        while !app.observers[id].has_fresh_paint() {
            let event = app.pane_rx.recv().await.expect("pane event channel");
            app.pane_event(event, Instant::now());
        }
    })
    .await
    .expect("observer never reached a fresh paint");
}

#[tokio::test]
async fn evicted_observer_closes_its_attach_and_a_revisit_repaints_history() {
    // 34 single-pane tabs overflow the 32-entry off-tab LRU.
    let ids: Vec<String> = (1..=34).map(|n| format!("t{n}")).collect();
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    let mut app = app_with(&refs);
    let (accepted, mut requests) = mpsc::channel(64);
    app.open_attach = Arc::new(move |params| {
        let accepted = accepted.clone();
        Box::pin(async move {
            let (core, read, write) = testcore::pair(64 * 1024);
            accepted.send((core, params.clone())).await.unwrap();
            Attach::open(read, write, None, &params).await
        })
    });
    visit(&mut app, "t1");
    let (mut core1, _) = serve_observer(&mut requests, "t1", 7).await;
    core1
        .send_stream(7, Kind::Data, b"\x1b[?9001lt1 screen")
        .await;
    drive_until_fresh_paint(&mut app, "t1").await;

    // Walk past the LRU: tab 1's observer is evicted once 32 newer
    // off-tab panes are cached. The un-served cores stay held so no
    // wire EOF churns a Failed report.
    let mut held = Vec::new();
    for id in &ids[1..] {
        visit(&mut app, id);
        let (core, _params) = timeout(testcore::DEADLINE, requests.recv())
            .await
            .expect("attach accept deadline")
            .expect("attach accept");
        held.push(core);
    }
    assert!(!app.observers.contains_key("t1"));

    // The dropped pane ends its attach: either its queued CLOSE lands
    // on the wire, or the dedicated conn goes down with it — both leave
    // the evicted observer feeding on nothing further.
    timeout(testcore::DEADLINE, async {
        while let Some(frame) = core1.try_read_frame().await {
            if frame.channel == 7 && frame.kind == Kind::Close {
                return;
            }
        }
    })
    .await
    .expect("evicted observer must close its attach");

    // A revisit opens a fresh observe attach asking for first-paint
    // history — nothing was retained off the wire while evicted.
    visit(&mut app, "t1");
    let (mut core1b, params1b) = serve_observer(&mut requests, "t1", 9).await;
    assert_eq!(params1b.history, pane::HISTORY);
    core1b
        .send_stream(
            9,
            Kind::Data,
            b"history row one\r\nhistory row two\r\n\x1b[?9001l$ ",
        )
        .await;
    drive_until_fresh_paint(&mut app, "t1").await;
    let text: String = app
        .observers
        .get_mut("t1")
        .unwrap()
        .vt()
        .unwrap()
        .frame(0)
        .unwrap()
        .rows
        .iter()
        .flat_map(|row| row.cells.iter().map(|cell| cell.text.as_str()))
        .collect();
    assert!(text.contains("history row one"), "{text}");
    assert!(text.contains("history row two"), "{text}");
    drop(held);
}
