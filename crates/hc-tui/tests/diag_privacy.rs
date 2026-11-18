//! A snapshot of a run whose fixtures carry host, user, path, label and
//! title text holds none of it: counters, gauges and protocol ids only.

use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Duration;

use hc_proto::client::AttachConn;
use hc_proto::endpoint::{Endpoint, remote};
use hc_proto::stats::ConnStats;
use hc_proto::watch::{OpenRpc, WatchParams, Watcher};
use hc_tui::app::{App, Probe, UiEvent};
use hc_tui::diag::{Diag, Sources};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::{Value, json};
use tokio::sync::mpsc;

#[path = "common/loopcore.rs"]
mod loopcore;

/// Fixture text that must never reach a snapshot: the `--remote` target,
/// the machine and user names, the working directory and the title.
const SECRETS: [&str; 5] = ["alice", "host-a", "acme-shop", "/home/user", "deploy"];

fn fixture_terminal(address: &str, tab: &str, created_ms: u64) -> Value {
    json!({
        "address": address,
        "title": "alice: deploy on host-a",
        "labels": {"herdr.space": "acme-shop", "herdr.tab": tab},
        "argv": ["sh"],
        "cwd": "/home/user/acme-shop",
        "state": "live",
        "created_ms": created_ms,
        "screen": "exact",
        "cols": 80,
        "rows": 24,
    })
}

#[tokio::test]
async fn a_snapshot_of_a_run_with_fixture_secrets_holds_none_of_them() {
    let (sub_tx, mut subs) = mpsc::channel(4);
    let (attach_tx, mut attaches) = mpsc::channel(8);
    let rpc: OpenRpc = Arc::new(|| Box::pin(std::future::pending()));
    let watcher = Watcher::with_openers(rpc, loopcore::sub_opens(sub_tx), WatchParams::default());
    let config = hc_tui::config::Config::default();
    let shell = hc_tui::chrome::ClientShellConfig::from_config(
        &config,
        None,
        None,
        std::path::PathBuf::new(),
    );
    let probe = Arc::new(Probe {
        publish_panes: true,
        ..Probe::default()
    });
    let mut app = App::new(
        watcher,
        loopcore::attach_opens(attach_tx),
        config.live_keybinds(),
        shell,
        Arc::clone(&probe),
    );
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let (ui_tx, mut ui) = mpsc::channel(8);
    let (_reply_tx, mut replies) = mpsc::channel(1);
    let counters = Arc::clone(&probe);
    let driver = tokio::spawn(async move {
        let _session = loopcore::Session::connect(
            &mut subs,
            &mut attaches,
            4,
            json!({
                "seq": 1,
                "terminals": [
                    fixture_terminal("t1", "deploy", 1),
                    fixture_terminal("t2", "deploy", 2),
                ],
            }),
        )
        .await;
        let settled = loopcore::settled_draws(&counters).await;
        ui_tx
            .send(UiEvent::InputRejected("counted input"))
            .await
            .unwrap();
        loopcore::draws_beyond(&counters, settled).await;
        drop(ui_tx);
    });
    tokio::time::timeout(
        Duration::from_secs(15),
        app.run(&mut terminal, &mut ui, &mut replies),
    )
    .await
    .expect("the loop must stay responsive")
    .unwrap();
    driver.await.unwrap();
    let panes: Vec<u64> = probe.panes.iter().map(|p| p.load(Relaxed)).collect();
    assert_eq!(
        panes,
        vec![1, 1, 2, 0],
        "the fixtures never reached the loop"
    );

    let endpoint = Endpoint::Remote(remote("alice@host-a", "acme-shop").unwrap());
    let sources = Sources::new(
        Arc::clone(&probe),
        &endpoint,
        Arc::new(ConnStats::default()),
        Arc::new(ConnStats::default()),
        AttachConn::new(endpoint.clone(), None),
    );
    let dir = std::env::temp_dir().join(format!("hc-tui-diag-privacy-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("snap.json");
    Diag::new(path.clone(), sources).write().unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    for secret in SECRETS {
        assert!(!text.contains(secret), "snapshot holds {secret:?}: {text}");
    }
    let snap: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        snap["endpoints"],
        json!([{"index": 0, "transport": "remote"}])
    );
}
