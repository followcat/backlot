//! D4 liveness evidence and stall tests: spinner normalisation only, busy
//! child (injected + real Linux CPU loop), kernel lock wait (3x + waiting on),
//! exited child `cutime` growth, root floor bounds, work-area & ledger events,
//! per-source clearing, and remote probe suppression.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use backlot_model::event::{ActorIdentity, EventKind, STALLED};
use backlot_model::{Event, EventBy, NewEvent};
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use hc_proto::frame::{Frame, Kind};
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, DuplexStream};

use crate::daemon::Stage;
use crate::liveness::{ROOT_CPU_FLOOR_JIFFIES_PER_SEC, ROOT_IO_FLOOR_BYTES_PER_SEC};
use crate::liveness_diff::WorkAreaSnap;
use crate::liveness_probe::ProcInfo;
use crate::reconcile::LiveTerminal;
use crate::sample::Watched;
use crate::store::{self, Pending};
use crate::test_support::TestDir;

fn tmp() -> TestDir {
    TestDir::new("backlot-liveness-test")
}

#[derive(Default)]
struct FakeCore {
    screen: String,
}

async fn serve(stream: DuplexStream, fake: Arc<Mutex<FakeCore>>) {
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = Reader::new(read);
    let hello = reader.read_line().await.unwrap().unwrap();
    let req: Value = serde_json::from_slice(&hello).unwrap();
    assert_eq!(req["method"], "hello");
    write
        .write_all(
            serde_json::to_string(&json!({
                "jsonrpc": "2.0", "id": 0,
                "result": {
                    "generation": 1, "version": "fake-core 0.0", "framed": true,
                    "core": {"id": "core-fake-1", "name": "fake-core"},
                    "methods": ["terminal.read", "terminal.keys", "terminal.send"],
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
        let req: Value = serde_json::from_slice(&frame.payload).unwrap();
        let result = match req["method"].as_str().unwrap_or("") {
            "terminal.read" => json!({
                "text": fake.lock().unwrap().screen,
                "complete": true,
                "how": "screen",
            }),
            _ => json!({}),
        };
        let reply = serde_json::to_vec(&json!({
            "jsonrpc": "2.0", "id": req["id"], "result": result,
        }))
        .unwrap();
        write
            .write_all(&Frame::data(0, reply).encode())
            .await
            .unwrap();
    }
}

async fn make_stage() -> (Stage, Arc<Mutex<FakeCore>>) {
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    let fake = Arc::new(Mutex::new(FakeCore::default()));
    tokio::spawn(serve(core_side, Arc::clone(&fake)));
    let (read, write) = tokio::io::split(client_side);
    let client = Client::open(Box::pin(read), Box::pin(write), None)
        .await
        .unwrap();
    (Stage::for_test(client), fake)
}

fn actor_cfg() -> ActorIdentity {
    ActorIdentity {
        runner: "claude".into(),
        runner_version: None,
        model: "sonnet".into(),
        effort: Some("high".into()),
        permission_profile: "auto".into(),
    }
}

fn setup_studio(dir: &std::path::Path, limit_s: u32) {
    let cand = backlot_model::event::Candidate {
        name: "actor-a".into(),
        runner: "claude".into(),
        runner_version: None,
        model: "sonnet".into(),
        effort: Some("high".into()),
        permission_profile: "auto".into(),
    };
    for kind in [
        EventKind::StudioCreated {
            title: "acme".into(),
            repo: dir.join("repo").to_string_lossy().into_owned(),
            members: Vec::new(),
            no_git: true,
            mode: backlot_model::event::StudioMode::Standard,
            roles: Box::new(backlot_model::event::RoleConfigs {
                producer: actor_cfg(),
                director: actor_cfg(),
                writer: actor_cfg(),
            }),
            candidates: vec![cand],
            rework_cap: 3,
            stall_limit_s: limit_s,
            nudge_after_s: 300,
            escalate_after_s: 600,
            check_env: Vec::new(),
            checks: Vec::new(),
            task_budget_s: None,
            limits: Default::default(),
            max_actors: backlot_model::event::default_max_actors(),
            push: Default::default(),
            design_gate: backlot_model::event::DesignGate::Off,
        },
        EventKind::ReqVersion {
            version: 1,
            file: "req/v1.md".into(),
            summary: "v1".into(),
        },
        EventKind::TaskCreated {
            task: "t1".into(),
            title: "cart".into(),
            task_kind: "feature".into(),
            acceptance: "plan/a.md".into(),
            requirement_version: 1,
            dependencies: Vec::new(),
        },
        EventKind::TaskCast {
            task: "t1".into(),
            candidate: "actor-a".into(),
            reason: "go".into(),
            actor: actor_cfg(),
            priority: None,
        },
        EventKind::RoleStarted {
            role: "actor-t1-c1".into(),
            terminal: Some("t1".into()),
            session_id: Some("s1".into()),
            actor: None,
            reason: None,
        },
    ] {
        store::append(dir, |_, _| {
            Ok::<_, std::convert::Infallible>(Pending {
                event: NewEvent::new(EventBy::role("director"), kind),
                files: Vec::new(),
            })
        })
        .unwrap();
    }
}

fn events(dir: &std::path::Path, kind: &str) -> Vec<Event> {
    store::load_events(dir)
        .unwrap()
        .0
        .into_iter()
        .filter(|e| e.kind.name() == kind)
        .collect()
}

fn term(pid: Option<u32>) -> LiveTerminal {
    LiveTerminal {
        name: "bl-acme-t1".into(),
        address: "t1".into(),
        pid,
        status: Some(hc_proto::terminal::Status {
            state: "working".into(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

async fn tick(stage: &Stage, dir: &std::path::Path, t: &LiveTerminal, runner: &str, now_ms: u64) {
    let state = store::load_state(dir).unwrap();
    let w = Watched {
        dir,
        state: &state,
        role: "actor-t1-c1",
        runner,
        term: t,
    };
    stage.sample_work("acme", &w, now_ms).await.unwrap();
}

fn proc_node(pid: u32, ppid: u32, depth: u32, cpu: u64, io: u64, cmd: &str) -> ProcInfo {
    ProcInfo {
        pid,
        ppid,
        depth,
        cpu_jiffies: cpu,
        child_cpu_jiffies: 0,
        io_bytes: io,
        argv: cmd.into(),
        wchan: String::new(),
    }
}

/// 1. Spinner/timer animations only: stalls past limit naming `animates only`
/// while raw `Stage.screens` tracks keep updating for the send gate.
#[tokio::test]
async fn spinner_only_stalls_with_animates_only_while_raw_screen_updates() {
    let dir = tmp();
    setup_studio(&dir, 30);
    let (stage, fake) = make_stage().await;
    let t = term(None);
    let glyphs = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

    for sec in 0..31u64 {
        let g = glyphs[(sec as usize) % glyphs.len()];
        fake.lock().unwrap().screen = format!("{g} Running command... {sec}s · ↑ 1.2k tokens\n");
        tick(&stage, &dir, &t, "agy", 1_000 + sec * 1_000).await;
        if sec < 30 {
            assert!(events(&dir, "attention.raised").is_empty());
        }
    }
    let raw_track = stage
        .screens
        .borrow()
        .get(&("acme".into(), "actor-t1-c1".into()))
        .cloned()
        .unwrap();
    assert_eq!(raw_track.same_since_ms, 31_000);

    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 1);
    match &raised[0].kind {
        EventKind::AttentionRaised { reason, detail, .. } => {
            assert_eq!(reason, STALLED);
            assert!(detail.contains("animates only"), "{detail}");
            assert!(detail.contains("attention/"), "{detail}");
        }
        _ => unreachable!(),
    }

    fake.lock().unwrap().screen = "⠋ Running command... 99s · ↑ 9k tokens\n".into();
    tick(&stage, &dir, &t, "agy", 32_000).await;
    assert!(events(&dir, "attention.cleared").is_empty());
}

/// 2. Busy child process keeps role alive past 2x limit with frozen screen.
#[tokio::test]
async fn busy_child_survives_two_limits_then_stalls_when_frozen() {
    let dir = tmp();
    setup_studio(&dir, 30);
    let tree = Arc::new(Mutex::new(vec![
        proc_node(100, 1, 0, 100, 0, "agy"),
        proc_node(101, 100, 1, 50, 0, "cargo test"),
    ]));
    let tree_r = Arc::clone(&tree);
    let (mut stage, fake) = make_stage().await;
    stage.proc_read = Box::new(move |_| tree_r.lock().unwrap().clone());
    fake.lock().unwrap().screen = "working\n".into();
    let t = term(Some(100));

    for step in 0..=6u64 {
        tree.lock().unwrap()[1].cpu_jiffies = 50 + step * 500;
        tick(&stage, &dir, &t, "agy", 1_000 + step * 10_000).await;
    }
    assert!(events(&dir, "attention.raised").is_empty());

    tick(&stage, &dir, &t, "agy", 91_000).await;
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 1);
    match &raised[0].kind {
        EventKind::AttentionRaised { detail, .. } => {
            assert!(detail.contains("cargo test (CPU)"), "{detail}");
            assert!(detail.contains("no sign of life for 30s"), "{detail}");
        }
        _ => unreachable!(),
    }
}

/// 2b (Linux only). Real OS child running a CPU busy loop stays alive past
/// `stall_limit_s`, then stalls after the child is terminated.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn real_linux_busy_child_prevents_stall_then_stalls_once_killed() {
    use std::os::unix::process::CommandExt;

    struct GroupGuard(Option<std::process::Child>);
    impl Drop for GroupGuard {
        fn drop(&mut self) {
            if let Some(mut c) = self.0.take() {
                unsafe {
                    libc::kill(-(c.id() as libc::pid_t), libc::SIGKILL);
                }
                let _ = c.wait();
            }
        }
    }

    let dir = tmp();
    setup_studio(&dir, 2);
    let (stage, fake) = make_stage().await;
    fake.lock().unwrap().screen = "frozen screen\n".into();

    let mut cmd = std::process::Command::new("sh");
    cmd.args(["-c", "sh -c 'while :; do :; done' & wait"]);
    unsafe {
        cmd.pre_exec(|| {
            libc::setpgid(0, 0);
            Ok(())
        });
    }
    let child = cmd.spawn().unwrap();
    let pid = child.id();
    let guard = GroupGuard(Some(child));

    // Wait briefly until the descendant CPU loop appears in `/proc`.
    for _ in 0..50 {
        if crate::liveness_probe::real_proc_tree(pid).len() >= 2 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let t = term(Some(pid));

    tick(&stage, &dir, &t, "fake", 1_000).await;
    std::thread::sleep(std::time::Duration::from_millis(120));
    tick(&stage, &dir, &t, "fake", 11_000).await;
    std::thread::sleep(std::time::Duration::from_millis(120));
    tick(&stage, &dir, &t, "fake", 21_000).await;

    drop(guard);
    assert!(
        events(&dir, "attention.raised").is_empty(),
        "real busy CPU child must keep role alive past stall_limit_s"
    );

    tick(&stage, &dir, &t, "fake", 31_000).await;
    assert_eq!(events(&dir, "attention.raised").len(), 1);
}

/// 3. Lock wait (`lock_inode_wait`): tolerated below 3x, stalls at 3x with
/// `waiting on: <argv> for <dur>`.
#[tokio::test]
async fn lock_wait_triples_limit_and_reports_waiting_on() {
    let dir = tmp();
    setup_studio(&dir, 30);
    let mut flock = proc_node(202, 200, 1, 1, 0, "flock /home/user/x.lock sleep 1");
    flock.wchan = "locks_lock_inode_wait".into();
    let tree = vec![proc_node(200, 1, 0, 10, 0, "agy"), flock];
    let (mut stage, fake) = make_stage().await;
    stage.proc_read = Box::new(move |_| tree.clone());
    fake.lock().unwrap().screen = "frozen\n".into();
    let t = term(Some(200));

    for s in [0u64, 30, 60, 75] {
        tick(&stage, &dir, &t, "agy", s * 1_000).await;
        assert!(events(&dir, "attention.raised").is_empty(), "early at {s}s");
    }
    tick(&stage, &dir, &t, "agy", 90_000).await;
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 1);
    match &raised[0].kind {
        EventKind::AttentionRaised { reason, detail, .. } => {
            assert_eq!(reason, STALLED);
            assert!(
                detail.contains("waiting on: flock /home/user/x.lock sleep 1 for "),
                "{detail}"
            );
        }
        _ => unreachable!(),
    }
}

/// 4. Exited short-lived child detected via root `cutime + cstime` growth.
#[tokio::test]
async fn exited_child_cutime_growth_keeps_role_alive() {
    let dir = tmp();
    setup_studio(&dir, 20);
    let cutime = Arc::new(Mutex::new(0u64));
    let cutime_r = Arc::clone(&cutime);
    let (mut stage, fake) = make_stage().await;
    stage.proc_read = Box::new(move |_| {
        let mut root = proc_node(300, 1, 0, 10, 0, "claude");
        root.child_cpu_jiffies = *cutime_r.lock().unwrap();
        vec![root]
    });
    fake.lock().unwrap().screen = "frozen\n".into();
    let t = term(Some(300));

    tick(&stage, &dir, &t, "claude", 0).await;
    *cutime.lock().unwrap() = 25;
    tick(&stage, &dir, &t, "claude", 15_000).await;
    *cutime.lock().unwrap() = 50;
    tick(&stage, &dir, &t, "claude", 30_000).await;
    assert!(events(&dir, "attention.raised").is_empty());
}

/// 5. Root process noise floor: idle heartbeat below floor stalls; real work
/// above CPU or I/O floor clears `"process activity"` and stays alive.
#[tokio::test]
async fn root_process_floor_ignores_idle_noise_and_accepts_above_floor() {
    let dir = tmp();
    setup_studio(&dir, 20);
    let stats = Arc::new(Mutex::new((100u64, 1_000u64)));
    let stats_r = Arc::clone(&stats);
    let (mut stage, fake) = make_stage().await;
    stage.proc_read = Box::new(move |_| {
        let (cpu, io) = *stats_r.lock().unwrap();
        vec![proc_node(400, 1, 0, cpu, io, "agy")]
    });
    fake.lock().unwrap().screen = "frozen\n".into();
    let t = term(Some(400));

    tick(&stage, &dir, &t, "agy", 0).await;
    *stats.lock().unwrap() = (
        100 + (ROOT_CPU_FLOOR_JIFFIES_PER_SEC / 2) * 10,
        1_000 + (ROOT_IO_FLOOR_BYTES_PER_SEC / 2) * 10,
    );
    tick(&stage, &dir, &t, "agy", 10_000).await;
    *stats.lock().unwrap() = (
        100 + ROOT_CPU_FLOOR_JIFFIES_PER_SEC * 10,
        1_000 + ROOT_IO_FLOOR_BYTES_PER_SEC * 10,
    );
    tick(&stage, &dir, &t, "agy", 20_000).await;
    assert_eq!(events(&dir, "attention.raised").len(), 1);

    *stats.lock().unwrap() = (
        100 + ROOT_CPU_FLOOR_JIFFIES_PER_SEC * 20 + 50,
        1_000 + ROOT_IO_FLOOR_BYTES_PER_SEC * 10,
    );
    tick(&stage, &dir, &t, "agy", 30_000).await;
    *stats.lock().unwrap() = (
        100 + ROOT_CPU_FLOOR_JIFFIES_PER_SEC * 20 + 50,
        1_000 + ROOT_IO_FLOOR_BYTES_PER_SEC * 20 + 10_000,
    );
    tick(&stage, &dir, &t, "agy", 40_000).await;
    let cleared = events(&dir, "attention.cleared");
    assert_eq!(cleared.len(), 1);
    match &cleared[0].kind {
        EventKind::AttentionCleared { reason, .. } => {
            assert_eq!(reason.as_deref(), Some("process activity"));
        }
        _ => unreachable!(),
    }
    assert_eq!(events(&dir, "attention.raised").len(), 1);
}

/// 6. Work-area movement and ledger event clearing with exact clear reasons,
/// plus remote endpoints skipping OS process/git probes.
#[tokio::test]
async fn work_area_ledger_clear_reasons_and_remote_skips_os_probes() {
    let dir = tmp();
    setup_studio(&dir, 10);
    let snap = Arc::new(Mutex::new(WorkAreaSnap {
        head_sig: "ref1".into(),
        index_mtime_ns: Some(1),
        status_hash: Some(11),
    }));
    let snap_r = Arc::clone(&snap);
    let work_calls = Arc::new(AtomicUsize::new(0));
    let work_calls_r = Arc::clone(&work_calls);
    let proc_calls = Arc::new(AtomicUsize::new(0));
    let proc_calls_r = Arc::clone(&proc_calls);

    let (mut stage, fake) = make_stage().await;
    stage.work_area_read = Box::new(move |_, run_status, _| {
        work_calls_r.fetch_add(1, Ordering::Relaxed);
        let mut s = snap_r.lock().unwrap().clone();
        if !run_status {
            s.status_hash = None;
        }
        s
    });
    stage.proc_read = Box::new(move |_| {
        proc_calls_r.fetch_add(1, Ordering::Relaxed);
        vec![proc_node(500, 1, 0, 1, 0, "claude")]
    });
    fake.lock().unwrap().screen = "frozen\n".into();
    let t = term(Some(500));

    tick(&stage, &dir, &t, "claude", 0).await;
    tick(&stage, &dir, &t, "claude", 10_000).await;
    assert_eq!(events(&dir, "attention.raised").len(), 1);

    snap.lock().unwrap().head_sig = "ref2".into();
    tick(&stage, &dir, &t, "claude", 11_000).await;
    let cleared = events(&dir, "attention.cleared");
    assert_eq!(cleared.len(), 1);
    match &cleared[0].kind {
        EventKind::AttentionCleared { reason, .. } => {
            assert_eq!(reason.as_deref(), Some("work area changed"));
        }
        _ => unreachable!(),
    }

    tick(&stage, &dir, &t, "claude", 21_000).await;
    assert_eq!(events(&dir, "attention.raised").len(), 2);
    store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("actor-t1-c1"),
                EventKind::TaskState {
                    task: "t1".into(),
                    state: backlot_model::event::TaskStateChange::Blocked,
                    reason: "need input".into(),
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    tick(&stage, &dir, &t, "claude", 22_000).await;
    let cleared = events(&dir, "attention.cleared");
    assert_eq!(cleared.len(), 2);
    match &cleared[1].kind {
        EventKind::AttentionCleared { reason, .. } => {
            assert_eq!(reason.as_deref(), Some("ledger event"));
        }
        _ => unreachable!(),
    }

    let before_p = proc_calls.load(Ordering::Relaxed);
    let before_w = work_calls.load(Ordering::Relaxed);
    let mut remote_t = term(Some(500));
    remote_t.address = "host-a/t1".into();
    tick(&stage, &dir, &remote_t, "claude", 23_000).await;
    assert_eq!(proc_calls.load(Ordering::Relaxed), before_p);
    assert_eq!(work_calls.load(Ordering::Relaxed), before_w);
}

fn emit_signal(dir: &std::path::Path, role: &str, ts_ms: u64, event: &str, session_id: &str) {
    let path = crate::signals::signals_path(dir, role);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let line = json!({
        "ts_ms": ts_ms,
        "event": event,
        "session_id": session_id,
    });
    use std::io::Write as _;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    writeln!(f, "{line}").unwrap();
}

/// 7 (Fix1 regression). A hooked Claude role with a stale `Working` signal,
/// high root redraw CPU (30 jiffies/s), and no descendants must still stall
/// naming the last hook signal.
#[tokio::test]
async fn hooked_claude_high_root_cpu_stalls_without_descendants() {
    let dir = tmp();
    setup_studio(&dir, 30);
    emit_signal(&dir, "actor-t1-c1", 10_000, "UserPromptSubmit", "s1");
    let stats = Arc::new(Mutex::new((100u64, 0u64)));
    let stats_r = Arc::clone(&stats);
    let (mut stage, fake) = make_stage().await;
    stage.proc_read = Box::new(move |_| {
        let (c, io) = *stats_r.lock().unwrap();
        vec![proc_node(600, 1, 0, c, io, "claude")]
    });
    fake.lock().unwrap().screen = "frozen\n".into();
    let t = term(Some(600));

    for step in 0..=3u64 {
        *stats.lock().unwrap() = (100 + step * 300, step * 500_000);
        tick(&stage, &dir, &t, "claude", 10_000 + step * 10_000).await;
        if step < 3 {
            assert!(events(&dir, "attention.raised").is_empty());
        }
    }
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 1);
    match &raised[0].kind {
        EventKind::AttentionRaised { reason, detail, .. } => {
            assert_eq!(reason, STALLED);
            assert!(detail.contains("UserPromptSubmit (hook)"), "{detail}");
        }
        _ => unreachable!(),
    }
}

/// 8 (Fix1 busy child). The same hooked Claude role with an active
/// descendant process does not stall.
#[tokio::test]
async fn hooked_claude_with_busy_descendant_does_not_stall() {
    let dir = tmp();
    setup_studio(&dir, 30);
    emit_signal(&dir, "actor-t1-c1", 10_000, "UserPromptSubmit", "s1");
    let cpu = Arc::new(Mutex::new((100u64, 10u64)));
    let cpu_r = Arc::clone(&cpu);
    let (mut stage, fake) = make_stage().await;
    stage.proc_read = Box::new(move |_| {
        let (root_cpu, child_cpu) = *cpu_r.lock().unwrap();
        vec![
            proc_node(601, 1, 0, root_cpu, 0, "claude"),
            proc_node(602, 601, 1, child_cpu, 0, "cargo test"),
        ]
    });
    fake.lock().unwrap().screen = "frozen\n".into();
    let t = term(Some(601));

    for step in 0..=6u64 {
        *cpu.lock().unwrap() = (100 + step * 300, 10 + step * 25);
        tick(&stage, &dir, &t, "claude", 10_000 + step * 10_000).await;
    }
    assert!(events(&dir, "attention.raised").is_empty());
}

/// 9 (Fix1 no hooks). A role without hook signals whose root CPU exceeds the
/// floor (e.g. agy during a pure-model turn before hooks) does not stall.
#[tokio::test]
async fn unhooked_role_above_root_floor_does_not_stall() {
    let dir = tmp();
    setup_studio(&dir, 30);
    let cpu = Arc::new(Mutex::new(100u64));
    let cpu_r = Arc::clone(&cpu);
    let (mut stage, fake) = make_stage().await;
    stage.proc_read =
        Box::new(move |_| vec![proc_node(603, 1, 0, *cpu_r.lock().unwrap(), 0, "agy")]);
    fake.lock().unwrap().screen = "frozen\n".into();
    let t = term(Some(603));

    for step in 0..=6u64 {
        *cpu.lock().unwrap() = 100 + step * 300;
        tick(&stage, &dir, &t, "agy", 10_000 + step * 10_000).await;
    }
    assert!(events(&dir, "attention.raised").is_empty());
}

/// 10 (Rebase R7d2+R7d1 interaction). Once `agy` has session hook signals,
/// root redraw CPU no longer counts: a backgrounded command relies on moving
/// descendants, work-area changes, or newer hook signals.
#[tokio::test]
async fn hooked_agy_relies_on_descendant_work_area_or_signals() {
    let dir = tmp();
    setup_studio(&dir, 30);
    emit_signal(&dir, "actor-t1-c1", 10_000, "UserPromptSubmit", "s1");
    let cpu = Arc::new(Mutex::new((100u64, 10u64)));
    let cpu_r = Arc::clone(&cpu);
    let snap = Arc::new(Mutex::new(WorkAreaSnap {
        head_sig: "ref-a".into(),
        index_mtime_ns: Some(1),
        status_hash: Some(10),
    }));
    let snap_r = Arc::clone(&snap);

    let (mut stage, fake) = make_stage().await;
    stage.proc_read = Box::new(move |_| {
        let (root_cpu, child_cpu) = *cpu_r.lock().unwrap();
        vec![
            proc_node(700, 1, 0, root_cpu, 0, "agy"),
            proc_node(701, 700, 1, child_cpu, 0, "sleep 660"),
        ]
    });
    stage.work_area_read = Box::new(move |_, run_status, _| {
        let mut s = snap_r.lock().unwrap().clone();
        if !run_status {
            s.status_hash = None;
        }
        s
    });
    fake.lock().unwrap().screen = "● [16:36:40] sleep 660 running\n? for shortcuts\n".into();
    let t = term(Some(700));

    // Step 0..2 (20s): descendant CPU grows -> agy stays alive even without hooks firing.
    for step in 0..=2u64 {
        *cpu.lock().unwrap() = (100 + step * 300, 10 + step * 5);
        tick(&stage, &dir, &t, "agy", 10_000 + step * 10_000).await;
    }
    assert!(events(&dir, "attention.raised").is_empty());

    // Descendant stops (flat CPU=20), while agy root keeps burning 30 jiffies/s:
    // 30s later (at 60_000) it stalls because root CPU is ignored when hooked.
    for step in 3..=5u64 {
        *cpu.lock().unwrap() = (100 + step * 300, 20);
        tick(&stage, &dir, &t, "agy", 10_000 + step * 10_000).await;
    }
    assert_eq!(events(&dir, "attention.raised").len(), 1);

    // A work-area update clears the stall (`"work area changed"`).
    snap.lock().unwrap().head_sig = "ref-b".into();
    tick(&stage, &dir, &t, "agy", 61_000).await;
    let cleared = events(&dir, "attention.cleared");
    assert_eq!(cleared.len(), 1);
    match &cleared[0].kind {
        EventKind::AttentionCleared { reason, .. } => {
            assert_eq!(reason.as_deref(), Some("work area changed"));
        }
        _ => unreachable!(),
    }
}
