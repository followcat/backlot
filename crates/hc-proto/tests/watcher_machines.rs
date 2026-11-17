//! Machine-list refresh under the watcher: heartbeats that return an
//! unchanged list publish nothing, while a visible field change publishes
//! exactly once.

mod common;

use common::attach::DEADLINE;
use common::watcher::{
    METHODS, await_view, machine, serve_rpc, serve_subscribe, spawn_fake, watcher,
};
use hc_proto::watch::Link;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;

/// Poll `calls` until it reaches `want` — the fake core counts each
/// answered `machine.list`.
async fn machine_lists_reach(calls: &AtomicU64, want: u64) {
    timeout(DEADLINE, async {
        while calls.load(Ordering::SeqCst) < want {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("a heartbeat's machine.list never completed");
}

/// Every heartbeat queues a `machine.list`; an answer identical to the
/// stored one carries nothing a subscriber can see, so it must not
/// publish. A real field change publishes exactly once.
#[tokio::test]
async fn an_unchanged_machine_list_does_not_publish() {
    let (watcher, accepted) = watcher(vec![]);
    let machines = Arc::new(Mutex::new(vec![machine("host-b")]));
    let calls = Arc::new(AtomicU64::new(0));
    let (events_tx, events_rx) = mpsc::channel::<(String, Value)>(16);
    let mut events_rx = Some(events_rx);
    let fake = {
        let machines = Arc::clone(&machines);
        let calls = Arc::clone(&calls);
        spawn_fake(accepted, move |core, framed| {
            let machines = Arc::clone(&machines);
            let calls = Arc::clone(&calls);
            let events_rx = if framed { None } else { events_rx.take() };
            Box::pin(async move {
                if framed {
                    serve_rpc(core, METHODS, move |req| {
                        match req["method"].as_str().unwrap() {
                            "machine.list" => {
                                calls.fetch_add(1, Ordering::SeqCst);
                                let m = machines.lock().unwrap().clone();
                                Some(Ok(json!({"machines": m})))
                            }
                            _ => Some(Ok(Value::Null)),
                        }
                    })
                    .await;
                } else {
                    let mut core = core;
                    serve_subscribe(&mut core, METHODS, json!({"seq": 1, "terminals": []})).await;
                    let mut events_rx = events_rx.expect("one sub conn");
                    while let Some((method, params)) = events_rx.recv().await {
                        core.notify(&method, params).await;
                    }
                }
            })
        })
    };
    let settled = await_view(&watcher, |v| {
        v.link == Link::Online
            && v.machines
                .as_ref()
                .is_some_and(|m| m.iter().any(|m| m.name == "host-b"))
    })
    .await;
    machine_lists_reach(&calls, 1).await;
    let mut version = settled.version;

    // Three heartbeats, each answered with the stored list: nothing a
    // subscriber can see changed, so the published version stays put.
    for _ in 0..3 {
        events_tx
            .send(("heartbeat".into(), json!({"seq": 1})))
            .await
            .unwrap();
        machine_lists_reach(&calls, calls.load(Ordering::SeqCst) + 1).await;
        // Give a redundant publish a bounded chance to land: correct
        // behavior never produces one.
        if timeout(Duration::from_millis(300), watcher.changed_since(version))
            .await
            .is_ok()
        {
            version = watcher.version();
        }
    }
    assert_eq!(
        watcher.version(),
        settled.version,
        "unchanged machine.list answers published {} extra views",
        watcher.version() - settled.version
    );

    // A field a subscriber sees changes (the line state): one publish.
    machines.lock().unwrap()[0]["state"] = "connecting".into();
    let before = watcher.version();
    events_tx
        .send(("heartbeat".into(), json!({"seq": 1})))
        .await
        .unwrap();
    machine_lists_reach(&calls, calls.load(Ordering::SeqCst) + 1).await;
    let view = await_view(&watcher, |v| {
        v.machines
            .as_ref()
            .is_some_and(|m| m.iter().any(|m| m.state == "connecting"))
    })
    .await;
    assert_eq!(
        view.version,
        before + 1,
        "a changed machine field publishes exactly once"
    );
    watcher.stop();
    fake.abort();
}
