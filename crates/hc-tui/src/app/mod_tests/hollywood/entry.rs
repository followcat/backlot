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
            documents: vec!["backlot".into()],
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

    let mut app = app_over(watcher, true);
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

    // The read lands with two studios: the bar stays collapsed —
    // the pill is its face — and the card never showed.
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
        LayerMode::Collapsed,
        "a client with studios starts on the pill (S0)"
    );
    assert!(!app.opening_visible());
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(!text.contains("New studio"), "{text}");
    // R3-M6: a plain `new` workspace is no studio workspace — the
    // sidebar stays studio-free; the pill's count proves the read.
    assert!(text.contains("◉2"), "{text}");
}

// ---------- another machine's late studios (U6a) ----------

/// This core's `backlot` answers at once with zero studios — the card
/// shows — while `host-b`'s read is held. Releasing it publishes the
/// remote studios: the card closes and the bar stays collapsed and
/// unfocused, exactly like a late local studio.
#[tokio::test]
async fn remote_studios_close_the_card_without_opening_the_bar() {
    let (accepted, rx) = mpsc::channel(4);
    let (rpc, sub) = crate::testcore::openers(accepted);
    let watcher = Watcher::with_openers(
        rpc,
        sub,
        hc_proto::watch::WatchParams {
            traversal: Some(crate::project::agent_traversal),
            documents: vec!["backlot".into()],
            ..hc_proto::watch::WatchParams::default()
        },
    );
    // host-b's `doc.get` parks until `released` flips: the serve loop
    // must not await inside a request — every `doc.get` is its own
    // spawned call on this one connection, so a blocking hold could
    // starve the local read the entry waits on.
    let released = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (released_core, body) = (released.clone(), summary_json());
    crate::testcore::spawn_fake(rx, move |mut core, framed| {
        let (released, body) = (released_core.clone(), body.clone());
        Box::pin(async move {
            if !framed {
                crate::testcore::serve_subscribe(
                    &mut core,
                    serde_json::json!({ "seq": 1, "terminals": [] }),
                )
                .await;
                std::future::pending::<()>().await;
            }
            core.answer_hello(true, crate::testcore::METHODS, &[]).await;
            let mut held: Option<serde_json::Value> = None;
            let mut tick = tokio::time::interval(std::time::Duration::from_millis(25));
            loop {
                tokio::select! {
                    frame = core.try_read_frame() => {
                        let Some(frame) = frame else { break };
                        if frame.channel != 0 || frame.kind != hc_proto::frame::Kind::Data {
                            continue;
                        }
                        let request: serde_json::Value =
                            serde_json::from_slice(&frame.payload).unwrap();
                        let result = match request["method"].as_str().unwrap_or("") {
                            "machine.list" => serde_json::json!({
                                "machines": [{
                                    "name": "host-b", "target": "alice@host",
                                    "id": "core-b", "host": "host-b",
                                    "version": "0.9", "state": "connected",
                                }]
                            }),
                            "doc.get" if request["params"]["on"] == "host-b" => {
                                held = Some(request["id"].clone());
                                continue;
                            }
                            // This core: an answered, studio-less document.
                            "doc.get" => serde_json::json!({
                                "value": {"schema": "backlot/1", "updated_ms": 1, "studios": []},
                                "version": 1
                            }),
                            "terminal.list" => serde_json::json!({ "terminals": [] }),
                            _ => serde_json::Value::Null,
                        };
                        core.reply(&request["id"], result).await;
                    }
                    _ = tick.tick(), if held.is_some() => {
                        if released.load(std::sync::atomic::Ordering::Relaxed) {
                            let id = held.take().unwrap();
                            core.reply(&id, serde_json::json!({
                                "value": body, "version": 2
                            }))
                            .await;
                        }
                    }
                }
            }
        })
    });

    let mut app = app_over(watcher, true);
    app.set_plain(false);
    // The local document settles studio-less: the card is up and the
    // bar collapsed while host-b's read is still on the wire.
    crate::testcore::await_view(&app.watcher, |view| {
        view.documents
            .get(&doc_key())
            .is_some_and(|doc| !doc.fetching && !doc.value.is_null())
    })
    .await;
    app.on_view();
    assert!(app.opening.ready(), "the local read settles the entry");
    assert!(app.opening_visible(), "no studios anywhere yet: the card");
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);

    // host-b's document lands: its studios are real studios — the
    // card leaves and the bar keeps its collapsed, unfocused state.
    released.store(true, std::sync::atomic::Ordering::Relaxed);
    crate::testcore::await_view(&app.watcher, |view| {
        view.documents
            .get(&hc_proto::watch::DocKey {
                machine: Some("host-b".into()),
                name: "backlot".into(),
            })
            .is_some_and(|doc| !doc.fetching && !doc.value.is_null())
    })
    .await;
    app.on_view();
    assert!(!app.opening_visible(), "a remote studio closes the card");
    assert_eq!(
        app.hollywood.mode,
        LayerMode::Collapsed,
        "arriving studios do not pull the bar open"
    );
    // The lot pill counts the remote machine's studios.
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(text.contains("◉2"), "the pill's studio count: {text}");
}
