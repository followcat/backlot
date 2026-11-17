use super::*;
use crate::app::Probe;
use hc_proto::client::AttachConn;
use hc_proto::endpoint::{Endpoint, Local};
use hc_proto::stats::ConnStats;
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::atomic::Ordering::Relaxed;

/// A fresh scratch directory for one test, removed when the test ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("hc-tui-diag-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn endpoint(dir: &Path) -> Endpoint {
    Endpoint::Local(Local {
        session: "hctui-test-diag".into(),
        socket: dir.join("core.sock"),
        holder_socket: dir.join("core.holder.sock"),
        state: dir.to_path_buf(),
        config: dir.join("config.toml"),
    })
}

fn sources(probe: Arc<Probe>, endpoint: &Endpoint, subscription: Arc<ConnStats>) -> Sources {
    Sources::new(
        probe,
        endpoint,
        subscription,
        Arc::default(),
        AttachConn::new(endpoint.clone(), None),
    )
}

fn connection<'a>(snap: &'a Value, kind: &str) -> &'a Value {
    snap["connections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|connection| connection["kind"] == kind)
        .unwrap_or_else(|| panic!("no {kind} connection in {snap}"))
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn snapshot_has_the_documented_shape_and_counts() {
    let dir = Scratch::new("shape");
    let probe = Arc::new(Probe::default());
    probe.wakeups.store(7, Relaxed);
    probe.wake_kinds[1].store(4, Relaxed);
    probe.dirty_marks[1].store(3, Relaxed);
    probe.draws.store(2, Relaxed);
    probe.elided_draws.store(1, Relaxed);
    probe.terminal_writes.store(5, Relaxed);
    probe.terminal_bytes.store(900, Relaxed);
    probe.stdin_reads.store(6, Relaxed);
    probe.stdin_empty_reads.store(1, Relaxed);
    probe.panes[0].store(1, Relaxed);
    probe.panes[2].store(2, Relaxed);
    let subscription = Arc::new(ConnStats::default());
    subscription.connected();
    subscription.connected();
    let endpoint = endpoint(&dir.0);
    let diag = Diag::new(
        dir.0.join("snap.json"),
        sources(probe, &endpoint, subscription),
    );
    let snap = diag.snapshot();
    assert_eq!(snap["schema"], 1, "{snap}");
    assert!(snap["uptime_ms"].is_u64(), "{snap}");
    assert_eq!(snap["loop"]["wakeups"]["total"], 7, "{snap}");
    assert_eq!(snap["loop"]["wakeups"]["input"], 4, "{snap}");
    assert_eq!(snap["loop"]["dirty_marks"]["input"], 3, "{snap}");
    assert_eq!(snap["loop"]["draws"], 2, "{snap}");
    assert_eq!(snap["loop"]["elided_draws"], 1, "{snap}");
    assert_eq!(snap["loop"]["terminal_writes"], 5, "{snap}");
    assert_eq!(snap["loop"]["terminal_bytes"], 900, "{snap}");
    assert_eq!(snap["loop"]["stdin_reads"], 6, "{snap}");
    assert_eq!(snap["loop"]["stdin_empty_reads"], 1, "{snap}");
    assert_eq!(snap["panes"]["by_role"]["active"], 1, "{snap}");
    assert_eq!(snap["panes"]["by_role"]["size_lease"], 2, "{snap}");
    assert_eq!(
        snap["panes"]["channels"],
        json!({"live": 0, "held": 0, "dead": 0}),
        "{snap}"
    );
    assert_eq!(
        snap["endpoints"],
        json!([{"index": 0, "transport": "local"}]),
        "{snap}"
    );
    let subscribed = connection(&snap, "subscription");
    assert_eq!(subscribed["protocol"], "events.subscribe", "{snap}");
    assert_eq!(subscribed["endpoint"], 0, "{snap}");
    assert_eq!(subscribed["connects"], 2, "{snap}");
    assert_eq!(subscribed["reconnects"], 1, "{snap}");
    assert_eq!(connection(&snap, "shared_rpc")["reconnects"], 0, "{snap}");
    assert_eq!(
        connection(&snap, "attach_mux")["protocol"],
        "terminal.attach"
    );
}

#[test]
fn a_write_is_private_and_leaves_only_the_snapshot_behind() {
    let dir = Scratch::new("write");
    let path = dir.0.join("snap.json");
    let endpoint = endpoint(&dir.0);
    let diag = Diag::new(
        path.clone(),
        sources(Arc::new(Probe::default()), &endpoint, Arc::default()),
    );
    diag.write().unwrap();
    diag.write().unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    assert_eq!(read_json(&path)["schema"], 1);
    let names: Vec<_> = std::fs::read_dir(&dir.0)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, vec![std::ffi::OsString::from("snap.json")]);
}

#[test]
fn a_write_into_a_missing_directory_fails_without_panicking() {
    let dir = Scratch::new("missing");
    let endpoint = endpoint(&dir.0);
    let diag = Diag::new(
        dir.0.join("no/such/dir/snap.json"),
        sources(Arc::new(Probe::default()), &endpoint, Arc::default()),
    );
    assert!(diag.write().is_err());
}

#[test]
fn a_clean_exit_writes_the_final_snapshot() {
    let dir = Scratch::new("finish");
    let path = dir.0.join("snap.json");
    let endpoint = endpoint(&dir.0);
    let diag = Diag::new(
        path.clone(),
        sources(Arc::new(Probe::default()), &endpoint, Arc::default()),
    );
    assert!(!path.exists());
    diag.finish();
    assert_eq!(read_json(&path)["schema"], 1);
}

#[test]
fn only_a_non_empty_path_turns_diagnostics_on() {
    let dir = Scratch::new("env");
    let endpoint = endpoint(&dir.0);
    let mut env = Env::new();
    assert!(!requested(&env));
    env.insert(ENV.into(), "".into());
    assert!(!requested(&env));
    let off = Diag::start(
        &env,
        sources(Arc::new(Probe::default()), &endpoint, Arc::default()),
    );
    assert!(off.is_none());
    env.insert(ENV.into(), dir.0.join("snap.json").into_os_string());
    assert!(requested(&env));
}
