//! Opt-in real-core stage test: the compiled `backlot` binary drives a
//! whole stage loop on a `ScratchCore` — role terminals start and stop
//! on ledger changes, the lifecycle events land, and the `backlot`
//! document updates with throttled writes.
//!
//! Ignored by default: `cargo test` never touches a real core. Run
//! explicitly with
//!
//! ```sh
//! HC_TUI_TEST_CORE=$(command -v hc) \
//!     cargo test -p hc-tui --test real_stage -- --ignored --test-threads=1
//! ```

#![cfg(unix)]

#[path = "../../hc-proto/tests/common/scratch.rs"]
mod scratch;

use scratch::ScratchCore;

use hc_proto::client::Client;
use hc_proto::doc::DocGetParams;
use hc_proto::endpoint::Endpoint;
use hc_proto::terminal::{ListParams, ReadParams, Terminal};

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Finite bound on any single real-core step.
const DEADLINE: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(150);

/// The `backlot` binary under test — cargo exports it for this target.
fn tui_bin() -> &'static str {
    env!("CARGO_BIN_EXE_backlot")
}

/// Every `backlot` invocation sees the scratch session, an explicit
/// state dir and nothing inherited — a real session can never leak in.
fn bl_env(scratch: &ScratchCore, state: &Path) -> Vec<(String, String)> {
    let Endpoint::Local(local) = &scratch.endpoint else {
        panic!("scratch endpoint is always local");
    };
    let root = |dir: &str| scratch.root.join(dir).to_string_lossy().into_owned();
    vec![
        ("PATH".into(), "/usr/bin:/bin".into()),
        ("LANG".into(), "C.UTF-8".into()),
        ("HOME".into(), root("home")),
        ("XDG_STATE_HOME".into(), root("state")),
        ("XDG_CONFIG_HOME".into(), root("config")),
        ("XDG_RUNTIME_DIR".into(), root("run")),
        ("HC_SESSION".into(), scratch.session.clone()),
        (
            "HC_SOCKET".into(),
            local.socket.to_string_lossy().into_owned(),
        ),
        (
            "BACKLOT_STATE_DIR".into(),
            state.to_string_lossy().into_owned(),
        ),
        ("BACKLOT_STUDIO".into(), "acme-shop".into()),
    ]
}

/// `backlot <args>` under the fixture env; assert exit 0 within the
/// deadline and return stdout.
fn bl(scratch: &ScratchCore, state: &Path, args: &str) -> String {
    let mut child = Command::new(tui_bin())
        .args(args.split(' '))
        .env_clear()
        .envs(bl_env(scratch, state))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("cannot spawn backlot {args}: {e}"));
    let deadline = Instant::now() + DEADLINE;
    loop {
        if child.try_wait().expect("try_wait").is_some() {
            let out = child.wait_with_output().expect("output");
            assert!(
                out.status.success(),
                "backlot {args} exited {}; stderr: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr)
            );
            return String::from_utf8_lossy(&out.stdout).into_owned();
        }
        assert!(Instant::now() < deadline, "backlot {args} timed out");
        std::thread::sleep(POLL);
    }
}

async fn connect(scratch: &ScratchCore) -> Client {
    Client::connect(&scratch.endpoint, None::<&std::ffi::OsStr>)
        .await
        .expect("connect to scratch core")
}

/// The live terminal with `name`, when the core shows one.
async fn terminal_named(client: &Client, name: &str) -> Option<Terminal> {
    client
        .terminal_list(ListParams::default())
        .await
        .ok()?
        .terminals
        .into_iter()
        .find(|t| t.name.as_deref() == Some(name))
}

async fn wait_terminal(client: &Client, name: &str) -> Terminal {
    let deadline = Instant::now() + DEADLINE;
    loop {
        if let Some(t) = terminal_named(client, name).await {
            return t;
        }
        assert!(Instant::now() < deadline, "no terminal named {name}");
        tokio::time::sleep(POLL).await;
    }
}

/// Wait until `name` is absent even counting finished terminals —
/// `terminal.kill` + `terminal.rm` leave no record.
async fn wait_absent(client: &Client, name: &str) {
    let deadline = Instant::now() + DEADLINE;
    loop {
        let found = client
            .terminal_list(ListParams {
                all: true,
                ..ListParams::default()
            })
            .await
            .expect("terminal.list")
            .terminals
            .iter()
            .any(|t| t.name.as_deref() == Some(name));
        if !found {
            return;
        }
        assert!(Instant::now() < deadline, "terminal {name} still exists");
        tokio::time::sleep(POLL).await;
    }
}

/// `terminal.read` until it shows `needle` (the program may still be
/// starting when the terminal record appears).
async fn wait_read(client: &Client, terminal: &str, needle: &str) -> String {
    let deadline = Instant::now() + DEADLINE;
    loop {
        let text = client
            .terminal_read(ReadParams {
                terminal: terminal.into(),
                lines: 20,
                caller: None,
            })
            .await
            .map(|r| r.text)
            .unwrap_or_default();
        if text.contains(needle) {
            return text;
        }
        assert!(
            Instant::now() < deadline,
            "{terminal} never showed {needle:?}"
        );
        tokio::time::sleep(POLL).await;
    }
}

/// The `backlot` document's version and value once it exists.
async fn wait_doc(client: &Client) -> (u64, serde_json::Value) {
    let deadline = Instant::now() + DEADLINE;
    loop {
        let r = client
            .doc_get(DocGetParams {
                name: "backlot".into(),
                on: None,
            })
            .await
            .expect("doc.get");
        if !r.value.is_null() {
            return (r.version, r.value);
        }
        assert!(
            Instant::now() < deadline,
            "the backlot document never appeared"
        );
        tokio::time::sleep(POLL).await;
    }
}

async fn doc_version(client: &Client) -> u64 {
    client
        .doc_get(DocGetParams {
            name: "backlot".into(),
            on: None,
        })
        .await
        .expect("doc.get")
        .version
}

/// Every ledger line for `acme-shop`, parsed.
fn ledger(state: &Path) -> Vec<serde_json::Value> {
    fs::read_to_string(state.join("studios/acme-shop/events.jsonl"))
        .expect("events.jsonl")
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).expect("ledger line"))
        .collect()
}

const CAST_TOML: &str = r#"
[producer]
runner = "fake"
model = "model-p"
effort = "high"
permission_profile = "standard"

[director]
runner = "fake"
model = "model-d"
effort = "high"
permission_profile = "standard"

[writer]
runner = "fake"
model = "model-w"
effort = "medium"
permission_profile = "standard"

[[candidates]]
name = "actor-a"
runner = "fake"
model = "model-a"
effort = "high"
permission_profile = "worktree"

[[candidates]]
name = "actor-b"
runner = "fake"
model = "model-b"
effort = "low"
permission_profile = "worktree"
"#;

fn fixture(scratch: &ScratchCore, name: &str, content: &str) -> PathBuf {
    let path = scratch.work.join(name);
    fs::write(&path, content).unwrap();
    path
}

/// Assert `t` carries the role labels design.md requires.
fn check_labels(t: &Terminal, studio: &str, role: &str, task: Option<&str>) {
    let l = &t.labels;
    assert_eq!(
        l.get("backlot.studio").map(String::as_str),
        Some(studio),
        "{t:?}"
    );
    assert_eq!(
        l.get("backlot.role").map(String::as_str),
        Some(role),
        "{t:?}"
    );
    assert_eq!(
        l.get("herdr.space").map(String::as_str),
        Some("backlot"),
        "{t:?}"
    );
    assert_eq!(l.get("herdr.seen").map(String::as_str), Some("0"), "{t:?}");
    assert_eq!(l.get("backlot.task").map(String::as_str), task, "{t:?}");
}

#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn real_stage_loop() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let state = scratch.root.join("bl-state");
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    let req = fixture(&scratch, "req.md", "# requirement\nbuild the shop\n");
    let acc = fixture(&scratch, "acc.md", "# acceptance\n");
    let note = fixture(&scratch, "note.md", "director: go\n");

    // A studio exists; the stage starts once and is idempotent.
    bl(
        &scratch,
        &state,
        &format!(
            "studio new --title acme-shop --repo {} --cast {}",
            scratch.work.display(),
            cast.display()
        ),
    );
    let out = bl(&scratch, &state, "stage start");
    assert!(out.contains("started"), "{out}");
    let again = bl(&scratch, &state, "stage start");
    assert!(again.contains("already running"), "{again}");

    // Producer and director come up with their labels and env.
    let producer = wait_terminal(&client, "bl-acme-shop-producer").await;
    let director = wait_terminal(&client, "bl-acme-shop-director").await;
    check_labels(&producer, "acme-shop", "producer", None);
    check_labels(&director, "acme-shop", "director", None);
    assert!(
        producer.argv[0].ends_with("tools/fake-runner.sh"),
        "{:?}",
        producer.argv
    );
    wait_read(
        &client,
        "bl-acme-shop-producer",
        "fake runner: producer of acme-shop",
    )
    .await;

    // The writer is wanted between plan request and dispatch.
    bl(
        &scratch,
        &state,
        &format!(
            "req revise --file {} --summary v1 --as producer",
            req.display()
        ),
    );
    bl(&scratch, &state, "plan request --as producer");
    let writer = wait_terminal(&client, "bl-acme-shop-writer").await;
    check_labels(&writer, "acme-shop", "writer", None);
    bl(
        &scratch,
        &state,
        &format!("dispatch --file {} --as producer", note.display()),
    );
    wait_absent(&client, "bl-acme-shop-writer").await;

    // A cast starts an actor; closing its task removes the terminal.
    bl(
        &scratch,
        &state,
        &format!(
            "task new --title cart --kind feature --acceptance-file {} --as director",
            acc.display()
        ),
    );
    bl(
        &scratch,
        &state,
        "cast --task t1 --actor actor-a --reason first --as director",
    );
    let actor_t = wait_terminal(&client, "bl-acme-shop-t1").await;
    check_labels(&actor_t, "acme-shop", "actor-t1-c1", Some("t1"));

    // The summary document appears and names the studio.
    let (_, doc) = wait_doc(&client).await;
    assert_eq!(doc["studios"][0]["id"], "acme-shop", "{doc}");

    // Closing the task stops the actor deliberately.
    bl(
        &scratch,
        &state,
        "task abandon --task t1 --reason done --as director",
    );
    wait_absent(&client, "bl-acme-shop-t1").await;

    // The ledger holds role.closed, no role.lost, and role.started
    // events carrying terminal addresses.
    let events = ledger(&state);
    fn kind(e: &serde_json::Value) -> &str {
        e["kind"].as_str().unwrap_or("")
    }
    assert!(
        events
            .iter()
            .any(|e| kind(e) == "role.closed" && e["role"] == "actor-t1-c1"),
        "no role.closed for actor-t1-c1: {events:?}"
    );
    assert!(
        !events.iter().any(|e| kind(e) == "role.lost"),
        "role.lost present: {events:?}"
    );
    let started: Vec<&serde_json::Value> = events
        .iter()
        .filter(|e| kind(e) == "role.started")
        .collect();
    assert!(started.len() >= 4, "{events:?}");
    for e in &started {
        assert!(
            e["terminal"].as_str().is_some_and(|s| !s.is_empty()),
            "role.started without an address: {e}"
        );
    }

    // status --json still shows the closed role and its session id.
    let status = bl(&scratch, &state, "status --json");
    let status: serde_json::Value = serde_json::from_str(&status).unwrap();
    let closed = &status["roles"]["actor-t1-c1"];
    assert_eq!(closed["closed"], true, "{closed}");
    assert_eq!(closed["live"], false, "{closed}");
    assert_eq!(
        closed["session_id"].as_str(),
        Some("fake-bl-acme-shop-t1"),
        "{closed}"
    );

    // The document saw the close too.
    let deadline = Instant::now() + DEADLINE;
    loop {
        let (_, doc) = wait_doc(&client).await;
        let roles = &doc["studios"][0]["roles"];
        let closed_role = roles
            .as_array()
            .and_then(|rs| rs.iter().find(|r| r["role"] == "actor-t1-c1"));
        if closed_role.is_some_and(|r| r["state"] == "closed" && r["doing"].is_null()) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "closed actor never reached the doc: {doc}"
        );
        tokio::time::sleep(POLL).await;
    }

    // A burst of changes: at most one write per two seconds. The gap
    // is open, then several ledger-changing commands land inside it.
    tokio::time::sleep(Duration::from_millis(2300)).await;
    let v0 = doc_version(&client).await;
    let req2 = fixture(&scratch, "req2.md", "# requirement v2\nbigger shop\n");
    bl(
        &scratch,
        &state,
        &format!(
            "req revise --file {} --summary v2 --as producer",
            req2.display()
        ),
    );
    bl(&scratch, &state, "plan request --as producer");
    bl(
        &scratch,
        &state,
        &format!(
            "task new --title returns --kind fix --acceptance-file {} --as director",
            acc.display()
        ),
    );
    bl(
        &scratch,
        &state,
        "cast --task t2 --actor actor-b --reason second --as director",
    );
    tokio::time::sleep(Duration::from_millis(4500)).await;
    let v1 = doc_version(&client).await;
    assert!(
        (1..=2).contains(&(v1 - v0)),
        "expected 1-2 doc writes for the burst, got {} (v{v0} -> v{v1})",
        v1 - v0
    );
    // The last change's content landed.
    let (_, doc) = wait_doc(&client).await;
    assert_eq!(doc["studios"][0]["requirement_version"], 2, "{doc}");

    drop(client);
    scratch.finish().expect("scratch teardown");
}
