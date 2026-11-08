//! Opt-in integration suite against a real pinned `herdr-core` binary.
//!
//! Ignored by default: `cargo test` never touches a real core. Run
//! explicitly with
//!
//! ```sh
//! HC_TUI_TEST_CORE=<path-to-pinned-herdr-core> \
//!     cargo test -p hc-proto --test integration -- --ignored --test-threads=1
//! ```
//!
//! A missing `HC_TUI_TEST_CORE` fails the explicit run instead of skipping.
//! Every test owns a fresh [`ScratchCore`]: a random `hctui-test-*`
//! session, temporary XDG/HOME roots and a foreground `serve` it stops
//! and reaps itself — no real session is ever discovered or signaled.
//! Clients and watchers are always closed before the fixture drops so no
//! reconnect/autostart can race the final `server stop`.

#![cfg(unix)]

mod common;

use std::ffi::OsStr;
use std::time::Duration;

use common::scratch::{ScratchCore, core_binary};
use hc_proto::client::{Attach, AttachConn, Client, Output};
use hc_proto::endpoint::Endpoint;
use hc_proto::events::{ServerNotification, SubscribeParams, event_kind};
use hc_proto::subscribe::Subscription;
use hc_proto::terminal::{AttachParams, ListParams, ReadParams, RunParams, TerminalParams};
use hc_proto::watch::{Link, WatchParams, Watcher};
use serde_json::json;

/// Finite bound on any single real-core step.
const DEADLINE: Duration = Duration::from_secs(20);

async fn connect(endpoint: &Endpoint) -> Client {
    Client::connect(endpoint, None::<&OsStr>)
        .await
        .expect("connect to scratch core")
}

/// `terminal.read` until `needle` appears — output flush timing varies.
async fn read_until(client: &Client, terminal: &str, needle: &str) -> String {
    tokio::time::timeout(DEADLINE, async {
        loop {
            let out = client
                .terminal_read(ReadParams {
                    terminal: terminal.into(),
                    lines: 100,
                    caller: None,
                })
                .await
                .expect("terminal.read");
            if out.text.contains(needle) {
                return out.text;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("terminal.read never contained {needle:?}"))
}

/// Events for `terminal` until one of `kinds` arrives; every other kind
/// for it (and every heartbeat) is skipped, so interleaved events cannot
/// satisfy or break the wait.
async fn await_kind(sub: &mut Subscription, terminal: &str, kinds: &[&str]) -> String {
    tokio::time::timeout(DEADLINE, async {
        loop {
            if let ServerNotification::Event(e) = sub.recv().await.expect("subscription recv")
                && e.terminal.as_deref() == Some(terminal)
                && kinds.contains(&e.kind.as_str())
            {
                return e.kind;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("no {kinds:?} event for {terminal} before deadline"))
}

/// Poll a `Watcher` view until `cond` holds. `changed_since` only wakes;
/// the condition is re-checked against the current view each time.
async fn await_view(watcher: &Watcher, what: &str, cond: impl Fn(&hc_proto::watch::View) -> bool) {
    tokio::time::timeout(DEADLINE, async {
        loop {
            let view = watcher.view();
            if cond(&view) {
                return;
            }
            watcher.changed_since(view.version).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("view never satisfied {what}"))
}

/// The terminal's program pid is still alive — used before and after the
/// adoption teardown to prove the detached holder was really stopped.
fn alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// hello/discovery and the initial terminal list on a fresh scratch core.
#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE pointing at the pinned herdr-core binary"]
async fn hello_discovery_and_initial_list() {
    let _binary = core_binary();
    let fixture = ScratchCore::spawn().await;

    let client = connect(&fixture.endpoint).await;
    assert_eq!(client.core().name, "host-a");
    for method in [
        "terminal.list",
        "terminal.run",
        "terminal.read",
        "terminal.attach",
        "terminal.kill",
        "events.subscribe",
        "machine.list",
        "doc.get",
    ] {
        assert!(
            client.hello().methods.iter().any(|m| m == method),
            "hello methods missing {method}"
        );
    }
    common::discovery_settled(&client).await;
    assert!(
        client.discovery().is_some(),
        "rpc.discover must yield a document on the real core"
    );

    let list = client
        .terminal_list(ListParams::default())
        .await
        .expect("terminal.list");
    assert!(list.terminals.is_empty(), "fresh session lists nothing");

    drop(client);
    fixture
        .finish()
        .unwrap_or_else(|root| panic!("teardown failed; root kept at {root:?}"));
}

/// subscribe-before-run ordering, a controlled command in a neutral cwd,
/// its created/status events, deterministic read, attach first paint +
/// human input, then kill and the final event.
#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE pointing at the pinned herdr-core binary"]
async fn subscribe_then_run_read_attach_kill() {
    let fixture = ScratchCore::spawn().await;

    // Subscription first: its snapshot must be the authoritative empty one.
    let mut sub = Subscription::connect(
        &fixture.endpoint,
        None::<&OsStr>,
        &SubscribeParams::default(),
    )
    .await
    .expect("events.subscribe");
    assert!(
        sub.inventory().terminals().is_empty(),
        "snapshot on a fresh session is empty"
    );

    let client = connect(&fixture.endpoint).await;
    // A controlled program: reads one line, then prints a marker that is
    // never part of the typed input — tty echo alone cannot produce it.
    let run = client
        .terminal_run(RunParams {
            name: Some("hctui-it".into()),
            argv: vec![
                "/bin/sh".into(),
                "-c".into(),
                "read x; printf 'PROOF-9c3a71\\n'; read y".into(),
            ],
            cwd: Some(fixture.work.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .await
        .expect("terminal.run");
    let address = run.terminal.address.clone();
    assert_eq!(run.terminal.name.as_deref(), Some("hctui-it"));

    // The subscription sees the new terminal before we touch it again.
    await_kind(&mut sub, &address, &[event_kind::CREATED]).await;

    let attach = Attach::connect(
        &fixture.endpoint,
        None::<&OsStr>,
        &AttachParams {
            terminal: address.clone(),
            observe: false,
            resize_notifications: true,
            quiet_human_release: false,
            history: 100,
            cols: 80,
            rows: 24,
            display: None,
            resumed: false,
            caller: None,
        },
    )
    .await
    .expect("terminal.attach");

    // First paint = real DATA; RESIZE/CONTROL alone do not count.
    tokio::time::timeout(DEADLINE, async {
        loop {
            match attach.recv().await {
                Some(Output::Data(bytes)) => {
                    attach.consume(bytes.len()).expect("credit first paint");
                    return;
                }
                Some(Output::Resize { .. }) | Some(Output::Control { .. }) => {}
                Some(Output::Closed(r)) => panic!("attach closed early: {r:?}"),
                Some(Output::Failed(e)) => panic!("attach failed: {e}"),
                None => panic!("attach stream ended before first paint"),
            }
        }
    })
    .await
    .expect("no first-paint DATA before deadline");

    // Human input through the framed stream makes the program emit the
    // marker — proof it executed, not just echoed.
    attach.send_input(b"go\n").await.expect("send_input");
    let screen = read_until(&client, &address, "PROOF-9c3a71").await;
    assert!(screen.contains("PROOF-9c3a71"));

    attach.detach().await.expect("detach");

    client
        .terminal_kill(TerminalParams {
            terminal: address.clone(),
            caller: None,
        })
        .await
        .expect("terminal.kill");
    let last = await_kind(
        &mut sub,
        &address,
        &[event_kind::FINISHED, event_kind::REMOVED],
    )
    .await;
    assert!(
        [event_kind::FINISHED, event_kind::REMOVED].contains(&last.as_str()),
        "expected the terminal's final event, got {last}"
    );

    sub.close().await.expect("subscription close");
    drop(client);
    fixture
        .finish()
        .unwrap_or_else(|root| panic!("teardown failed; root kept at {root:?}"));
}

/// `attach`'s DATA frames accumulate until `needle` is in them — proof
/// this channel itself carried the terminal's output.
async fn data_until(attach: &Attach, needle: &str) {
    tokio::time::timeout(DEADLINE, async {
        let mut received = Vec::new();
        loop {
            match attach.recv().await {
                Some(Output::Data(bytes)) => {
                    attach.consume(bytes.len()).expect("credit");
                    received.extend_from_slice(&bytes);
                    if String::from_utf8_lossy(&received).contains(needle) {
                        return;
                    }
                }
                Some(Output::Resize { .. }) | Some(Output::Control { .. }) => {}
                other => panic!("attach ended before {needle:?}: {other:?}"),
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("no {needle:?} DATA on the attach"))
}

/// Two attaches on one shared connection against the real core: both
/// receive the terminal's output on their own channels and one detaches
/// cleanly while the other keeps receiving — the local twin of what a
/// remote endpoint's multiplexed attaches do.
#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE pointing at the pinned herdr-core binary"]
async fn two_attaches_on_one_shared_connection() {
    let fixture = ScratchCore::spawn().await;
    let client = connect(&fixture.endpoint).await;
    let run = client
        .terminal_run(RunParams {
            name: Some("hctui-mux".into()),
            argv: vec![
                "/bin/sh".into(),
                "-c".into(),
                "read x; printf 'MUX-7f2e\\n'; read y; printf 'DONE-44aa\\n'".into(),
            ],
            cwd: Some(fixture.work.to_string_lossy().into_owned()),
            ..Default::default()
        })
        .await
        .expect("terminal.run");
    let address = run.terminal.address.clone();

    let conn = AttachConn::new(fixture.endpoint.clone(), None);
    let attach = |observe: bool| AttachParams {
        terminal: address.clone(),
        observe,
        resize_notifications: true,
        quiet_human_release: false,
        history: 100,
        cols: 80,
        rows: 24,
        display: None,
        resumed: false,
        caller: None,
    };
    // The active and the observer of one terminal ride one connection —
    // each gets its own channel.
    let active_params = attach(false);
    let observer_params = attach(true);
    let (active, observer) =
        tokio::join!(conn.attach(&active_params), conn.attach(&observer_params));
    let active = active.expect("active attach");
    let observer = observer.expect("observer attach");
    assert_ne!(active.channel(), observer.channel(), "two channels");

    active.send_input(b"go\n").await.expect("send_input");
    data_until(&active, "MUX-7f2e").await;
    data_until(&observer, "MUX-7f2e").await;

    // Detaching the observer closes only its channel; the connection
    // and the active attach keep working — a second marker proves it.
    observer.detach().await.expect("observer detach");
    active.send_input(b"quit\n").await.expect("send_input");
    data_until(&active, "DONE-44aa").await;
    active.detach().await.expect("active detach");

    client
        .terminal_kill(TerminalParams {
            terminal: address.clone(),
            caller: None,
        })
        .await
        .expect("terminal.kill");
    drop(conn);
    drop(client);
    fixture
        .finish()
        .unwrap_or_else(|root| panic!("teardown failed; root kept at {root:?}"));
}

/// Watcher over the real core: online on generation 1 with a doc fetched —
/// then a killed+respawned core yields offline, a fresh subscription
/// epoch, and a *fresh* `doc.get`: the doc's value is swapped on disk
/// while the core is down, so a retained epoch-1 cache cannot pass.
#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE pointing at the pinned herdr-core binary"]
async fn watcher_recovers_new_epoch_and_refetches_docs() {
    let mut fixture = ScratchCore::spawn().await;

    // Seed the watched document through the wire (doc.set version-gated).
    let seeder = connect(&fixture.endpoint).await;
    seeder
        .call(
            "doc.set",
            Some(json!({"name": "hctui-note", "value": {"v": 1}, "version": 0})),
        )
        .await
        .expect("doc.set v1");
    drop(seeder);

    let watcher = Watcher::connect(
        &fixture.endpoint,
        None::<&OsStr>,
        WatchParams {
            documents: vec!["hctui-note".into()],
            reconnect_initial: Duration::from_millis(100),
            reconnect_max: Duration::from_secs(2),
            ..Default::default()
        },
    );

    await_view(&watcher, "epoch 1 online", |v| {
        v.generation == 1
            && v.link == Link::Online
            && v.rpc == Link::Online
            && v.core.as_ref().is_some_and(|c| c.name == "host-a")
    })
    .await;
    await_view(&watcher, "epoch 1 doc v1 installed", |v| {
        v.documents
            .values()
            .any(|d| d.value["v"] == 1 && !d.fetching)
    })
    .await;

    fixture.kill();
    await_view(&watcher, "offline after kill", |v| {
        matches!(v.rpc, Link::Offline { .. }) || matches!(v.link, Link::Offline { .. })
    })
    .await;

    // While the core is down, only the owned state file changes — v2 can
    // only come from a fresh epoch-2 `doc.get`, never from cache.
    std::fs::write(
        fixture.state_dir().join("docs.json"),
        json!({"hctui-note": {"value": {"v": 2}, "version": 2}}).to_string(),
    )
    .expect("rewrite owned docs.json");

    fixture.respawn();
    await_view(&watcher, "epoch 2 online", |v| {
        v.generation == 2 && v.link == Link::Online && v.rpc == Link::Online
    })
    .await;
    await_view(&watcher, "epoch 2 doc v2 installed", |v| {
        v.generation == 2
            && v.documents
                .values()
                .any(|d| d.value["v"] == 2 && !d.fetching)
    })
    .await;

    watcher.close().await;
    fixture
        .finish()
        .unwrap_or_else(|root| panic!("teardown failed; root kept at {root:?}"));
}

/// Teardown regression: a live terminal under a detached holder, the
/// serve killed, then `finish` *without* any mid-test respawn — the
/// adoption path must stop the holder and its program. The known pid
/// disappears and the owned root is removed.
#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE pointing at the pinned herdr-core binary"]
async fn teardown_stops_detached_holders_and_programs() {
    let mut fixture = ScratchCore::spawn().await;
    let client = connect(&fixture.endpoint).await;

    let run = client
        .terminal_run(RunParams {
            name: Some("hctui-leak".into()),
            argv: vec!["/bin/sh".into(), "-c".into(), "sleep 600".into()],
            ..Default::default()
        })
        .await
        .expect("terminal.run");
    let pid = run.terminal.pid.expect("terminal record carries a pid");

    fixture.kill();
    assert!(
        alive(pid),
        "the holder must keep the program alive across serve death"
    );

    let root = fixture.root.clone();
    drop(client); // close clients before teardown: connect can autostart
    fixture
        .finish() // adopts holders via a replacement serve, then stops
        .unwrap_or_else(|root| panic!("teardown failed; root kept at {root:?}"));
    assert!(!root.exists(), "successful teardown removes its owned root");
    assert!(
        !alive(pid),
        "teardown must stop the detached holder's program (pid {pid})"
    );
}
