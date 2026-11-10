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
use backlot_model::state::{RoleLifecycle, StudioState};
use backlot_model::{EventBy, NewEvent};
use hc_proto::client::{CallError, Client};
use hc_proto::codec::Reader;
use hc_proto::frame::{Frame, Kind};
use hc_proto::jsonrpc::RpcError;
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, DuplexStream};

use crate::cli::Fail;
use crate::daemon::{Inventory, Stage, keep_or_fail, resolved_base};
use crate::reconcile::Action;
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

/// The `hello` reply the fake cores share — `methods` is what the
/// client offers; a list naming less than everything makes the
/// missing calls `Unsupported` before they reach the wire.
fn hello_reply(methods: &[&str]) -> Vec<u8> {
    serde_json::to_string(&json!({
        "jsonrpc": "2.0", "id": 0,
        "result": {
            "generation": 1, "version": "fake-core 0.0",
            "framed": true,
            "core": {"id": "core-fake-1", "name": "fake-core"},
            "methods": methods,
            "features": [],
        }
    }))
    .unwrap()
    .into_bytes()
}

const ALL_METHODS: &[&str] = &[
    "terminal.run",
    "terminal.kill",
    "terminal.rm",
    "terminal.read",
    "terminal.list",
    "terminal.send",
    "terminal.keys",
    "events.subscribe",
];

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
    write.write_all(&hello_reply(ALL_METHODS)).await.unwrap();
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

/// `hello` with every method, then silence: each call hangs until the
/// client's own deadline, producing a `Timeout` — a refused pass,
/// not a dead wire.
async fn serve_mute(stream: DuplexStream) {
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = Reader::new(read);
    let hello = reader.read_line().await.unwrap().unwrap();
    let request: Value = serde_json::from_slice(&hello).unwrap();
    assert_eq!(request["method"], "hello");
    write.write_all(&hello_reply(ALL_METHODS)).await.unwrap();
    write.write_all(b"\n").await.unwrap();
    while let Ok(Some(_)) = reader.read_frame().await {}
}

/// `hello`, then the wire dies: the next call is `Closed`, the one
/// error that still ends the stage.
async fn serve_drop(stream: DuplexStream) {
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = Reader::new(read);
    let hello = reader.read_line().await.unwrap().unwrap();
    let request: Value = serde_json::from_slice(&hello).unwrap();
    assert_eq!(request["method"], "hello");
    write.write_all(&hello_reply(ALL_METHODS)).await.unwrap();
    write.write_all(b"\n").await.unwrap();
    // `write` drops when this returns — the client's next call is Closed.
}

/// `hello` offering only `terminal.list` — every `terminal.run` is
/// `Unsupported`, a refused pass that never touches the ledger.
async fn serve_no_run(stream: DuplexStream) {
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = Reader::new(read);
    let hello = reader.read_line().await.unwrap().unwrap();
    let request: Value = serde_json::from_slice(&hello).unwrap();
    assert_eq!(request["method"], "hello");
    write
        .write_all(&hello_reply(&["terminal.list"]))
        .await
        .unwrap();
    write.write_all(b"\n").await.unwrap();
    while let Ok(Some(_)) = reader.read_frame().await {}
}

/// A client on a duplex wire to `serve`.
async fn client_on<F, Fut>(serve: F) -> Client
where
    F: FnOnce(DuplexStream) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move { serve(core_side).await });
    let (read, write) = tokio::io::split(client_side);
    Client::open(Box::pin(read), Box::pin(write), None)
        .await
        .unwrap()
}

/// A connected stage on a duplex wire to the fake core `serve` runs.
async fn stage_on<F, Fut>(serve: F) -> Stage
where
    F: FnOnce(DuplexStream) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    Stage::for_test(client_on(serve).await)
}

async fn stage() -> Stage {
    stage_on(serve).await
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
                    nudge_after_s: 180,
                    escalate_after_s: 600,
                    check_env: Vec::new(),
                    checks: Vec::new(),
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
    assert!(
        error
            .as_deref()
            .is_some_and(|e| e.starts_with("events.jsonl")),
        "the error leads with the reason, not the studio: {error:?}"
    );

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

/// `Fail::call`: only a dead wire — `Closed` or `Protocol` — maps
/// to `Core`; every other `CallError` is one studio's refused pass.
#[test]
fn only_a_dead_wire_is_core() {
    for (e, core) in [
        (CallError::Closed, true),
        (CallError::Protocol("bad frame".into()), true),
        (
            CallError::Rpc(RpcError::new(-32000, "usage", "refused", "ask again")),
            false,
        ),
        (CallError::Timeout, false),
        (
            CallError::Unsupported {
                method: "terminal.run".into(),
                reason: "not offered".into(),
            },
            false,
        ),
        (CallError::TooLarge { size: 4_000_000 }, false),
        (CallError::Full, false),
        (
            CallError::Shape(serde_json::from_str::<Value>("nope").unwrap_err()),
            false,
        ),
    ] {
        match (Fail::call(e), core) {
            (Fail::Core(_), true) | (Fail::Refused(_), false) => {}
            (other, want) => panic!("wrong verdict ({want} core wanted): {other:?}"),
        }
    }
}

/// The refresh decision `run` applies each pass: a dead wire exits
/// the stage; any other failure keeps the last inventory for the
/// next refresh.
#[test]
fn a_failed_refresh_keeps_the_last_inventory() {
    let inv = |n: usize| Inventory {
        terms: (0..n)
            .map(|i| crate::reconcile::LiveTerminal {
                name: format!("t{i}"),
                address: format!("t{i}"),
                argv: Vec::new(),
                cwd: String::new(),
                labels: BTreeMap::new(),
            })
            .collect(),
        doing: BTreeMap::new(),
    };
    assert!(matches!(
        keep_or_fail(Err(Fail::core("gone")), inv(0)),
        Err(Fail::Core(_))
    ));
    let kept = keep_or_fail(Err(Fail::refused("busy")), inv(2)).unwrap();
    assert_eq!(kept.terms.len(), 2);
    let fresh = keep_or_fail(Ok(inv(3)), inv(0)).unwrap();
    assert_eq!(fresh.terms.len(), 3);
}

/// A `terminal.run` that never answers is the client's `Timeout` —
/// `Fail::call` refuses it for that studio, the tick walks past and
/// finishes: were it `Core`, `tick` would have errored before
/// reaching `good`.
#[tokio::test(start_paused = true)]
async fn a_timed_out_call_does_not_end_the_tick() {
    let base = tmp();
    studio(&base, "deaf");
    studio(&base, "good");
    let mut stage = stage_on(serve_mute).await;
    stage.base = base;
    let live = Inventory {
        terms: Vec::new(),
        doing: BTreeMap::new(),
    };
    // `Stage` is !Sync, so the tick stays on this task while a
    // helper walks the paused clock past each call deadline.
    let advancer = tokio::spawn(async {
        loop {
            tokio::time::advance(std::time::Duration::from_secs(130)).await;
            tokio::task::yield_now().await;
        }
    });
    stage
        .tick(&live)
        .await
        .expect("a muted call ended the tick");
    advancer.abort();
}

/// `hello`, then the wire dies: the next call is `Closed`, the one
/// error that still ends the stage.
#[tokio::test]
async fn a_dead_wire_ends_the_tick() {
    let base = tmp();
    studio(&base, "dead");
    let mut stage = stage_on(serve_drop).await;
    stage.base = base;
    let live = Inventory {
        terms: Vec::new(),
        doing: BTreeMap::new(),
    };
    let err = stage.tick(&live).await.unwrap_err();
    assert!(matches!(err, Fail::Core(_)), "{err:?}");
}

/// A refused pass shows in the summary's `error` until a pass
/// succeeds — then the slot clears.
#[tokio::test]
async fn a_failing_pass_shows_then_clears() {
    let base = tmp();
    studio(&base, "acme");
    let live = Inventory {
        terms: Vec::new(),
        doing: BTreeMap::new(),
    };
    let mut stage = stage_on(serve_no_run).await;
    stage.base = base;
    stage.tick(&live).await.unwrap();
    let error = stage
        .summary_view(&BTreeMap::new())
        .studios
        .iter()
        .find(|st| st.id == "acme")
        .and_then(|st| st.error.clone());
    assert!(
        error.as_deref().unwrap_or("").contains("terminal.run"),
        "{error:?}"
    );

    // The wire heals: `terminal.run` gets a refusal answer now and
    // the pass succeeds — the stale error clears.
    stage.client = client_on(serve).await;
    stage.tick(&live).await.unwrap();
    let error = stage
        .summary_view(&BTreeMap::new())
        .studios
        .iter()
        .find(|st| st.id == "acme")
        .and_then(|st| st.error.clone());
    assert_eq!(error, None);
}

/// A completed check goes through `check`'s finish branch: the run
/// leaves `checking` and `task.checked` lands. The borrow pattern
/// there once removed the run while its `RefMut` was still held.
#[tokio::test]
async fn a_finished_check_is_recorded_through_the_pass() {
    use backlot_model::testkit as tk;

    /// A child that has already exited 0.
    struct Done;
    impl crate::checkrun::CheckChild for Done {
        fn poll(&mut self) -> Option<crate::checkrun::PollOut> {
            Some(crate::checkrun::PollOut {
                code: Some(0),
                output: b"clean".to_vec(),
            })
        }
        fn term(&mut self) {}
        fn kill(&mut self) {}
    }

    let base = tmp();
    let dir = store::studio_dir(&base, "acme");
    fs::create_dir_all(&dir).unwrap();
    // The cast declares `unit`; t1 is cast and delivered at seq 5.
    let mut created = tk::studio_kind();
    let EventKind::StudioCreated { checks, .. } = &mut created else {
        unreachable!()
    };
    *checks = vec![tk::check_decl("unit", "true", 60)];
    for (role, kind) in [
        ("user", created),
        (
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "first".into(),
            },
        ),
        ("director", tk::task_created("t1", "feature", 1)),
        ("director", tk::cast("t1", "actor-a", "a")),
        ("actor-t1-c1", tk::delivered("t1", 1, "a", 1)),
    ] {
        store::append(&dir, |_, _| {
            Ok::<_, std::convert::Infallible>(Pending {
                event: NewEvent::new(EventBy::role(role), kind),
                files: Vec::new(),
            })
        })
        .unwrap();
    }
    let state = store::load_state(&dir).unwrap();
    let stage = stage().await;
    let run = crate::checkwork::Running::for_test(Box::new(Done), 60_000, dir.join("gone"));
    stage.checking.borrow_mut().insert("acme".into(), run);
    stage.check("acme", &dir, &state, 10_000).unwrap();
    assert!(stage.checking.borrow().is_empty(), "the run is spent");
    let state = store::load_state(&dir).unwrap();
    let r = &state.tasks["t1"].checked[0];
    assert_eq!((r.name.as_str(), r.passed()), ("unit", true));
}

/// A check still in flight keeps the studio's slot: the pass
/// polls it and leaves it alone — the same owed check is not
/// spawned again every tick with the live run orphaned.
#[tokio::test]
async fn a_running_check_is_polled_not_respawned() {
    use backlot_model::testkit as tk;
    use std::sync::{Arc, Mutex};

    /// A child that never exits.
    struct Hang;
    impl crate::checkrun::CheckChild for Hang {
        fn poll(&mut self) -> Option<crate::checkrun::PollOut> {
            None
        }
        fn term(&mut self) {}
        fn kill(&mut self) {}
    }

    let base = tmp();
    let dir = store::studio_dir(&base, "acme");
    fs::create_dir_all(&dir).unwrap();
    let mut created = tk::studio_kind();
    let EventKind::StudioCreated { checks, .. } = &mut created else {
        unreachable!()
    };
    *checks = vec![tk::check_decl("unit", "sleep 60", 60)];
    for (role, kind) in [
        ("user", created),
        (
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "first".into(),
            },
        ),
        ("director", tk::task_created("t1", "feature", 1)),
        ("director", tk::cast("t1", "actor-a", "a")),
        ("actor-t1-c1", tk::delivered("t1", 1, "a", 1)),
    ] {
        store::append(&dir, |_, _| {
            Ok::<_, std::convert::Infallible>(Pending {
                event: NewEvent::new(EventBy::role(role), kind),
                files: Vec::new(),
            })
        })
        .unwrap();
    }
    let state = store::load_state(&dir).unwrap();
    let spawns = Arc::new(Mutex::new(0usize));
    let mut stage = stage().await;
    {
        let spawns = Arc::clone(&spawns);
        stage.spawn_check = Box::new(move |_| {
            *spawns.lock().unwrap() += 1;
            Ok(Box::new(Hang))
        });
    }
    // The slot is already taken by a live run — three ticks must
    // not spawn or replace it.
    let run = crate::checkwork::Running::for_test(Box::new(Hang), 60_000, dir.join("gone"));
    stage.checking.borrow_mut().insert("acme".into(), run);
    for now in [10_000, 11_000, 12_000] {
        stage.check("acme", &dir, &state, now).unwrap();
    }
    assert_eq!(*spawns.lock().unwrap(), 0, "a live run was respawned");
    let map = stage.checking.borrow();
    assert_eq!(map["acme"].name, "unit", "the live run was replaced");
}

/// The studio's first `check` pass sweeps the run trees a dead
/// stage left — process included — while the owed delivery's tree
/// stays for the run about to start.
#[tokio::test]
async fn the_first_check_pass_prunes_stale_run_trees() {
    use backlot_model::testkit as tk;

    let base = tmp();
    let dir = store::studio_dir(&base, "acme");
    fs::create_dir_all(&dir).unwrap();
    let mut created = tk::studio_kind();
    let EventKind::StudioCreated { checks, .. } = &mut created else {
        unreachable!()
    };
    *checks = vec![tk::check_decl("unit", "true", 60)];
    for (role, kind) in [
        ("user", created),
        (
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "first".into(),
            },
        ),
        ("director", tk::task_created("t1", "feature", 1)),
        ("director", tk::cast("t1", "actor-a", "a")),
        ("actor-t1-c1", tk::delivered("t1", 1, "a", 1)),
    ] {
        store::append(&dir, |_, _| {
            Ok::<_, std::convert::Infallible>(Pending {
                event: NewEvent::new(EventBy::role(role), kind),
                files: Vec::new(),
            })
        })
        .unwrap();
    }
    // A dead stage's leftovers: a stale tree with a process still
    // parked in it, and the owed delivery's own tree.
    let stale = dir.join("checks/run-99");
    let owed_run = dir.join("checks/run-5");
    fs::create_dir_all(&stale).unwrap();
    fs::create_dir_all(&owed_run).unwrap();
    let mut sleeper = std::process::Command::new("sleep")
        .arg("300")
        .current_dir(&stale)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    for _ in 0..200 {
        if !crate::checkwork::cwd_pids(&stale).is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(!crate::checkwork::cwd_pids(&stale).is_empty());

    let state = store::load_state(&dir).unwrap();
    let mut stage = stage().await;
    // `worktree remove` declines: the fallback delete is real.
    *stage.git.borrow_mut() = Box::new(|argv, _cwd| crate::workareas::GitOut {
        code: if argv.contains(&"remove") { 1 } else { 0 },
        stdout: String::new(),
        stderr: "git declined".into(),
    });
    struct Hang;
    impl crate::checkrun::CheckChild for Hang {
        fn poll(&mut self) -> Option<crate::checkrun::PollOut> {
            None
        }
        fn term(&mut self) {}
        fn kill(&mut self) {}
    }
    stage.spawn_check = Box::new(|_| Ok(Box::new(Hang)));
    stage.check("acme", &dir, &state, 10_000).unwrap();

    assert!(!stale.exists(), "the stale run tree survived");
    for _ in 0..200 {
        if sleeper.try_wait().unwrap().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let status = sleeper.wait().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(status.signal(), Some(libc::SIGKILL), "{status}");
    }
    assert!(!status.success(), "{status}");
    assert!(
        stage.checking.borrow().contains_key("acme"),
        "the owed check never started"
    );
    // Pruning is once per studio: with the slot free again a new
    // stale tree survives the next pass untouched.
    stage.checking.borrow_mut().remove("acme");
    fs::create_dir_all(&stale).unwrap();
    stage.check("acme", &dir, &state, 11_000).unwrap();
    assert!(stale.exists(), "a later pass re-pruned");
    assert!(stage.checking.borrow().contains_key("acme"));
}

/// `Action::Resumed` records `role.resumed` with the live
/// terminal's address and the recorded session — a record only,
/// no `terminal.run`.
#[tokio::test]
async fn act_records_resumed_with_the_live_address() {
    let base = tmp();
    let dir = studio(&base, "acme");
    let stage = stage().await;
    let mut free = true;
    stage
        .act(
            "acme",
            &dir,
            &StudioState::default(),
            &Action::Resumed {
                role: "producer".into(),
                address: "t9".into(),
                session: "sess-1".into(),
            },
            &mut free,
            &std::collections::BTreeSet::new(),
        )
        .await
        .unwrap();
    let state = store::load_state(&dir).unwrap();
    let role = &state.roles["producer"];
    assert_eq!(role.lifecycle(), RoleLifecycle::Live);
    assert_eq!(role.terminal.as_deref(), Some("t9"));
    assert_eq!(role.session_id.as_deref(), Some("sess-1"));
    assert_eq!(role.recovered_seq, Some(2), "the recovered notice is owed");
}
