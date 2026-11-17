//! The first `backlot` read and the entry decision (U7 fix 1): a slot
//! that `request_doc` created is queued, not answered, and must never
//! decide the screen. Driven through a real watcher over a fake core.
use super::*;

// ---------- the first read on the wire (U7 fix 1) ----------

/// The core's answers for this test: hello, an empty subscription
/// snapshot, and every `doc.get` but `backlot` answered at once.
/// The `backlot` read is held until the test lets it go, so the
/// watcher's first publish carries the slot exactly as
/// `request_doc` made it.
async fn serve_studios_later(
    mut core: crate::testcore::FakeCore,
    framed: bool,
    asked: std::sync::Arc<tokio::sync::Notify>,
    hold: std::sync::Arc<tokio::sync::Notify>,
    body: serde_json::Value,
) {
    if !framed {
        crate::testcore::serve_subscribe(
            &mut core,
            serde_json::json!({ "seq": 1, "terminals": [] }),
        )
        .await;
        std::future::pending::<()>().await;
    }
    core.answer_hello(true, crate::testcore::METHODS, &[]).await;
    while let Some(frame) = core.try_read_frame().await {
        if frame.channel != 0 || frame.kind != hc_proto::frame::Kind::Data {
            continue;
        }
        let request: serde_json::Value = serde_json::from_slice(&frame.payload).unwrap();
        let result = match request["method"].as_str().unwrap_or("") {
            "doc.get" => {
                asked.notify_one();
                hold.notified().await;
                serde_json::json!({ "value": body, "version": 2 })
            }
            "machine.list" => serde_json::json!({ "machines": [] }),
            "terminal.list" => serde_json::json!({ "terminals": [] }),
            _ => serde_json::Value::Null,
        };
        core.reply(&request["id"], result).await;
    }
}

/// Two studios on the core, and the `backlot` read held on the wire: the
/// watcher's first publish carries the queued slot, not an answer. The
/// entry must not be decided on it — no card, and when the studios land
/// the bar opens expanded. This is the shape a real attach hit: the slot
/// `request_doc` creates used to read as an empty, answered document.
#[tokio::test]
async fn a_queued_document_does_not_decide_the_entry() {
    let (accepted, rx) = mpsc::channel(4);
    let (rpc, sub) = crate::testcore::openers(accepted);
    let watcher = Watcher::with_openers(
        rpc,
        sub,
        hc_proto::watch::WatchParams {
            traversal: Some(crate::project::agent_traversal),
            local_documents: vec!["backlot".into()],
            ..hc_proto::watch::WatchParams::default()
        },
    );
    let asked = Arc::new(tokio::sync::Notify::new());
    let hold = Arc::new(tokio::sync::Notify::new());
    let (asked_core, hold_core) = (asked.clone(), hold.clone());
    let body = summary_json();
    crate::testcore::spawn_fake(rx, move |core, framed| {
        let (asked, hold, body) = (asked_core.clone(), hold_core.clone(), body.clone());
        Box::pin(serve_studios_later(core, framed, asked, hold, body))
    });

    let mut app = app_over(watcher);
    app.set_plain(false);
    // The first publish: the slot exists and its read is on the wire.
    let first =
        crate::testcore::await_view(&app.watcher, |view| view.documents.contains_key(&doc_key()))
            .await;
    assert!(
        first.documents[&doc_key()].fetching,
        "a queued slot reads as fetching"
    );
    app.on_view();
    assert!(
        !app.opening.ready(),
        "a queued slot is not an answer to decide on"
    );
    assert!(!app.opening_visible());
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(!text.contains("New studio"), "{text}");

    // The read lands with two studios: the bar opens, the card never
    // showed.
    tokio::time::timeout(crate::testcore::DEADLINE, asked.notified())
        .await
        .expect("the backlot read was never dispatched");
    hold.notify_one();
    crate::testcore::await_view(&app.watcher, |view| {
        view.documents
            .get(&doc_key())
            .is_some_and(|doc| !doc.fetching && !doc.value.is_null())
    })
    .await;
    app.on_view();
    assert!(app.opening.ready(), "the answer decides it");
    assert_eq!(
        app.hollywood.mode,
        LayerMode::Expanded,
        "a client with studios opens the bar"
    );
    assert!(!app.opening_visible());
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(!text.contains("New studio"), "{text}");
    assert!(text.contains("acme-shop"), "{text}");
}
