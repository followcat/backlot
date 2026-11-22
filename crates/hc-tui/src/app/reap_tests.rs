//! `should_reap_exited` predicate coverage, `reap_exited` gating and
//! dedup, and the worker's `terminal.get` → `terminal.rm` exchange over
//! a fake core.
use super::*;
use crate::app::{App, OpenAttach, Probe};
use crate::testcore::{self, FakeCore};
use hc_proto::watch::{Link, Watcher};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::mpsc;

fn app_with(terms: Vec<Terminal>) -> App {
    let (accepted, _rx) = mpsc::channel(1);
    let (rpc, sub) = crate::testcore::openers(accepted);
    let watcher = Watcher::with_openers(rpc, sub, hc_proto::watch::WatchParams::default());
    let open: OpenAttach = Arc::new(|_| unreachable!("no attach in this test"));
    let config = crate::config::Config::default();
    let shell = crate::chrome::ClientShellConfig::from_config(
        &config,
        None,
        None,
        std::path::PathBuf::new(),
    );
    let mut app = App::new(
        watcher,
        open,
        config.live_keybinds(),
        shell,
        Arc::new(Probe::default()),
    );
    app.view = crate::sidebar::fixture::view(terms, vec![]);
    app
}

fn exited(address: &str, created_ms: u64) -> Terminal {
    let mut term = crate::sidebar::fixture::term(address);
    term.state = "finished".into();
    term.reason = Some("exited".into());
    term.exit_code = Some(0);
    term.created_ms = created_ms;
    term
}

fn exited_record(address: &str, created_ms: u64) -> serde_json::Value {
    let mut record = testcore::minimal_terminal();
    record["address"] = json!(address);
    record["created_ms"] = json!(created_ms);
    record["state"] = json!("finished");
    record["reason"] = json!("exited");
    record["exit_code"] = json!(0);
    record
}

async fn hello(core: &mut FakeCore, methods: &[&str]) {
    core.read_hello().await;
    core.answer_hello(true, methods, &[]).await;
}

/// A JSON-RPC error reply for the request `request` (its id is reused).
async fn reply_error(core: &mut FakeCore, request: &serde_json::Value, kind: &str) {
    let error = json!({"jsonrpc":"2.0","id":request["id"],
        "error":{"code":-32000,"message":kind,"data":{"kind":kind}}});
    core.send(&hc_proto::frame::Frame::data(0, serde_json::to_vec(&error).unwrap()).encode())
        .await;
}

/// Nothing may arrive for `millis` — the window is short, so a healthy
/// exchange never flakes.
async fn expect_quiet(core: &mut FakeCore, millis: u64) {
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(millis),
            core.read_request()
        )
        .await
        .is_err(),
        "unexpected request reached the core"
    );
}

#[test]
fn only_a_finished_exited_terminal_is_eligible() {
    let now = 10_000;
    assert!(should_reap_exited(&exited("t1", 0), now));
    for reason in [
        "killed",
        "signaled",
        "holder_lost",
        "server_stop",
        "host_shutdown",
        "stopped_by_restart",
    ] {
        let mut term = exited("t1", 0);
        term.reason = Some(reason.into());
        assert!(!should_reap_exited(&term, now), "{reason} must stay");
    }
    let mut live = exited("t1", 0);
    live.state = "live".into();
    assert!(!should_reap_exited(&live, now));
    let mut unknown = exited("t1", 0);
    unknown.reason = None;
    assert!(!should_reap_exited(&unknown, now));

    // A child ended automatically with its closed parent (`die_with_parent`)
    // is reaped instead of leaving residual `finished·killed` tabs behind.
    let mut child = exited("t1", 0);
    child.reason = Some("killed".into());
    child.extra.insert(
        "ended_by".into(),
        json!({"by": "die_with_parent", "parent": "t0"}),
    );
    assert!(should_reap_exited(&child, now));

    // Explicit `terminal.kill` without `rm` still stays.
    let mut manual_kill = exited("t1", 0);
    manual_kill.reason = Some("killed".into());
    manual_kill
        .extra
        .insert("ended_by".into(), json!({"by": "terminal.kill"}));
    assert!(!should_reap_exited(&manual_kill, now));
}

#[test]
fn wherdr_workers_and_coordinators_are_never_reaped() {
    let now = 10_000;
    for label in ["wherdr.run", "wherdr.task", "wherdr.coordinator"] {
        let mut term = exited("t1", 0);
        term.labels.insert(label.into(), "1".into());
        assert!(!should_reap_exited(&term, now), "{label} must stay");
    }
}

#[test]
fn a_wherdr_request_failure_keeps_its_launch_window() {
    // wherdr reads `launchFailure` for 2 s after a quick non-zero exit.
    let mut term = exited("t1", 8_000);
    term.exit_code = Some(1);
    term.labels.insert("wherdr-request".into(), "{}".into());
    assert!(!should_reap_exited(&term, 9_000));
    assert!(should_reap_exited(&term, 13_000));
    term.exit_code = Some(0);
    assert!(
        should_reap_exited(&term, 9_000),
        "a clean exit reaps at once"
    );
    term.exit_code = None;
    assert!(should_reap_exited(&term, 9_000), "absent means clean");
    // Without the label a non-zero exit has no read window at all.
    let mut term = exited("t1", 8_000);
    term.exit_code = Some(1);
    assert!(should_reap_exited(&term, 9_000));
}

#[tokio::test]
async fn exited_panes_are_queued_once_and_pruned_when_gone() {
    let mut app = app_with(vec![exited("t1", 7), exited("t2", 8)]);
    let (tx, mut rx) = mpsc::channel(8);
    app.reap_tx = Some(tx);
    app.reap_exited();
    let set = app.reaped_exited.clone();
    let reaped = || set.lock().unwrap().clone();
    assert_eq!(
        reaped(),
        HashSet::from([("t1".to_owned(), 7), ("t2".to_owned(), 8)])
    );
    assert_eq!(rx.try_recv().unwrap(), ("t1".to_owned(), 7));
    assert_eq!(rx.try_recv().unwrap(), ("t2".to_owned(), 8));
    app.reap_exited();
    assert!(rx.try_recv().is_err(), "a second pass re-queues nothing");
    // Once `t1` leaves the inventory its marker is pruned.
    let mut terms: Vec<_> = app.view.inventory.terminals().values().cloned().collect();
    terms.retain(|t| t.address != "t1");
    let seq = app.view.inventory.seq() + 1;
    app.view.inventory_mut().replace(seq, terms);
    app.reap_exited();
    assert_eq!(reaped(), HashSet::from([("t2".to_owned(), 8)]));
}

#[tokio::test]
async fn nothing_is_queued_offline_or_without_the_worker() {
    let mut app = app_with(vec![exited("t1", 7)]);
    app.reap_exited(); // no worker yet: quietly does nothing
    let (tx, mut rx) = mpsc::channel(8);
    app.reap_tx = Some(tx);
    for link in [
        Link::Offline {
            attempt: 1,
            error: "down".into(),
        },
        Link::Connecting { attempt: 1 },
    ] {
        app.view.link = link;
        app.reap_exited();
        assert!(rx.try_recv().is_err());
        app.view.link = Link::Online;
        app.view.rpc = Link::Offline {
            attempt: 1,
            error: "down".into(),
        };
        app.reap_exited();
        assert!(rx.try_recv().is_err());
        app.view.rpc = Link::Online;
    }
    assert!(app.reaped_exited.lock().unwrap().is_empty());
}

#[tokio::test]
async fn wherdr_records_never_reach_the_queue() {
    let mut pm = exited("t1", 7);
    pm.labels.insert("wherdr.task".into(), "1".into());
    let mut req = exited("t2", now_ms());
    req.exit_code = Some(1);
    req.labels.insert("wherdr-request".into(), "{}".into());
    let mut app = app_with(vec![pm, req]);
    let (tx, mut rx) = mpsc::channel(8);
    app.reap_tx = Some(tx);
    app.reap_exited();
    assert!(rx.try_recv().is_err(), "no wherdr record may be queued");
    assert!(app.reaped_exited.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_exited_terminal_is_reaped_over_rpc() {
    let mut app = app_with(vec![exited("t1", 7)]);
    let (accepted, mut rx) = mpsc::channel(4);
    let (rpc, _sub) = testcore::openers(accepted);
    app.set_operations(rpc);
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        hello(&mut core, &["terminal.get", "terminal.rm"]).await;
        let get = core.read_request().await;
        assert_eq!(get["method"], "terminal.get");
        assert_eq!(get["params"]["terminal"], "t1");
        core.reply(&get["id"], exited_record("t1", 7)).await;
        let rm = core.read_request().await;
        assert_eq!(rm["method"], "terminal.rm");
        assert_eq!(rm["params"], json!({"terminal": "t1"}));
        core.reply(&rm["id"], json!({})).await;
    });
    app.reap_exited();
    tokio::time::timeout(testcore::DEADLINE, server)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn a_replaced_terminal_is_not_removed() {
    let mut app = app_with(vec![exited("t1", 7)]);
    let (accepted, mut rx) = mpsc::channel(4);
    let (rpc, _sub) = testcore::openers(accepted);
    app.set_operations(rpc);
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        hello(&mut core, &["terminal.get", "terminal.rm"]).await;
        let get = core.read_request().await;
        // A different incarnation now owns the address: never remove it.
        core.reply(&get["id"], exited_record("t1", 999)).await;
        expect_quiet(&mut core, 300).await;
    });
    app.reap_exited();
    tokio::time::timeout(testcore::DEADLINE, server)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn an_already_removed_terminal_is_not_an_error() {
    let mut app = app_with(vec![exited("t1", 7)]);
    let (accepted, mut rx) = mpsc::channel(4);
    let (rpc, _sub) = testcore::openers(accepted);
    app.set_operations(rpc);
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        hello(&mut core, &["terminal.get", "terminal.rm"]).await;
        let get = core.read_request().await;
        reply_error(&mut core, &get, "not_found").await;
        expect_quiet(&mut core, 300).await;
    });
    app.reap_exited();
    tokio::time::timeout(testcore::DEADLINE, server)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn a_core_without_terminal_rm_is_left_alone() {
    let mut app = app_with(vec![exited("t1", 7)]);
    let (accepted, mut rx) = mpsc::channel(4);
    let (rpc, _sub) = testcore::openers(accepted);
    app.set_operations(rpc);
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        hello(&mut core, &["terminal.get"]).await;
        expect_quiet(&mut core, 300).await;
    });
    app.reap_exited();
    tokio::time::timeout(testcore::DEADLINE, server)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn a_transient_error_requeues_the_terminal() {
    let mut app = app_with(vec![exited("t1", 7)]);
    let (accepted, mut rx) = mpsc::channel(4);
    let (rpc, _sub) = testcore::openers(accepted);
    app.set_operations(rpc);
    let server = tokio::spawn(async move {
        let mut core = rx.recv().await.unwrap();
        hello(&mut core, &["terminal.get", "terminal.rm"]).await;
        let get = core.read_request().await;
        reply_error(&mut core, &get, "internal").await;
        // The transient failure drops the conn — the retry opens a new one.
        let mut core = rx.recv().await.unwrap();
        hello(&mut core, &["terminal.get", "terminal.rm"]).await;
        let get = core.read_request().await;
        core.reply(&get["id"], exited_record("t1", 7)).await;
        let rm = core.read_request().await;
        assert_eq!(rm["method"], "terminal.rm");
        core.reply(&rm["id"], json!({})).await;
    });
    app.reap_exited();
    // Wait for the transient failure to un-mark the entry, then let the
    // next publish re-queue it.
    tokio::time::timeout(testcore::DEADLINE, async {
        while !app.reaped_exited.lock().unwrap().is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    app.reap_exited();
    tokio::time::timeout(testcore::DEADLINE, server)
        .await
        .unwrap()
        .unwrap();
}
