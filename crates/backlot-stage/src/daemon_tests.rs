//! The stage's startup path handling: a state base reached through
//! a symlink resolves to the real path the OS reports to terminals
//! (`term.cwd`) and git (`worktree list`), and a base that cannot
//! resolve keeps the path as given. The tick tests prove one bad
//! studio — an unreadable ledger or a failing pass — is skipped
//! while the rest of the stage carries on.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use backlot_model::event::EventKind;
use backlot_model::{EventBy, NewEvent};
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use hc_proto::frame::{Frame, Kind};
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, DuplexStream};

use crate::daemon::{Inventory, Stage, resolved_base};
use crate::store::{self, Pending};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn tmp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "backlot-daemon-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A symlinked base is created first when missing, then resolved to
/// the same path the OS reports — so every path downstream compares
/// equal to a terminal's cwd and a worktree listing.
#[test]
#[cfg(unix)]
fn state_base_resolves_through_a_symlink() {
    let root = tmp();
    // Missing first: created, then resolved.
    let fresh = root.join("fresh");
    assert_eq!(resolved_base(fresh.clone()), fresh.canonicalize().unwrap());
    assert!(fresh.is_dir());
    // Reached through a symlink — macOS's /var tmp: the real path.
    let real = root.join("real");
    fs::create_dir_all(&real).unwrap();
    let link = root.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let resolved = resolved_base(link.clone());
    assert_eq!(resolved, real.canonicalize().unwrap());
    // A base that cannot resolve keeps the path as given.
    let dangling = root.join("dangling");
    std::os::unix::fs::symlink(root.join("gone"), &dangling).unwrap();
    assert_eq!(resolved_base(dangling.clone()), dangling);
}

/// Serve `hello` offering every terminal method, then answer each
/// request with a JSON-RPC refusal — an `Rpc` verdict is the
/// tolerated one, so a launch lands `launch refused` instead of a
/// dead-wire `Fail::Core`.
async fn serve(stream: DuplexStream) {
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = Reader::new(read);
    let hello = reader.read_line().await.unwrap().unwrap();
    let request: Value = serde_json::from_slice(&hello).unwrap();
    assert_eq!(request["method"], "hello");
    write
        .write_all(
            serde_json::to_string(&json!({
                "jsonrpc": "2.0", "id": 0,
                "result": {
                    "generation": 1, "version": "fake-core 0.0",
                    "framed": true,
                    "core": {"id": "core-fake-1", "name": "fake-core"},
                    "methods": [
                        "terminal.run", "terminal.kill", "terminal.rm",
                        "terminal.read", "terminal.list", "terminal.send",
                        "terminal.keys", "events.subscribe",
                    ],
                    "features": [],
                }
            }))
            .unwrap()
            .as_bytes(),
        )
        .await
        .unwrap();
    write.write_all(b"\n").await.unwrap();
    while let Ok(Some(frame)) = reader.read_frame().await {
        if frame.channel != 0 || frame.kind != Kind::Data {
            continue;
        }
        let request: Value = serde_json::from_slice(&frame.payload).unwrap();
        let reply = serde_json::to_vec(&json!({
            "jsonrpc": "2.0", "id": request["id"],
            "error": {"code": -32000, "message": "refused"},
        }))
        .unwrap();
        write
            .write_all(&Frame::data(0, reply).encode())
            .await
            .unwrap();
    }
}

/// A connected stage on a duplex wire to the refusing fake core.
async fn stage() -> Stage {
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    tokio::spawn(serve(core_side));
    let (read, write) = tokio::io::split(client_side);
    let client = Client::open(Box::pin(read), Box::pin(write), None)
        .await
        .unwrap();
    Stage::for_test(client)
}

/// `studios/<name>` holding a bare `studio.created` — a fresh
/// reconcile wants the fixed roles, and `act` lands the `launch
/// refused` attention the fake core's refusals owe.
fn studio(base: &Path, name: &str) -> PathBuf {
    let dir = store::studio_dir(base, name);
    fs::create_dir_all(&dir).unwrap();
    let repo = dir.join("repo");
    fs::create_dir_all(&repo).unwrap();
    store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("user"),
                EventKind::StudioCreated {
                    title: name.into(),
                    repo: repo.to_string_lossy().into_owned(),
                    mode: backlot_model::event::StudioMode::Standard,
                    roles: Box::new(backlot_model::event::RoleConfigs {
                        producer: actor(),
                        director: actor(),
                        writer: actor(),
                    }),
                    candidates: Vec::new(),
                    rework_cap: 2,
                    stall_limit_s: 600,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    dir
}

fn actor() -> backlot_model::event::ActorIdentity {
    backlot_model::event::ActorIdentity {
        runner: "fake".into(),
        runner_version: None,
        model: "plain".into(),
        effort: "high".into(),
        permission_profile: "standard".into(),
    }
}

/// Whether the ledger shows the studio reconciled: `act` raised the
/// attention its refused launches owe.
fn reconciled(dir: &Path) -> bool {
    store::load_state(dir)
        .unwrap()
        .attention
        .iter()
        .any(|a| a.reason == "launch refused")
}

/// A ledger that cannot be parsed marks its studio broken: the tick
/// skips it but reconciles the good studio beside it, the summary
/// shows the error, and repairing the file brings it back.
#[tokio::test]
async fn a_corrupt_ledger_is_skipped_until_it_loads() {
    let base = tmp();
    let good = studio(&base, "good");
    let bad = store::studio_dir(&base, "bad");
    fs::create_dir_all(&bad).unwrap();
    fs::write(bad.join("events.jsonl"), b"not json\n").unwrap();
    let mut stage = stage().await;
    stage.base = base.clone();
    let live = Inventory {
        terms: Vec::new(),
        doing: BTreeMap::new(),
    };

    stage.tick(&live).await.unwrap();
    assert!(reconciled(&good), "the good studio was not reconciled");
    let error = stage
        .summary_view(&live.doing)
        .studios
        .into_iter()
        .find(|st| st.id == "bad")
        .and_then(|st| st.error);
    assert!(error.is_some(), "the broken studio shows no error");

    // The person repairs the file; the next tick picks it up again.
    fs::remove_file(bad.join("events.jsonl")).unwrap();
    let bad = studio(&base, "bad");
    stage.tick(&live).await.unwrap();
    let entry = stage
        .summary_view(&live.doing)
        .studios
        .into_iter()
        .find(|st| st.id == "bad")
        .unwrap();
    assert_eq!(entry.error, None, "{entry:?}");
    assert!(reconciled(&bad), "the repaired studio never reconciled");
}

/// A broken ledger is retried every tick: corrupting then repairing
/// its first byte in place — the file's length never changes —
/// still brings the studio back on the next pass.
#[tokio::test]
async fn a_repaired_ledger_recovers_at_the_same_length() {
    let base = tmp();
    let bad = studio(&base, "bad");
    let file = bad.join("events.jsonl");
    let good_bytes = fs::read(&file).unwrap();
    let mut corrupt = good_bytes.clone();
    corrupt[0] = b'x';
    fs::write(&file, &corrupt).unwrap();
    let mut stage = stage().await;
    stage.base = base.clone();
    let live = Inventory {
        terms: Vec::new(),
        doing: BTreeMap::new(),
    };

    stage.tick(&live).await.unwrap();
    let error = stage
        .summary_view(&live.doing)
        .studios
        .into_iter()
        .find(|st| st.id == "bad")
        .and_then(|st| st.error);
    assert!(error.is_some(), "the broken studio shows no error");

    // Same length, byte for byte except the repair: without the
    // broken retry this studio would stay skipped until an append.
    fs::write(&file, &good_bytes).unwrap();
    stage.tick(&live).await.unwrap();
    let entry = stage
        .summary_view(&live.doing)
        .studios
        .into_iter()
        .find(|st| st.id == "bad")
        .unwrap();
    assert_eq!(entry.error, None, "{entry:?}");
    assert!(reconciled(&bad), "the repaired studio never reconciled");
}

/// A studio whose pass fails mid-tick — its ledger replays but the
/// append an action owes is refused — logs once and the tick moves
/// on to the next studio.
#[tokio::test]
async fn a_failing_pass_does_not_end_the_tick() {
    let base = tmp();
    // `deaf` sorts first: if its error were fatal `good` never runs.
    let deaf = studio(&base, "deaf");
    let good = studio(&base, "good");
    let mut perms = fs::metadata(deaf.join("events.jsonl"))
        .unwrap()
        .permissions();
    perms.set_readonly(true);
    fs::set_permissions(deaf.join("events.jsonl"), perms).unwrap();
    let mut stage = stage().await;
    stage.base = base.clone();
    let live = Inventory {
        terms: Vec::new(),
        doing: BTreeMap::new(),
    };

    stage.tick(&live).await.unwrap();
    assert!(reconciled(&good), "the tick stopped at the failing studio");
}
