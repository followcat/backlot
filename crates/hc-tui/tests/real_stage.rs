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

/// `name` exists again at an address different from `old` — a
/// stopped-and-restarted terminal under the shared name.
async fn wait_replaced(client: &Client, name: &str, old: &str) -> Terminal {
    let deadline = Instant::now() + DEADLINE;
    loop {
        if let Some(t) = terminal_named(client, name).await
            && t.address != old
        {
            return t;
        }
        assert!(
            Instant::now() < deadline,
            "terminal {name} never left {old}"
        );
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
            "{terminal} never showed {needle:?}; screen: {text:?}"
        );
        tokio::time::sleep(POLL).await;
    }
}

/// `terminal.send` raw text into a terminal's program.
async fn send_text(client: &Client, terminal: &str, text: &str) {
    client
        .call(
            "terminal.send",
            Some(serde_json::json!({"terminal": terminal, "text": text})),
        )
        .await
        .expect("terminal.send");
}

/// The acme-shop role's summary entry once `want` holds for it.
async fn wait_role(
    client: &Client,
    role: &str,
    want: fn(&serde_json::Value) -> bool,
) -> serde_json::Value {
    let deadline = Instant::now() + DEADLINE;
    loop {
        let (_, doc) = wait_doc(client).await;
        let found = doc["studios"]
            .as_array()
            .and_then(|ss| ss.iter().find(|s| s["id"] == "acme-shop"))
            .and_then(|s| {
                s["roles"]
                    .as_array()
                    .and_then(|rs| rs.iter().find(|r| r["role"] == role))
            });
        if let Some(r) = found.filter(|r| want(r)) {
            return r.clone();
        }
        assert!(
            Instant::now() < deadline,
            "{role} never matched in the doc: {doc}"
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

/// The ledger gains an event of `kind` on `role` within the deadline.
fn wait_event(state: &Path, kind: &str, role: &str) -> serde_json::Value {
    let deadline = Instant::now() + DEADLINE;
    loop {
        if let Some(e) = ledger(state)
            .into_iter()
            .find(|e| e["kind"] == kind && e["role"] == role)
        {
            return e;
        }
        assert!(
            Instant::now() < deadline,
            "no {kind} for {role} in the ledger"
        );
        std::thread::sleep(POLL);
    }
}

/// The ledger reaches `n` events of `kind` on `role` — the count
/// catches a repeated event the single-event wait cannot see.
fn wait_events(state: &Path, kind: &str, role: &str, n: usize) {
    let deadline = Instant::now() + DEADLINE;
    loop {
        let count = ledger(state)
            .iter()
            .filter(|e| e["kind"] == kind && e["role"] == role)
            .count();
        if count >= n {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "wanted {n} {kind} for {role} in the ledger"
        );
        std::thread::sleep(POLL);
    }
}

const CAST_TOML: &str = r#"
[producer]
runner = "fake"
model = "trust"
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
    // The producer's `model = "trust"` puts the fake runner behind
    // Claude Code's folder-trust dialog — with the real dialog's
    // unsafe first second: a Down that arrives inside it is dropped
    // (and logged where the test can see), an early Enter would end
    // the runner outright. The stage's pacing means nothing goes out
    // inside that window: `❯ No` sits past the settle, `❯ Yes` past
    // the key gap, and only then does the runner announce itself.
    // The answer is a terminal interaction only: no ledger event
    // comes of it.
    let screen = wait_read(&client, "bl-acme-shop-producer", "❯ No, exit").await;
    assert!(screen.contains("Quick safety check"), "{screen}");
    wait_read(
        &client,
        "bl-acme-shop-producer",
        "❯ Yes, I trust this folder",
    )
    .await;
    wait_read(
        &client,
        "bl-acme-shop-producer",
        "fake runner: producer of acme-shop",
    )
    .await;
    // Nothing was sent inside the dialog's unsafe first second —
    // the fake logs any early byte it drains.
    assert!(
        !state.join("fake-early-keys").exists(),
        "a key reached the dialog inside its unsafe first second: {}",
        fs::read_to_string(state.join("fake-early-keys")).unwrap_or_default()
    );
    let events_then = ledger(&state).len();
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(
        ledger(&state).len(),
        events_then,
        "the trust answer changed the ledger: {:?}",
        ledger(&state)
    );

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

    // A second studio beside acme-shop: `acme`'s names share a
    // prefix with `acme-shop`'s, and all four role terminals must
    // stay live — ownership is the label, not the name.
    bl(
        &scratch,
        &state,
        &format!(
            "studio new --studio acme --title acme --repo {} --cast {}",
            scratch.work.display(),
            cast.display()
        ),
    );
    let p2 = wait_terminal(&client, "bl-acme-producer").await;
    let d2 = wait_terminal(&client, "bl-acme-director").await;
    check_labels(&p2, "acme", "producer", None);
    check_labels(&d2, "acme", "director", None);

    // A cast starts an actor; a recast hands the name to c2.
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
    let first = wait_terminal(&client, "bl-acme-shop-t1").await;
    check_labels(&first, "acme-shop", "actor-t1-c1", Some("t1"));

    // The recast closes c1 and starts c2 under the shared name. c1's
    // stop finishes only its own terminal: c2 stays live across the
    // following ticks and is never recorded lost.
    bl(
        &scratch,
        &state,
        "cast --task t1 --actor actor-b --reason recast --as director",
    );
    wait_event(&state, "role.started", "actor-t1-c2");
    let second = wait_replaced(&client, "bl-acme-shop-t1", &first.address).await;
    check_labels(&second, "acme-shop", "actor-t1-c2", Some("t1"));
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(
        terminal_named(&client, "bl-acme-shop-t1")
            .await
            .is_some_and(|t| t.address == second.address),
        "the recast actor did not stay live"
    );
    // Both studios' fixed roles are still up — neither killed the
    // other's, and no `bl-`-named user's terminal is touched.
    for name in [
        "bl-acme-shop-producer",
        "bl-acme-shop-director",
        "bl-acme-producer",
        "bl-acme-director",
    ] {
        assert!(
            terminal_named(&client, name).await.is_some(),
            "{name} is gone"
        );
    }

    // The summary document appears and names the studio.
    let (_, doc) = wait_doc(&client).await;
    assert!(
        doc["studios"]
            .as_array()
            .is_some_and(|ss| ss.iter().any(|s| s["id"] == "acme-shop")),
        "{doc}"
    );

    // The work sample (R17): the fake's announce screen reads
    // `waiting`, and `work:` lines move it through `working` to
    // `blocked` — the summary carries the classification, never
    // the ledger.
    wait_role(&client, "actor-t1-c2", |r| r["work"] == "waiting").await;
    send_text(&client, &second.address, "work:working\n").await;
    wait_role(&client, "actor-t1-c2", |r| r["work"] == "working").await;
    send_text(&client, &second.address, "work:blocked\n").await;
    let c2 = wait_role(&client, "actor-t1-c2", |r| r["work"] == "blocked").await;
    assert_eq!(c2["task"], "t1", "{c2}");
    assert_eq!(c2["priority"], "normal", "{c2}");

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
    // Both casts closed deliberately: c1 at the recast, c2 at the
    // task close — and nobody was ever recorded lost.
    for role in ["actor-t1-c1", "actor-t1-c2"] {
        assert!(
            events
                .iter()
                .any(|e| kind(e) == "role.closed" && e["role"] == role),
            "no role.closed for {role}: {events:?}"
        );
    }
    assert!(
        !events.iter().any(|e| kind(e) == "role.lost"),
        "role.lost present: {events:?}"
    );
    let started: Vec<&serde_json::Value> = events
        .iter()
        .filter(|e| kind(e) == "role.started")
        .collect();
    assert!(started.len() >= 5, "{events:?}");
    for e in &started {
        assert!(
            e["terminal"].as_str().is_some_and(|s| !s.is_empty()),
            "role.started without an address: {e}"
        );
    }

    // status --json still shows the closed roles and their session id.
    let status = bl(&scratch, &state, "status --json");
    let status: serde_json::Value = serde_json::from_str(&status).unwrap();
    for role in ["actor-t1-c1", "actor-t1-c2"] {
        let closed = &status["roles"][role];
        assert_eq!(closed["closed"], true, "{closed}");
        assert_eq!(closed["live"], false, "{closed}");
        assert_eq!(
            closed["session_id"].as_str(),
            Some("fake-bl-acme-shop-t1"),
            "{closed}"
        );
    }

    // The document saw the close too.
    let deadline = Instant::now() + DEADLINE;
    loop {
        let (_, doc) = wait_doc(&client).await;
        let studio = doc["studios"]
            .as_array()
            .and_then(|ss| ss.iter().find(|s| s["id"] == "acme-shop"));
        let closed_role = studio.and_then(|s| {
            s["roles"]
                .as_array()
                .and_then(|rs| rs.iter().find(|r| r["role"] == "actor-t1-c2"))
        });
        if closed_role.is_some_and(|r| r["state"] == "closed" && r["doing"].is_null()) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "closed actor never reached the doc: {doc}"
        );
        tokio::time::sleep(POLL).await;
    }

    // A closed person with a session is `resumable` — and the
    // document itself never carries a session id.
    let c2 = wait_role(&client, "actor-t1-c2", |r| {
        r["state"] == "closed" && r["resumable"] == true
    })
    .await;
    assert_eq!(c2["task"], "t1", "{c2}");
    let (_, doc) = wait_doc(&client).await;
    let json = serde_json::to_string(&doc).unwrap();
    assert!(
        !json.contains("fake-bl"),
        "a session id reached the document: {json}"
    );

    // `backlot reopen` holds the closed person; the stage resumes it
    // into the same fake session under `role.resumed`. `backlot
    // close` releases it and the usual Stop path closes it again.
    bl(&scratch, &state, "reopen --person actor-t1-c2 --as user");
    let resumed = wait_event(&state, "role.resumed", "actor-t1-c2");
    assert_eq!(resumed["reason"], "reopened", "{resumed}");
    assert_eq!(resumed["session_id"], "fake-bl-acme-shop-t1", "{resumed}");
    let back = wait_terminal(&client, "bl-acme-shop-t1").await;
    check_labels(&back, "acme-shop", "actor-t1-c2", Some("t1"));
    let status = bl(&scratch, &state, "status --json");
    let status: serde_json::Value = serde_json::from_str(&status).unwrap();
    let c2s = &status["roles"]["actor-t1-c2"];
    assert_eq!(c2s["held"], true, "{c2s}");
    assert_eq!(c2s["live"], true, "{c2s}");

    bl(&scratch, &state, "close --person actor-t1-c2 --as user");
    wait_absent(&client, "bl-acme-shop-t1").await;
    wait_events(&state, "role.closed", "actor-t1-c2", 2);
    let status = bl(&scratch, &state, "status --json");
    let status: serde_json::Value = serde_json::from_str(&status).unwrap();
    let c2s = &status["roles"]["actor-t1-c2"];
    assert_eq!(c2s["held"], false, "{c2s}");
    assert_eq!(c2s["closed"], true, "{c2s}");

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
    let shop = doc["studios"]
        .as_array()
        .and_then(|ss| ss.iter().find(|s| s["id"] == "acme-shop"))
        .expect("acme-shop in the doc");
    assert_eq!(shop["requirement_version"], 2, "{doc}");

    drop(client);
    scratch.finish().expect("scratch teardown");
}
