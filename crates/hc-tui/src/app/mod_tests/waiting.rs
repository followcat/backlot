//! The Waiting overlay end to end: eligibility, the lazy probe worker,
//! one `machine.exec` listing per machine, the Claude `file.read`
//! layer, and the presentation-only overlay on a shown Idle.
use super::*;
use crate::chrome::status::AgentStatus;
use crate::testcore::{self, FakeCore};
use crate::waiting;
use hc_proto::client::Client;
use hc_proto::frame::Kind;
use hc_proto::terminal::{Foreground, Status};
use hc_proto::watch::OpenRpc;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::mpsc;

/// A live agent carrying `word`/`app` on its root record, a foreground
/// pid and the space/tab labels the projection needs.
fn waiting_agent(address: &str, word: &str, app: &str, pid: i32) -> hc_proto::terminal::Terminal {
    let mut term = crate::sidebar::fixture::agent(address, word);
    term.status = Some(Status {
        state: word.into(),
        app: app.into(),
        ..Status::default()
    });
    term.foreground = Some(Foreground {
        pid,
        name: app.into(),
        argv: vec![app.into()],
        cwd: "/home/user/proj".into(),
    });
    term.labels.insert("herdr.space".into(), "space-a".into());
    term.labels.insert("herdr.tab".into(), "tab-a".into());
    term
}

/// The app plus `rpc_rx`, which hands out the server half of every
/// conn `set_operations` opens for its workers. The watcher rides its
/// own opener — its `machine.list`/`doc.get` polling is not the probe
/// worker, and must not count against the worker's laziness.
fn waiting_app() -> (App, mpsc::Receiver<FakeCore>) {
    let (rpc_accepted, rpc_rx) = mpsc::channel(8);
    let rpc: OpenRpc = hc_proto::client::shared_rpc_with(move || {
        let tx = rpc_accepted.clone();
        async move {
            let (core, read, write) = testcore::pair(64 * 1024);
            if tx.send(core).await.is_err() {
                return Err(hc_proto::client::Error::Timeout("fake accept"));
            }
            Client::open(read, write, None).await
        }
    });
    let (sub_accepted, _sub_rx) = mpsc::channel(4);
    let (watcher_rpc, sub) = testcore::openers(sub_accepted);
    let watcher = Watcher::with_openers(
        watcher_rpc,
        sub,
        hc_proto::watch::WatchParams {
            traversal: Some(crate::project::agent_traversal),
            ..hc_proto::watch::WatchParams::default()
        },
    );
    let open: OpenAttach = Arc::new(|_| Box::pin(std::future::pending()));
    let config = crate::config::Config::default();
    let live = config.live_keybinds();
    let shell = crate::chrome::ClientShellConfig::from_config(
        &config,
        None,
        None,
        std::path::PathBuf::new(),
    );
    let mut app = App::new(watcher, open, live, shell, Arc::new(Probe::default()));
    app.set_operations(rpc);
    (app, rpc_rx)
}

/// Answers every conn `rpc_rx` hands out: `methods` is the hello
/// advertisement, `answers` supplies results beyond the fixed
/// `doc.get`/`machine.list`/`terminal.list`, and every request's
/// `method`+`params` lands on `requests` for assertions.
fn serve_all(
    mut rpc_rx: mpsc::Receiver<FakeCore>,
    requests: mpsc::Sender<Value>,
    methods: &'static [&'static str],
    answers: Arc<dyn Fn(&Value) -> Value + Send + Sync>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(core) = rpc_rx.recv().await {
            let requests = requests.clone();
            let answers = answers.clone();
            tokio::spawn(async move {
                let mut core = core;
                core.read_hello().await;
                core.answer_hello(true, methods, &[]).await;
                while let Some(frame) = core.try_read_frame().await {
                    if frame.channel != 0 || frame.kind != Kind::Data {
                        continue;
                    }
                    let req: Value = serde_json::from_slice(&frame.payload).unwrap();
                    let _ = requests
                        .send(json!({"method": req["method"], "params": req["params"]}))
                        .await;
                    let result = match req["method"].as_str() {
                        Some("doc.get") => json!({"value": null, "version": 1}),
                        Some("machine.list") => json!({"machines": []}),
                        Some("terminal.list") => json!({"terminals": []}),
                        _ => answers(&req),
                    };
                    core.reply(&req["id"], result).await;
                }
            });
        }
    })
}

/// Publishes `terms` as the current view and reconciles —
/// `sync_waiting` runs inside.
fn publish(app: &mut App, terms: Vec<hc_proto::terminal::Terminal>) {
    app.view = crate::sidebar::fixture::view(terms, vec![]);
    app.reconcile_published_view();
}

/// The next probe outcome, with the test deadline wrapped around it.
async fn waiting_result(app: &mut App) -> crate::app::waiting::Outcome {
    tokio::time::timeout(testcore::DEADLINE, app.waiting_rx.recv())
        .await
        .expect("probe never resolved")
        .expect("probe worker stopped")
}

fn agent_status_of(app: &App, pane_id: &str) -> crate::chrome::status::AgentStatus {
    app.projection
        .snapshot
        .agents
        .iter()
        .find(|agent| agent.pane_id == pane_id)
        .map(|agent| agent.agent_status)
        .expect("agent not projected")
}

/// A `machine.exec` result carrying `stdout` text, base64 the way the
/// wire sends it.
fn exec_result(stdout: &str) -> Value {
    json!({
        "code": 0, "signal": null,
        "stdout": crate::selection::base64_encode(stdout.as_bytes()),
        "stderr": "", "truncated": false, "timed_out": false, "ms": 1,
    })
}

/// A `ps` listing with the agent row plus optional extra rows.
fn ps(agent_pid: u32, agent_etime: &str, extra: &str) -> String {
    format!("{agent_pid:>6}       1    {agent_etime} devin\n{extra}")
}

const METHODS: &[&str] = &[
    "machine.exec",
    "file.read",
    "doc.get",
    "machine.list",
    "terminal.list",
];

#[tokio::test]
async fn an_idle_agent_with_a_tool_shell_descendant_shows_waiting() {
    let (mut app, rpc_rx) = waiting_app();
    let (requests, _recorded) = mpsc::channel(64);
    let answers = Arc::new(|req: &Value| {
        assert_eq!(req["method"], "machine.exec");
        exec_result(&ps(
            4242,
            "08:20",
            "  4300    4242       07:00 zsh -c sleep 30\n",
        ))
    });
    let serve = serve_all(rpc_rx, requests, METHODS, answers);
    publish(&mut app, vec![waiting_agent("t1", "idle", "devin", 4242)]);
    assert_eq!(agent_status_of(&app, "t1"), AgentStatus::Idle);

    // The shell must show in two consecutive listings — the first is
    // only a candidate, the second is the evidence.
    let outcome = waiting_result(&mut app).await;
    assert!(outcome.candidate && !outcome.waiting);
    app.on_waiting_result(outcome);
    let probed = app.waiting["t1"].probed_at;
    app.tick_waiting(probed + std::time::Duration::from_secs(3));
    let outcome = waiting_result(&mut app).await;
    assert!(outcome.waiting);
    app.on_waiting_result(outcome);
    assert_eq!(agent_status_of(&app, "t1"), AgentStatus::Waiting);
    serve.abort();
}

#[tokio::test]
async fn no_eligible_terminal_opens_no_connection() {
    let (mut app, mut rpc_rx) = waiting_app();
    // A live view with no eligible terminal: the worker must not even
    // open the shared conn for a cycle.
    publish(&mut app, vec![]);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        rpc_rx.try_recv().is_err(),
        "a conn opened with no eligible terminal"
    );
}

#[tokio::test]
async fn one_listing_serves_every_agent_on_a_machine() {
    let (mut app, rpc_rx) = waiting_app();
    let (requests, mut recorded) = mpsc::channel(64);
    let answers = Arc::new(|_req: &Value| {
        exec_result(
            "  4242       1       08:20 devin\n\
              4300    4242       07:00 zsh -c sleep 30\n\
              5200       1       08:30 devin\n\
              5300    5200       07:10 sh -c sleep 10\n",
        )
    });
    let serve = serve_all(rpc_rx, requests, METHODS, answers);
    publish(
        &mut app,
        vec![
            waiting_agent("t1", "idle", "devin", 4242),
            waiting_agent("t2", "idle", "devin", 5200),
        ],
    );
    // First listing: both shells are candidates; the second listing
    // confirms them — still one `ps` per machine per cycle.
    let first = waiting_result(&mut app).await;
    let second = waiting_result(&mut app).await;
    app.on_waiting_result(first);
    app.on_waiting_result(second);
    let probed = app.waiting["t1"].probed_at;
    app.tick_waiting(probed + std::time::Duration::from_secs(3));
    let first = waiting_result(&mut app).await;
    let second = waiting_result(&mut app).await;
    app.on_waiting_result(first);
    app.on_waiting_result(second);
    assert_eq!(agent_status_of(&app, "t1"), AgentStatus::Waiting);
    assert_eq!(agent_status_of(&app, "t2"), AgentStatus::Waiting);
    // Two cycles, one listing each — the terminals always shared it.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let mut execs = 0;
    while let Ok(req) = recorded.try_recv() {
        if req["method"] == "machine.exec" {
            execs += 1;
        }
    }
    assert_eq!(execs, 2, "each terminal ran its own listing");
    serve.abort();
}

#[tokio::test]
async fn a_transient_shell_needs_two_consecutive_listings() {
    let (mut app, rpc_rx) = waiting_app();
    let (requests, _recorded) = mpsc::channel(64);
    // The listing's shell pid changes between cycles — a `sh -c`
    // helper that lives only milliseconds (a statusLine or hook) must
    // not flicker the row idle → waiting → idle.
    let extra = Arc::new(std::sync::Mutex::new(String::from(
        "  4300    4242       07:00 sh -c sleep 30\n",
    )));
    let served = extra.clone();
    let answers = Arc::new(move |_req: &Value| {
        let extra = served.lock().unwrap().clone();
        exec_result(&format!("  4242       1       08:20 devin\n{extra}"))
    });
    let serve = serve_all(rpc_rx, requests, METHODS, answers);
    publish(&mut app, vec![waiting_agent("t1", "idle", "devin", 4242)]);

    // First sighting of pid 4300: a candidate, not yet evidence.
    let first = waiting_result(&mut app).await;
    assert!(first.candidate && !first.waiting);
    app.on_waiting_result(first);
    assert_eq!(agent_status_of(&app, "t1"), AgentStatus::Idle);

    // The same pid in the next listing is the evidence.
    let probed = app.waiting["t1"].probed_at;
    app.tick_waiting(probed + std::time::Duration::from_secs(3));
    let second = waiting_result(&mut app).await;
    assert!(second.waiting);
    app.on_waiting_result(second);
    assert_eq!(agent_status_of(&app, "t1"), AgentStatus::Waiting);

    // A different pid is a different shell — the count starts over.
    *extra.lock().unwrap() = "  4400    4242       07:10 sh -c sleep 30\n".into();
    let probed = app.waiting["t1"].probed_at;
    app.tick_waiting(probed + std::time::Duration::from_secs(3));
    let third = waiting_result(&mut app).await;
    assert!(third.candidate && !third.waiting);
    app.on_waiting_result(third);
    assert_eq!(agent_status_of(&app, "t1"), AgentStatus::Idle);
    serve.abort();
}

#[tokio::test]
async fn a_candidate_shell_keeps_the_fast_cadence() {
    let (mut app, rpc_rx) = waiting_app();
    let (requests, _recorded) = mpsc::channel(64);
    let answers = Arc::new(|_req: &Value| {
        exec_result(&ps(
            4242,
            "08:20",
            "  4300    4242       07:00 sh -c sleep 30\n",
        ))
    });
    let serve = serve_all(rpc_rx, requests, METHODS, answers);
    publish(&mut app, vec![waiting_agent("t1", "idle", "devin", 4242)]);

    // A candidate keeps ~3s so the second sighting shows quickly.
    let first = waiting_result(&mut app).await;
    assert!(first.candidate);
    app.on_waiting_result(first);
    let probed = app.waiting["t1"].probed_at;
    assert_eq!(
        app.waiting_due,
        Some(probed + std::time::Duration::from_secs(3))
    );

    // A Waiting verdict also stays fast so it clears quickly.
    app.tick_waiting(probed + std::time::Duration::from_secs(3));
    let second = waiting_result(&mut app).await;
    assert!(second.waiting);
    app.on_waiting_result(second);
    let probed = app.waiting["t1"].probed_at;
    assert_eq!(
        app.waiting_due,
        Some(probed + std::time::Duration::from_secs(3))
    );
    serve.abort();
}

#[tokio::test]
async fn an_empty_listing_backs_off_the_next_probe() {
    let (mut app, rpc_rx) = waiting_app();
    let (requests, mut recorded) = mpsc::channel(64);
    let answers = Arc::new(|_req: &Value| exec_result(&ps(4242, "08:20", "")));
    let serve = serve_all(rpc_rx, requests, METHODS, answers);
    publish(&mut app, vec![waiting_agent("t1", "idle", "devin", 4242)]);

    // The listing found nothing: the next probe is ~10s out, not ~3s.
    let outcome = waiting_result(&mut app).await;
    assert!(!outcome.waiting && !outcome.candidate);
    app.on_waiting_result(outcome);
    let probed = app.waiting["t1"].probed_at;
    assert_eq!(
        app.waiting_due,
        Some(probed + std::time::Duration::from_secs(10))
    );

    // Nothing queues inside the backed-off window.
    app.tick_waiting(probed + std::time::Duration::from_secs(4));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let execs = std::iter::from_fn(|| recorded.try_recv().ok())
        .filter(|req| req["method"] == "machine.exec")
        .count();
    assert_eq!(execs, 1, "a probe ran inside the backed-off window");
    serve.abort();
}

#[tokio::test]
async fn an_eligible_terminal_reprobes_on_the_cadence() {
    let (mut app, rpc_rx) = waiting_app();
    let (requests, mut recorded) = mpsc::channel(64);
    // A persistent shell keeps the terminal on the fast ~3s cadence.
    let answers = Arc::new(|_req: &Value| {
        exec_result(&ps(
            4242,
            "08:20",
            "  4300    4242       07:00 sh -c sleep 30\n",
        ))
    });
    let serve = serve_all(rpc_rx, requests, METHODS, answers);
    publish(&mut app, vec![waiting_agent("t1", "idle", "devin", 4242)]);
    // A probe ran at eligibility and armed the cadence deadline.
    let first = waiting_result(&mut app).await;
    app.on_waiting_result(first);
    assert!(app.wake_deadline().is_some(), "no re-probe deadline armed");

    // Inside the window a tick queues nothing; at it, one new cycle.
    let probed = app.waiting["t1"].probed_at;
    app.tick_waiting(probed + std::time::Duration::from_secs(1));
    app.tick_waiting(probed + std::time::Duration::from_secs(3));
    let _ = waiting_result(&mut app).await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let execs = recorded
        .try_recv()
        .into_iter()
        .chain(std::iter::from_fn(|| recorded.try_recv().ok()))
        .filter(|req| req["method"] == "machine.exec")
        .count();
    assert_eq!(execs, 2, "expected one exec per ~3s cycle");
    serve.abort();
}

#[tokio::test]
async fn a_result_is_dropped_when_the_terminal_turns_working() {
    let (mut app, _rpc_rx) = waiting_app();
    // Seed a waiting verdict directly — the probe path is covered by
    // the listing test above; this one pins the drop rule.
    let term = waiting_agent("t1", "idle", "devin", 4242);
    publish(&mut app, vec![term.clone()]);
    app.on_waiting_result(crate::app::waiting::Outcome {
        address: "t1".into(),
        fingerprint: waiting::Fingerprint::of(&term).unwrap(),
        waiting: true,
        candidate: true,
    });
    assert_eq!(agent_status_of(&app, "t1"), AgentStatus::Waiting);

    publish(
        &mut app,
        vec![waiting_agent("t1", "working", "devin", 4242)],
    );
    assert_eq!(agent_status_of(&app, "t1"), AgentStatus::Working);
    assert!(
        app.waiting.is_empty(),
        "stale verdict survived the word change"
    );
}

#[tokio::test]
async fn an_unseen_completion_stays_done_then_shows_waiting() {
    let (mut app, _rpc_rx) = waiting_app();
    let mut term = waiting_agent("t1", "done", "devin", 4242);
    term.turn = 5;
    term.labels.insert("herdr.seen".into(), "0".into());
    publish(&mut app, vec![term.clone()]);
    assert_eq!(agent_status_of(&app, "t1"), AgentStatus::Done);
    app.on_waiting_result(crate::app::waiting::Outcome {
        address: "t1".into(),
        fingerprint: waiting::Fingerprint::of(&term).unwrap(),
        waiting: true,
        candidate: true,
    });
    // Unseen completion: the overlay never hides `Done`.
    assert_eq!(agent_status_of(&app, "t1"), AgentStatus::Done);
    // Viewed: the completion acknowledges to Idle, and the overlay
    // presents Waiting while the background work runs.
    assert!(
        app.presentation
            .acknowledge(&mut app.projection, "t1", Some(true), &app.waiting)
    );
    assert_eq!(agent_status_of(&app, "t1"), AgentStatus::Waiting);
}

#[tokio::test]
async fn the_claude_sessions_file_drives_the_exact_layer() {
    let (mut app, rpc_rx) = waiting_app();
    let (requests, _recorded) = mpsc::channel(64);
    let answers = Arc::new(|req: &Value| match req["method"].as_str() {
        Some("machine.exec") => {
            let argv = req["params"]["argv"].as_array().unwrap();
            if argv.iter().any(|a| a == "ps") {
                // The process layer finds nothing for Claude's pid.
                exec_result(&ps(4242, "08:20", ""))
            } else {
                exec_result("/home/user")
            }
        }
        Some("file.read") => {
            assert_eq!(
                req["params"]["path"],
                "/home/user/.claude/sessions/4242.json"
            );
            json!({
                "text": r#"{"pid":4242,"status":"shell","socket":"/private/sock"}"#,
                "first": 1, "last": 1, "lines": 1,
            })
        }
        _ => Value::Null,
    });
    let serve = serve_all(rpc_rx, requests, METHODS, answers);
    publish(
        &mut app,
        vec![waiting_agent("t1", "idle", "claude-code", 4242)],
    );
    let outcome = waiting_result(&mut app).await;
    assert!(outcome.waiting);
    app.on_waiting_result(outcome);
    assert_eq!(agent_status_of(&app, "t1"), AgentStatus::Waiting);
    serve.abort();
}

#[tokio::test]
async fn an_unoffered_method_turns_the_layer_off_quietly() {
    let (mut app, rpc_rx) = waiting_app();
    let (requests, mut recorded) = mpsc::channel(64);
    // Neither probe method is offered: the conn still opens lazily on
    // the first eligible terminal, then runs no wire call.
    let answers = Arc::new(|_req: &Value| Value::Null);
    let serve = serve_all(
        rpc_rx,
        requests,
        &["doc.get", "machine.list", "terminal.list"],
        answers,
    );
    publish(&mut app, vec![waiting_agent("t1", "idle", "devin", 4242)]);
    let outcome = waiting_result(&mut app).await;
    assert!(!outcome.waiting);
    app.on_waiting_result(outcome);
    assert_eq!(agent_status_of(&app, "t1"), AgentStatus::Idle);
    // No probe request ever reached the core, and no notice fired.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    while let Ok(req) = recorded.try_recv() {
        let method = req["method"].as_str().unwrap();
        assert!(
            !matches!(method, "machine.exec" | "file.read"),
            "unoffered {method} was still called"
        );
    }
    assert!(app.notice.is_none());
    serve.abort();
}
