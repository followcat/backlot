//! Watcher modes and lifecycle: static fallback, `stop`/`close`/`drop`
//! ownership of blocked openers, and resync on malformed input.

mod common;

use common::attach::DEADLINE;
use common::fakecore::{self, DuplexCore, hello_result_framed};
use common::watcher::{await_view, reconnect_params, serve_subscribe, spawn_fake, watcher};
use hc_proto::watch::{Link, OpenSub, SubAttempt, WatchParams};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Duration, timeout};

const METHODS: &[&str] = &[
    "hello",
    "terminal.list",
    "machine.list",
    "doc.get",
    "events.subscribe",
];

fn terminal(address: &str) -> Value {
    let mut t = common::minimal_terminal();
    t["address"] = address.into();
    t
}

/// An rpc conn answering machine.list/terminal.list/`doc.get` sanely.
async fn serve_rpc_ok(mut core: DuplexCore) {
    core.answer_hello(hello_result_framed(METHODS, &[], true))
        .await;
    while let Some(req) = core.try_read_request().await {
        match req["method"].as_str().unwrap() {
            "machine.list" => core.reply(&req["id"], json!({"machines": []})).await,
            "terminal.list" => {
                core.reply(
                    &req["id"],
                    json!({"terminals": [terminal("t-static")], "hidden_finished": 0}),
                )
                .await;
            }
            "doc.get" => {
                core.reply(&req["id"], json!({"value": {"v": 1}, "version": 1}))
                    .await;
            }
            _ => core.reply(&req["id"], Value::Null).await,
        }
    }
}

#[tokio::test]
async fn stop_ends_reconnect_attempts() {
    let (watcher, accepted) = watcher(vec![]);
    let (kill_tx, kill_rx) = oneshot::channel::<()>();
    let mut kill_rx = Some(kill_rx);
    let mut hold_rx = Some(oneshot::channel::<()>().1); // held forever
    let attempts = Arc::new(AtomicUsize::new(0));
    let mut plain = 0u32;
    let fake = {
        let attempts = Arc::clone(&attempts);
        spawn_fake(accepted, move |core, framed| {
            let (kill, hold) = if framed {
                (None, None)
            } else {
                plain += 1;
                (
                    if plain == 1 { kill_rx.take() } else { None },
                    if plain == 2 { hold_rx.take() } else { None },
                )
            };
            let attempts = Arc::clone(&attempts);
            Box::pin(async move {
                if framed {
                    serve_rpc_ok(core).await;
                    return;
                }
                attempts.fetch_add(1, Ordering::SeqCst);
                let mut core = core;
                if let Some(rx) = hold {
                    let _ = rx.await; // conn 2's hello never answers
                }
                serve_subscribe(&mut core, METHODS, json!({"seq": 1, "terminals": []})).await;
                if let Some(rx) = kill {
                    let _ = rx.await; // conn 1 dies on command
                } else {
                    std::future::pending::<()>().await;
                }
            })
        })
    };
    await_view(&watcher, |v| v.link == Link::Online).await;
    kill_tx.send(()).unwrap();
    await_view(&watcher, |v| matches!(v.link, Link::Offline { .. })).await;

    watcher.stop();
    await_view(&watcher, |v| v.link == Link::Stopped).await;
    // Backoffs tick at 20-200 ms; half a second without an attempt is
    // proof none will come.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "a reconnect ran after stop"
    );
    fake.abort();
}

#[tokio::test]
async fn an_unsupported_subscribe_falls_back_to_static_lists() {
    // The plain conn's hello advertises no events.subscribe → the
    // watcher takes the terminal.list path and stays there.
    let (watcher, accepted) = watcher(vec!["hc-tui".into()]);
    let sub_attempts = Arc::new(AtomicUsize::new(0));
    let fake = {
        let sub_attempts = Arc::clone(&sub_attempts);
        spawn_fake(accepted, move |core, framed| {
            let sub_attempts = Arc::clone(&sub_attempts);
            Box::pin(async move {
                if framed {
                    serve_rpc_ok(core).await;
                } else {
                    sub_attempts.fetch_add(1, Ordering::SeqCst);
                    let mut core = core;
                    core.answer_hello(hello_result_framed(&["hello", "terminal.list"], &[], false))
                        .await;
                    // The subscribe call must not even be attempted:
                    // method gating happens client-side. Park on the
                    // wire instead — a request would fail this test.
                    std::future::pending::<()>().await;
                }
            })
        })
    };
    let view = await_view(&watcher, |v| {
        matches!(v.link, Link::Static { .. }) && v.inventory.get("t-static").is_some()
    })
    .await;
    assert!(view.notes.contains_key("events.subscribe"));
    assert!(!view.inventory.is_offline());
    // Static is a stable resting state. install_rpc may re-probe once
    // per rpc epoch (capabilities can change); it must never churn —
    // the count is bounded and then frozen.
    let settled = sub_attempts.load(Ordering::SeqCst);
    assert!(settled <= 2, "unbounded sub attempts: {settled}");
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        sub_attempts.load(Ordering::SeqCst),
        settled,
        "static mode kept resubscribing"
    );
    watcher.stop();
    fake.abort();
}

#[tokio::test]
async fn a_held_opener_never_stacks_and_drop_aborts_it() {
    // The subscription opener hands the fake a conn then hangs forever;
    // the watcher must run exactly one attempt, never stacking retries.
    let (accepted_tx, mut accepted) = mpsc::channel(16);
    let attempts = Arc::new(AtomicUsize::new(0));
    let sub: OpenSub = {
        let attempts = Arc::clone(&attempts);
        Arc::new(move |_| {
            let tx = accepted_tx.clone();
            let attempts = Arc::clone(&attempts);
            Box::pin(async move {
                attempts.fetch_add(1, Ordering::SeqCst);
                let (core, _read, _write) = fakecore::pair(64 * 1024);
                if tx.send(core).await.is_err() {
                    return SubAttempt::Failed(hc_proto::subscribe::SubscribeError::Open(
                        hc_proto::client::Error::Timeout("fake accept"),
                    ));
                }
                std::future::pending::<SubAttempt>().await
            })
        })
    };
    let (rpc, _unused_sub) = common::watcher::openers({
        let (tx, mut rx) = mpsc::channel::<DuplexCore>(1);
        // Drain rpc conns in the background so openers never block.
        tokio::spawn(async move {
            while let Some(mut core) = rx.recv().await {
                tokio::spawn(async move {
                    // No spawn_fake here — the drain consumes hello
                    // itself, exactly like the accept loop does.
                    core.read_hello().await;
                    serve_rpc_ok(core).await;
                });
            }
        });
        tx
    });
    let params = WatchParams {
        documents: vec![],
        ..reconnect_params()
    };
    let watcher = hc_proto::watch::Watcher::with_openers(rpc, sub, params);

    let server = timeout(DEADLINE, accepted.recv()).await.unwrap().unwrap();
    await_view(&watcher, |v| matches!(v.link, Link::Connecting { .. })).await;
    // Well past several backoff ticks: still exactly one attempt.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "a held open stacked retries"
    );

    // Dropping the watcher aborts the held opener: its client halves
    // die and the fake's conn EOFs — a detached task would keep it up.
    drop(watcher);
    timeout(DEADLINE, async {
        let mut core = server;
        core.expect_eof_line().await;
    })
    .await
    .expect("the held opener was not aborted on drop");
}

/// A `close()` future cancelled while the actor is still dying must
/// not strand the held opener: `Watcher`'s `Drop` carries the same
/// stop+abort ownership, so the conn dies either way.
#[tokio::test]
async fn a_cancelled_close_still_kills_a_blocked_opener() {
    let (accepted_tx, mut accepted) = mpsc::channel(16);
    let sub: OpenSub = {
        Arc::new(move |_| {
            let tx = accepted_tx.clone();
            Box::pin(async move {
                let (core, _read, _write) = fakecore::pair(64 * 1024);
                if tx.send(core).await.is_err() {
                    return SubAttempt::Failed(hc_proto::subscribe::SubscribeError::Open(
                        hc_proto::client::Error::Timeout("fake accept"),
                    ));
                }
                std::future::pending::<SubAttempt>().await
            })
        })
    };
    let (rpc, _unused_sub) = common::watcher::openers({
        let (tx, mut rx) = mpsc::channel::<DuplexCore>(1);
        tokio::spawn(async move {
            while let Some(mut core) = rx.recv().await {
                tokio::spawn(async move {
                    core.read_hello().await;
                    serve_rpc_ok(core).await;
                });
            }
        });
        tx
    });
    let params = WatchParams {
        documents: vec![],
        ..reconnect_params()
    };
    let watcher = hc_proto::watch::Watcher::with_openers(rpc, sub, params);
    let server = timeout(DEADLINE, accepted.recv()).await.unwrap().unwrap();

    // Poll close until it takes the actor handle and waits for it. No
    // actor task can run during this poll on the current-thread runtime.
    // Cancelling now exercises the retained abort handle, rather than
    // dropping an unpolled close future.
    let mut closing = Box::pin(watcher.close());
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(closing.as_mut(), cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    drop(closing);
    timeout(DEADLINE, async {
        let mut core = server;
        core.expect_eof_line().await;
    })
    .await
    .expect("a cancelled close left the held opener alive");
}
