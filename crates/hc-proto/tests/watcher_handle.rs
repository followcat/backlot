//! `Watcher` handle semantics: the `changed_since` cursor must not
//! lose publishes, and document reads never precede the subscription.

mod common;

use common::attach::DEADLINE;
use common::minimal_terminal;
use common::watcher::{await_view, serve_rpc, serve_subscribe, spawn_fake, watcher};
use hc_proto::watch::Link;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

const METHODS: &[&str] = &[
    "hello",
    "terminal.list",
    "machine.list",
    "doc.get",
    "events.subscribe",
];

/// The subscription's reply must land before any `doc.get` reaches the
/// rpc conn — proved by holding the reply and asserting no document
/// request can arrive first.
#[tokio::test]
async fn documents_never_read_before_subscribe() {
    let (watcher, accepted) = watcher(vec!["hc-tui".into()]);
    let (release, released) = oneshot::channel::<()>();
    let (seen_tx, mut seen) = mpsc::channel::<String>(8);
    let mut released = Some(released);
    let fake = spawn_fake(accepted, move |core, framed| {
        let released = if framed { None } else { released.take() };
        let seen_tx = seen_tx.clone();
        Box::pin(async move {
            if framed {
                let mut core = core;
                core.answer_hello(common::fakecore::hello_result_framed(METHODS, &[], true))
                    .await;
                while let Some(req) = core.try_read_request().await {
                    seen_tx
                        .send(req["method"].as_str().unwrap().into())
                        .await
                        .ok();
                    if req["method"] == "doc.get" {
                        core.reply(&req["id"], json!({"value": null, "version": 0}))
                            .await;
                    } else {
                        core.reply(&req["id"], json!({"machines": []})).await;
                    }
                }
            } else {
                let mut core = core;
                core.answer_hello(common::fakecore::hello_result_framed(METHODS, &[], false))
                    .await;
                let req = core.read_message().await;
                assert_eq!(req["method"], "events.subscribe");
                released.unwrap().await.ok(); // hold the reply
                core.reply_line(&req["id"], json!({"seq": 1, "terminals": []}))
                    .await;
                std::future::pending::<()>().await;
            }
        })
    });

    // While the subscribe reply is held, a doc.get must not reach the
    // rpc conn (machine.list is unordered — it needs no epoch).
    let mut early = Vec::new();
    let _ = tokio::time::timeout(Duration::from_millis(300), async {
        while let Some(method) = seen.recv().await {
            early.push(method);
        }
    })
    .await;
    assert!(
        !early.iter().any(|m| m == "doc.get"),
        "a doc.get arrived before subscribe completed: {early:?}"
    );
    release.send(()).unwrap();
    await_view(&watcher, |v| v.link == Link::Online && v.generation == 1).await;
    loop {
        let method = tokio::time::timeout(DEADLINE, seen.recv())
            .await
            .unwrap()
            .unwrap();
        if method == "doc.get" {
            break;
        }
    }
    fake.abort();
}

/// `changed_since` must observe a publish that landed while no receiver
/// existed — the no-later-event race.
#[tokio::test]
async fn changed_since_observes_a_publish_made_without_receivers() {
    let (watcher, accepted) = watcher(vec![]);
    let (go_tx, go_rx) = oneshot::channel::<()>();
    let mut go_rx = Some(go_rx);
    let fake = spawn_fake(accepted, move |core, framed| {
        let go_rx = go_rx.take();
        Box::pin(async move {
            if framed {
                serve_rpc(core, METHODS, |req| {
                    if req["method"] == "machine.list" {
                        Some(Ok(json!({"machines": []})))
                    } else {
                        Some(Ok(Value::Null))
                    }
                })
                .await;
            } else {
                let mut core = core;
                let snapshot = json!({"seq": 1, "terminals": [minimal_terminal()]});
                serve_subscribe(&mut core, METHODS, snapshot).await;
                go_rx.unwrap().await.ok(); // hold the event until no receiver can exist
                core.notify(
                    "event",
                    json!({"seq": 2, "type": "title", "terminal": "t7", "data": {"title": "x"}}),
                )
                .await;
                std::future::pending::<()>().await;
            }
        })
    });
    let seen = await_view(&watcher, |v| v.link == Link::Online)
        .await
        .version;
    go_tx.send(()).unwrap();

    // The event publish lands while no `changed_since` receiver exists;
    // poll the view to confirm it landed, then the cursor must resolve.
    tokio::time::timeout(DEADLINE, async {
        while watcher.view().version == seen {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the event never published");
    tokio::time::timeout(Duration::from_millis(100), watcher.changed_since(seen))
        .await
        .expect("a publish made without receivers was lost");
    fake.abort();
}
