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
use hc_proto::terminal::{ListParams, ReadParams, Terminal, TerminalParams};

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

/// `backlot <args>` under the given env, returned raw — `bl` and
/// `bl_fail` assert on the exit from here.
fn bl_run(env: Vec<(String, String)>, args: &str) -> std::process::Output {
    let mut child = Command::new(tui_bin())
        .args(args.split(' '))
        .env_clear()
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("cannot spawn backlot {args}: {e}"));
    let deadline = Instant::now() + DEADLINE;
    loop {
        if child.try_wait().expect("try_wait").is_some() {
            return child.wait_with_output().expect("output");
        }
        assert!(Instant::now() < deadline, "backlot {args} timed out");
        std::thread::sleep(POLL);
    }
}

/// `backlot <args>` under the fixture env; assert exit 0 and return
/// stdout.
fn bl(scratch: &ScratchCore, state: &Path, args: &str) -> String {
    let out = bl_run(bl_env(scratch, state), args);
    assert!(
        out.status.success(),
        "backlot {args} exited {}; stderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// `bl` without the `BACKLOT_STUDIO` pin — the fixture pins every
/// call to `acme-shop`, but only an unpinned shell may `--studio`
/// another studio.
fn bl_unpinned(scratch: &ScratchCore, state: &Path, args: &str) -> String {
    let env = bl_env(scratch, state)
        .into_iter()
        .filter(|(k, _)| k != "BACKLOT_STUDIO")
        .collect();
    let out = bl_run(env, args);
    assert!(
        out.status.success(),
        "backlot {args} exited {}; stderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// `backlot <args>` expected to refuse; assert a non-zero exit and
/// return stderr.
fn bl_fail(scratch: &ScratchCore, state: &Path, args: &str) -> String {
    let out = bl_run(bl_env(scratch, state), args);
    assert!(
        !out.status.success(),
        "backlot {args} should refuse, got: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    String::from_utf8_lossy(&out.stderr).into_owned()
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

/// `terminal.keys` — named keys, the way a person types them or the
/// stage answers a permission prompt.
async fn send_keys(client: &Client, terminal: &str, keys: &[&str]) {
    client
        .call(
            "terminal.keys",
            Some(serde_json::json!({"terminal": terminal, "keys": keys})),
        )
        .await
        .expect("terminal.keys");
}

/// `terminal.read` until `marker` shows `n` times — counting the
/// marker is how "no key reached the prompt" is proven.
async fn wait_count(client: &Client, terminal: &str, marker: &str, n: usize) -> String {
    let deadline = Instant::now() + DEADLINE;
    loop {
        let text = client
            .terminal_read(ReadParams {
                terminal: terminal.into(),
                lines: 0,
                caller: None,
            })
            .await
            .map(|r| r.text)
            .unwrap_or_default();
        if text.matches(marker).count() >= n {
            return text;
        }
        assert!(
            Instant::now() < deadline,
            "{terminal} never showed {marker:?} {n} times; screen: {text:?}"
        );
        tokio::time::sleep(POLL).await;
    }
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
    // D10: a studio repo is a git work tree — the stage starts the
    // director's integration worktree off it.
    let out = Command::new("git")
        .args(["init", "-b", "main"])
        .arg(&scratch.work)
        .output()
        .expect("git init");
    assert!(out.status.success(), "git init failed");
    git(&scratch.work, &["commit", "--allow-empty", "-m", "init"]);
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
    // stay live — ownership is the label, not the name. Naming
    // another studio needs an unpinned shell.
    bl_unpinned(
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
    send_text(&client, &second.address, "work:working").await;
    wait_role(&client, "actor-t1-c2", |r| r["work"] == "working").await;
    send_text(&client, &second.address, "work:blocked").await;
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

/// The ledger reaches `n` events of `kind` regardless of role —
/// for kinds like `attention.cleared` that carry no `role` field.
fn wait_kind(state: &Path, kind: &str, n: usize) {
    let deadline = Instant::now() + DEADLINE;
    loop {
        let count = ledger(state).iter().filter(|e| e["kind"] == kind).count();
        if count >= n {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "wanted {n} {kind} events in the ledger"
        );
        std::thread::sleep(POLL);
    }
}

/// `git` in the fixture repo — the work-area assertions run real
/// git against it; commits get a fixture identity through the env.
fn git(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_AUTHOR_NAME", "alice")
        .env("GIT_AUTHOR_EMAIL", "alice@example.com")
        .env("GIT_COMMITTER_NAME", "alice")
        .env("GIT_COMMITTER_EMAIL", "alice@example.com")
        .output()
        .unwrap_or_else(|e| panic!("cannot run git {args:?}: {e}"));
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// An empty commit on the current branch of `dir` — a work area of
/// the fixture repo — answering the new HEAD sha.
fn commit_in(dir: &Path, msg: &str) -> String {
    git(dir, &["commit", "--allow-empty", "-m", msg]);
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

/// The fake runner logs every line it is sent into
/// `fake-inbox-<role>` under the state dir — wait until `needle`
/// shows in the file `name`.
fn wait_inbox(state: &Path, name: &str, needle: &str) -> String {
    let file = state.join(name);
    let deadline = Instant::now() + DEADLINE;
    loop {
        let text = fs::read_to_string(&file).unwrap_or_default();
        if text.contains(needle) {
            return text;
        }
        assert!(
            Instant::now() < deadline,
            "{name} never contained {needle:?}; inbox: {text:?}"
        );
        std::thread::sleep(POLL);
    }
}

/// The S5 closed loop on the fake runner: a requirement comes in,
/// the writer is summoned and told, the director is dispatched, an
/// actor works its own worktree, a delivery notices the director, a
/// rework review notices the actor, a midway revision notices the
/// director, a permission prompt is answered through `backlot
/// approve`, the accepted actor closes resumable — and `studio
/// finish` ends the studio.
#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn real_stage_closed_loop() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let state = scratch.root.join("bl-state");
    let studio = state.join("studios/acme-shop");
    // D10: the studio repo is a real git work tree.
    let repo = scratch.work.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let out = Command::new("git")
        .args(["init", "-b", "main"])
        .arg(&repo)
        .output()
        .expect("git init");
    assert!(out.status.success(), "git init failed");
    git(&repo, &["commit", "--allow-empty", "-m", "init"]);
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    let req = fixture(&scratch, "req.md", "# requirement\nbuild the shop\n");
    let req2 = fixture(&scratch, "req2.md", "# requirement v2\nbigger shop\n");
    let acc = fixture(&scratch, "acc.md", "# acceptance\n");
    let note = fixture(&scratch, "note.md", "director: go\n");
    let report = fixture(&scratch, "report.md", "# delivery report\n");

    bl(
        &scratch,
        &state,
        &format!(
            "studio new --title acme-shop --repo {} --cast {}",
            repo.display(),
            cast.display()
        ),
    );
    bl(&scratch, &state, "stage start");

    // Producer runs in the repo itself; the director's terminal is
    // in the integration worktree on the integration branch.
    wait_terminal(&client, "bl-acme-shop-producer").await;
    let director = wait_terminal(&client, "bl-acme-shop-director").await;
    assert_eq!(
        director.cwd.as_str(),
        studio.join("work/integration").to_str().unwrap()
    );
    assert!(
        git(
            &repo,
            &["branch", "--list", "backlot/acme-shop/integration"]
        )
        .contains("integration"),
        "no integration branch"
    );

    // Requirement in, then the plan request: the writer starts and
    // the stage sends it the file-less `[backlot]` line.
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
    wait_inbox(
        &state,
        "fake-inbox-writer",
        "a plan and an acceptance list are wanted",
    );

    // Dispatch: the director gets the producer's file; the writer
    // is no longer wanted.
    bl(
        &scratch,
        &state,
        &format!("dispatch --file {} --as producer", note.display()),
    );
    let inbox = wait_inbox(
        &state,
        "fake-inbox-director",
        "[backlot] producer → director: read ",
    );
    assert!(inbox.contains("-note.md"), "{inbox}");
    wait_absent(&client, "bl-acme-shop-writer").await;

    // A task and a cast: the actor lands in its own worktree on its
    // own branch, branched off the integration tip.
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
    let actor = wait_terminal(&client, "bl-acme-shop-t1").await;
    check_labels(&actor, "acme-shop", "actor-t1-c1", Some("t1"));
    assert_eq!(actor.cwd.as_str(), studio.join("work/t1").to_str().unwrap());
    assert!(
        git(&repo, &["branch", "--list", "backlot/acme-shop/t1"]).contains("t1"),
        "no task branch"
    );

    // The first delivery owes the director a notice naming the
    // generated body file.
    let work = studio.join("work/t1");
    let a = commit_in(&work, "a");
    let b = commit_in(&work, "b");
    bl(
        &scratch,
        &state,
        &format!(
            "deliver --commits {a}..{b} --report {} --check build=pass --as actor-t1-c1 --task t1",
            report.display()
        ),
    );
    wait_inbox(&state, "fake-inbox-director", "-delivered.md");

    // A review asking for changes owes the actor a rework notice.
    bl(
        &scratch,
        &state,
        "review --task t1 --delivery 1 --verdict changes-requested --conclusion fix-it --as director",
    );
    wait_inbox(&state, "fake-inbox-actor-t1", "-rework.md");

    // The requirement moves mid-loop: the director is told which
    // tasks are now behind.
    bl(
        &scratch,
        &state,
        &format!(
            "req revise --file {} --summary v2 --as producer",
            req2.display()
        ),
    );
    wait_inbox(&state, "fake-inbox-director", "-req.md");

    // A permission prompt: two blocked samples raise the attention
    // with the screen saved under `attention/`; the recorded
    // decision sends the key while the prompt still shows.
    // `work:blocked:2` makes the fake answer the first approval
    // with a new `fake-work: blocked` — a second prompt already up,
    // no unblocked sample between the two. Its attention needs the
    // whole window counted from the first key, not the first
    // episode's start: before the fix the second count never
    // began.
    send_text(&client, &actor.address, "work:blocked:2").await;
    let raised = wait_event(&state, "attention.raised", "actor-t1-c1");
    assert_eq!(raised["reason"], "permission prompt", "{raised}");
    let screen_file = studio.join(format!("attention/{}.txt", raised["seq"]));
    wait_inbox(&state, "fake-inbox-director", "-attention.md");
    assert!(
        fs::read_to_string(&screen_file)
            .unwrap_or_default()
            .contains("fake-work: blocked"),
        "the saved screen: {}",
        screen_file.display()
    );
    // No key is sent before a decision is recorded; once it is,
    // `enter` reaches the prompt and the attention clears. The
    // role's work doc never shows `waiting` after the key: the next
    // prompt's blocked screen is already there.
    bl(
        &scratch,
        &state,
        "approve --person actor-t1-c1 --decision allow --as director",
    );
    wait_read(&client, &actor.address, "fake-approved: enter").await;
    wait_kind(&state, "attention.cleared", 1);

    // The second, already-open prompt raises its own attention on
    // the still-blocked screen; `deny` sends esc, and the denied
    // role is owed a notice telling it the command did not run —
    // an `esc` interruption alone would leave it waiting forever.
    wait_events(&state, "attention.raised", "actor-t1-c1", 2);
    bl(
        &scratch,
        &state,
        "approve --person actor-t1-c1 --decision deny --as director",
    );
    wait_read(&client, &actor.address, "fake-approved: esc").await;
    wait_kind(&state, "attention.cleared", 2);
    wait_inbox(&state, "fake-inbox-actor-t1", "-denied.md");

    // `studio finish` names the tasks still open; once accepted the
    // actor closes deliberately and stays resumable.
    let refused = bl_fail(&scratch, &state, "studio finish --as producer");
    assert!(refused.contains("t1"), "{refused}");
    let c = commit_in(&work, "c");
    bl(
        &scratch,
        &state,
        &format!(
            "deliver --commits {b}..{c} --report {} --as actor-t1-c1 --task t1",
            report.display()
        ),
    );
    bl(
        &scratch,
        &state,
        "review --task t1 --delivery 2 --verdict accepted --conclusion done --as director",
    );
    wait_absent(&client, "bl-acme-shop-t1").await;
    let closed = wait_role(&client, "actor-t1-c1", |r| {
        r["state"] == "closed" && r["resumable"] == true
    })
    .await;
    assert_eq!(closed["task"], "t1", "{closed}");

    // Finishing ends the usual scheduling: producer and director
    // close; `studio.finished` is in the ledger.
    bl(
        &scratch,
        &state,
        "studio finish --reason accepted --as producer",
    );
    wait_kind(&state, "studio.finished", 1);
    wait_absent(&client, "bl-acme-shop-producer").await;
    wait_absent(&client, "bl-acme-shop-director").await;

    // Reopen still works after finish: the closed actor resumes its
    // session in the same worktree.
    bl(&scratch, &state, "reopen --person actor-t1-c1 --as user");
    wait_event(&state, "role.resumed", "actor-t1-c1");
    let back = wait_terminal(&client, "bl-acme-shop-t1").await;
    assert_eq!(back.cwd.as_str(), studio.join("work/t1").to_str().unwrap());

    drop(client);
    scratch.finish().expect("scratch teardown");
}

/// S9c: an approval recorded against an older prompt never reaches
/// the newer one. Prompt A is answered in the terminal itself, and
/// prompt B is already up before the stale attention's episode
/// window runs out — the director's `backlot approve` records
/// against A's still-open attention, and the block comparison is
/// what saves B from A's key: nothing is sent, A's attention clears
/// `prompt changed before the decision was sent`, and B raises its
/// own attention with its own saved screen.
#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn real_stage_approve_lands_on_the_prompt_it_was_made_for() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let state = scratch.root.join("bl-state");
    let studio = checks_studio(&scratch, &state, "", "");
    let actor = wait_terminal(&client, "bl-acme-shop-t1").await;

    // Prompt A: `prompt:` gives it its own text on screen, and the
    // attention saves that screen.
    send_text(&client, &actor.address, "prompt:rm -rf build").await;
    send_text(&client, &actor.address, "work:blocked").await;
    let raised_a = wait_event(&state, "attention.raised", "actor-t1-c1");
    assert_eq!(raised_a["reason"], "permission prompt", "{raised_a}");
    let seq_a = raised_a["seq"].as_u64().unwrap();
    let screen_a = studio.join(format!("attention/{seq_a}.txt"));
    let saved_a = fs::read_to_string(&screen_a).unwrap();
    assert!(saved_a.contains("fake-prompt: rm -rf build"), "{saved_a}");

    // A is answered in the terminal itself — a person typing enter.
    send_keys(&client, &actor.address, &["enter"]).await;
    wait_read(&client, &actor.address, "fake-approved: enter").await;

    // B is up inside the stale attention's window.
    send_text(&client, &actor.address, "prompt:cargo test --lib").await;
    send_text(&client, &actor.address, "work:blocked").await;
    wait_read(&client, &actor.address, "fake-prompt: cargo test --lib").await;

    // The director approves the prompt it was told about — A's.
    bl(
        &scratch,
        &state,
        "approve --person actor-t1-c1 --decision allow --as director",
    );

    // No key reaches B: A's attention clears `prompt changed`, and B
    // raises its own attention, `needs_user` computed on B's screen.
    wait_kind(&state, "attention.cleared", 1);
    let cleared_a = ledger(&state)
        .into_iter()
        .find(|e| e["kind"] == "attention.cleared")
        .expect("cleared event");
    assert_eq!(cleared_a["target"], seq_a, "{cleared_a}");
    assert_eq!(
        cleared_a["reason"], "prompt changed before the decision was sent",
        "{cleared_a}"
    );
    wait_events(&state, "attention.raised", "actor-t1-c1", 2);
    let raised = ledger(&state)
        .into_iter()
        .filter(|e| e["kind"] == "attention.raised" && e["role"] == "actor-t1-c1")
        .collect::<Vec<_>>();
    assert_eq!(raised.len(), 2, "{raised:?}");
    let raised_b = &raised[1];
    let seq_b = raised_b["seq"].as_u64().unwrap();
    assert_ne!(seq_b, seq_a);
    assert_eq!(raised_b["reason"], "permission prompt", "{raised_b}");
    let detail_b = raised_b["detail"].as_str().unwrap_or_default();
    assert!(
        detail_b.contains(&format!("attention/{seq_b}.txt")),
        "{detail_b}"
    );
    let saved_b = fs::read_to_string(studio.join(format!("attention/{seq_b}.txt"))).unwrap();
    assert!(
        saved_b.contains("fake-prompt: cargo test --lib"),
        "{saved_b}"
    );

    // Give any mistaken send a few samples to happen, then prove it
    // never did: one `fake-approved` — A's own answer — is all there
    // is. B's own approval then lands on B, so the key path still
    // works when the blocks match.
    tokio::time::sleep(Duration::from_secs(3)).await;
    let screen = client
        .terminal_read(ReadParams {
            terminal: actor.address.clone(),
            lines: 0,
            caller: None,
        })
        .await
        .map(|r| r.text)
        .unwrap_or_default();
    assert_eq!(
        screen.matches("fake-approved").count(),
        1,
        "a key reached B: {screen:?}"
    );
    bl(
        &scratch,
        &state,
        "approve --person actor-t1-c1 --decision allow --as director",
    );
    wait_count(&client, &actor.address, "fake-approved: enter", 2).await;
}

/// The same race where B's prompt text reaches for `BACKLOT_*`: the
/// fresh attention computes `needs_user` on B's own screen — the
/// stale one does not shield it — and the director's approve is the
/// user's to give.
#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn real_stage_approve_recomputes_needs_user() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let state = scratch.root.join("bl-state");
    let studio = checks_studio(&scratch, &state, "", "");
    let actor = wait_terminal(&client, "bl-acme-shop-t1").await;

    send_text(&client, &actor.address, "prompt:cargo fmt").await;
    send_text(&client, &actor.address, "work:blocked").await;
    let raised_a = wait_event(&state, "attention.raised", "actor-t1-c1");
    assert_ne!(raised_a["needs_user"], true, "{raised_a}");

    send_keys(&client, &actor.address, &["enter"]).await;
    wait_read(&client, &actor.address, "fake-approved: enter").await;
    send_text(
        &client,
        &actor.address,
        "prompt:BACKLOT_ROLE=director backlot review --task t1",
    )
    .await;
    send_text(&client, &actor.address, "work:blocked").await;
    wait_read(&client, &actor.address, "fake-prompt: BACKLOT_ROLE=").await;

    // A's stale attention is unflagged, so the director's approve
    // records — but it is A's decision, not B's: nothing is sent,
    // and B raises an attention that is the user's.
    bl(
        &scratch,
        &state,
        "approve --person actor-t1-c1 --decision allow --as director",
    );
    wait_events(&state, "attention.raised", "actor-t1-c1", 2);
    let raised = ledger(&state)
        .into_iter()
        .filter(|e| e["kind"] == "attention.raised" && e["role"] == "actor-t1-c1")
        .collect::<Vec<_>>();
    assert_eq!(raised[1]["needs_user"], true, "{}", raised[1]);
    let seq_b = raised[1]["seq"].as_u64().unwrap();
    let saved_b = fs::read_to_string(studio.join(format!("attention/{seq_b}.txt"))).unwrap();
    assert!(saved_b.contains("BACKLOT_ROLE=director"), "{saved_b}");

    let out = bl_fail(
        &scratch,
        &state,
        "approve --person actor-t1-c1 --decision allow --as director",
    );
    assert!(out.contains("is the user's"), "{out}");

    // The user's own approve is B's answer and sends the key.
    bl(
        &scratch,
        &state,
        "approve --person actor-t1-c1 --decision allow --as user",
    );
    wait_count(&client, &actor.address, "fake-approved: enter", 2).await;
}

/// A prompt answered in the terminal itself: with no decision
/// pending, its attention stays stale only until the role has been
/// out of the blocked episode for a whole window — then it clears
/// `prompt answered elsewhere`, and the next prompt raises an
/// attention of its own.
#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn real_stage_prompt_answered_elsewhere_clears() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let state = scratch.root.join("bl-state");
    checks_studio(&scratch, &state, "", "");
    let actor = wait_terminal(&client, "bl-acme-shop-t1").await;

    send_text(&client, &actor.address, "work:blocked").await;
    wait_event(&state, "attention.raised", "actor-t1-c1");

    // The person answers the prompt in the terminal — no approval
    // is ever recorded. After a window out of the episode the stale
    // attention clears.
    send_keys(&client, &actor.address, &["enter"]).await;
    wait_read(&client, &actor.address, "fake-approved: enter").await;
    wait_kind(&state, "attention.cleared", 1);
    let cleared = ledger(&state)
        .into_iter()
        .find(|e| e["kind"] == "attention.cleared")
        .expect("cleared event");
    assert_eq!(cleared["reason"], "prompt answered elsewhere", "{cleared}");

    // The next prompt raises its own attention — not swallowed by
    // the cleared one's still-open record.
    send_text(&client, &actor.address, "prompt:cargo test --lib").await;
    send_text(&client, &actor.address, "work:blocked").await;
    wait_events(&state, "attention.raised", "actor-t1-c1", 2);
}

/// S9d — the S8 bug: the director is `work:working` while the actor's
/// prompt raises its attention and queues the notice; the user's
/// approval clears the attention, and when the director goes
/// `work:waiting` the ledger holds `message.dropped` with `attention
/// cleared` — the director's inbox gets nothing.
#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn real_stage_stale_notice_drops() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let state = scratch.root.join("bl-state");
    checks_studio(&scratch, &state, "", "");
    let director = wait_terminal(&client, "bl-acme-shop-director").await;
    let actor = wait_terminal(&client, "bl-acme-shop-t1").await;

    // The director is busy while the actor hits a permission prompt:
    // the notice queues but the send gate never opens.
    send_text(&client, &director.address, "work:working").await;
    wait_role(&client, "director", |r| r["work"] == "working").await;
    send_text(&client, &actor.address, "work:blocked").await;
    let raised = wait_event(&state, "attention.raised", "actor-t1-c1");
    assert_eq!(raised["reason"], "permission prompt", "{raised}");
    let queued = {
        let deadline = Instant::now() + DEADLINE;
        loop {
            if let Some(e) = ledger(&state)
                .into_iter()
                .find(|e| e["kind"] == "message.queued" && e["source_seq"] == raised["seq"])
            {
                break e;
            }
            assert!(
                Instant::now() < deadline,
                "the director's notice never queued"
            );
            std::thread::sleep(POLL);
        }
    };

    // The user approves: the prompt's key goes in, the attention
    // clears — and the queued notice's source is gone.
    bl(
        &scratch,
        &state,
        "approve --person actor-t1-c1 --decision allow --as user",
    );
    wait_read(&client, &actor.address, "fake-approved: enter").await;
    wait_kind(&state, "attention.cleared", 1);

    // The director goes waiting: the drop lands instead of any send.
    send_text(&client, &director.address, "work:waiting").await;
    wait_kind(&state, "message.dropped", 1);
    let dropped = ledger(&state)
        .into_iter()
        .find(|e| e["kind"] == "message.dropped")
        .expect("message.dropped");
    assert_eq!(dropped["target"], queued["seq"], "{dropped}");
    assert_eq!(dropped["reason"], "attention cleared", "{dropped}");
    assert_eq!(dropped["by"]["role"], "stage", "{dropped}");

    // Past every send gate: the notice never reached the director —
    // no `message.delivered` targets it either.
    std::thread::sleep(Duration::from_secs(7));
    let inbox = fs::read_to_string(state.join("fake-inbox-director")).unwrap_or_default();
    assert!(!inbox.contains("attention"), "{inbox}");
    assert!(
        !ledger(&state)
            .iter()
            .any(|e| e["kind"] == "message.delivered" && e["target"] == queued["seq"]),
        "the dropped notice must not be marked delivered"
    );

    drop(client);
    scratch.finish().expect("scratch teardown");
}

/// S9d — several pending items go as one send: while the director
/// is `work:working` a `tell` and two deliveries on different
/// tasks queue for it; when it goes `work:waiting` a single
/// `[backlot]` line names `messages/digest-<top>.md (3 items)`,
/// the file lists both `delivered` notices ahead of the tell —
/// the priority order, not the ledger's, since the tell queued
/// first — and every item gets its `message.delivered`. Two
/// tasks, not two deliveries on one — the second delivery on a
/// task supersedes the first's notice.
#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn real_stage_digest_sends_once() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let state = scratch.root.join("bl-state");
    let studio = checks_studio(&scratch, &state, "", "");
    let acc = fixture(&scratch, "acc.md", "# acceptance\n");
    let note = fixture(&scratch, "note.md", "director: go\n");
    let report = fixture(&scratch, "report.md", "# delivery report\n");

    bl(
        &scratch,
        &state,
        &format!(
            "task new --title nav --kind feature --acceptance-file {} --as director",
            acc.display()
        ),
    );
    bl(
        &scratch,
        &state,
        "cast --task t2 --actor actor-b --reason second --as director",
    );
    wait_path(&studio.join("work/t2"));

    // The director is busy while all three items queue — the send
    // gate never opens on `working`.
    let director = wait_terminal(&client, "bl-acme-shop-director").await;
    send_text(&client, &director.address, "work:working").await;
    wait_role(&client, "director", |r| r["work"] == "working").await;

    // The tell queues first — its low seq fights the priority order
    // the digest must follow.
    bl(
        &scratch,
        &state,
        &format!("tell director --file {} --as writer", note.display()),
    );
    // The `bl` call already appended — the tell is the director's
    // queued message with no source_seq and no purpose.
    let tell = ledger(&state)
        .into_iter()
        .find(|e| {
            e["kind"] == "message.queued"
                && e["to"] == "director"
                && e["source_seq"].is_null()
                && e["purpose"].is_null()
        })
        .and_then(|e| e["seq"].as_u64())
        .expect("the tell's queued message");

    // Each delivery's `delivered` notice must queue before the next
    // lands, so the two notice seqs stay in order.
    let mut notices: Vec<u64> = Vec::new();
    for (task, actor) in [("t1", "actor-t1-c1"), ("t2", "actor-t2-c1")] {
        let work = studio.join(format!("work/{task}"));
        let (a, b) = (commit_in(&work, "a"), commit_in(&work, "b"));
        bl(
            &scratch,
            &state,
            &format!(
                "deliver --commits {a}..{b} --report {} --as {actor} --task {task}",
                report.display()
            ),
        );
        let deadline = Instant::now() + DEADLINE;
        loop {
            let evs = ledger(&state);
            let src = evs
                .iter()
                .find(|e| e["kind"] == "task.delivered" && e["task"] == task)
                .and_then(|e| e["seq"].as_u64());
            let queued = src.and_then(|s| {
                evs.iter()
                    .find(|e| e["kind"] == "message.queued" && e["source_seq"] == s)
            });
            if let Some(q) = queued {
                notices.push(q["seq"].as_u64().unwrap());
                break;
            }
            assert!(
                Instant::now() < deadline,
                "delivery {task}'s notice never queued"
            );
            std::thread::sleep(POLL);
        }
    }
    let top = notices.iter().copied().chain([tell]).max().unwrap();

    // Waiting opens the gate: one send, one line, one digest file.
    send_text(&client, &director.address, "work:waiting").await;
    let inbox = wait_inbox(&state, "fake-inbox-director", "digest-");
    let sends: Vec<&str> = inbox.lines().filter(|l| l.contains("[backlot]")).collect();
    assert_eq!(sends.len(), 1, "one send covered all three: {inbox}");
    assert!(sends[0].contains("stage → director"), "{inbox}");
    assert!(sends[0].contains("(3 items)"), "{inbox}");

    // The digest lists the delivered notices ahead of the tell.
    let body =
        fs::read_to_string(studio.join(format!("messages/digest-{top}.md"))).expect("digest file");
    let at = |s: String| {
        body.find(&s)
            .unwrap_or_else(|| panic!("{s} missing\n{body}"))
    };
    let (a, b, c) = (
        at(format!("## delivered (seq {})", notices[0])),
        at(format!("## delivered (seq {})", notices[1])),
        at(format!("## tell (seq {tell})")),
    );
    assert!(a < b && b < c, "{body}");

    // Every item the send covered is recorded delivered.
    let wanted: std::collections::BTreeSet<u64> = notices.iter().copied().chain([tell]).collect();
    wait_kind(&state, "message.delivered", 3);
    let got: std::collections::BTreeSet<u64> = ledger(&state)
        .iter()
        .filter(|e| e["kind"] == "message.delivered")
        .filter_map(|e| e["target"].as_u64())
        .collect();
    assert!(wanted.is_subset(&got), "{got:?} misses {wanted:?}");

    drop(client);
    scratch.finish().expect("scratch teardown");
}

/// The S9a nudge on the fake runner with short limits: the plan and
/// acceptance list are published and nobody answers — the owed
/// `dispatch` step shows in `backlot status`, the idle producer's
/// terminal gets the one `nudge` message, and a `dispatch` makes the
/// step disappear again.
#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn real_stage_nudge() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let state = scratch.root.join("bl-state");
    let repo = scratch.work.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let out = Command::new("git")
        .args(["init", "-b", "main"])
        .arg(&repo)
        .output()
        .expect("git init");
    assert!(out.status.success(), "git init failed");
    git(&repo, &["commit", "--allow-empty", "-m", "init"]);
    // Seconds, not minutes: the idle stretch passes inside a test.
    let cast = fixture(
        &scratch,
        "cast.toml",
        &format!("nudge_after_s = 2\nescalate_after_s = 4\n{CAST_TOML}"),
    );
    let req = fixture(&scratch, "req.md", "# requirement\nbuild the shop\n");
    let plan = fixture(&scratch, "plan.md", "# plan\n");
    let acc = fixture(&scratch, "acc.md", "# acceptance\n");
    let note = fixture(&scratch, "note.md", "director: go\n");

    bl(
        &scratch,
        &state,
        &format!(
            "studio new --title acme-shop --repo {} --cast {}",
            repo.display(),
            cast.display()
        ),
    );
    bl(&scratch, &state, "stage start");
    wait_read(
        &client,
        "bl-acme-shop-producer",
        "fake runner: producer of acme-shop",
    )
    .await;
    bl(
        &scratch,
        &state,
        &format!(
            "req revise --file {} --summary v1 --as producer",
            req.display()
        ),
    );
    bl(&scratch, &state, "plan request --as producer");
    wait_terminal(&client, "bl-acme-shop-writer").await;
    bl(
        &scratch,
        &state,
        &format!(
            "plan publish --kind plan --file {} --as writer",
            plan.display()
        ),
    );
    bl(
        &scratch,
        &state,
        &format!(
            "plan publish --kind acceptance --file {} --as writer",
            acc.display()
        ),
    );

    // The producer owes the dispatch — status says so, and after
    // `nudge_after_s` of its idle stretch the nudge lands in its
    // terminal inbox, exactly once.
    let status = bl(&scratch, &state, "status");
    assert!(status.contains("next: dispatch — producer"), "{status}");
    wait_inbox(&state, "fake-inbox-producer", "-nudge.md");
    assert_eq!(
        ledger(&state)
            .iter()
            .filter(|e| e["kind"] == "message.queued" && e["purpose"] == "nudge")
            .count(),
        1,
        "a second nudge queued: {:?}",
        ledger(&state)
    );

    // Dispatch answers the step: the `next: dispatch` line is gone
    // from status (the director's `tasks` step may follow once the
    // dispatch is delivered).
    bl(
        &scratch,
        &state,
        &format!("dispatch --file {} --as producer", note.display()),
    );
    let status = bl(&scratch, &state, "status");
    assert!(!status.contains("next: dispatch"), "{status}");

    drop(client);
    scratch.finish().expect("scratch teardown");
}

/// The S6a watch on the fake runner with a small `stall_limit_s`: a
/// `work:working` actor's frozen screen raises `stalled`, the notice
/// queues for the task's reviewer — the director — and a changed
/// screen clears the episode.
#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn real_stage_watch() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let state = scratch.root.join("bl-state");
    let studio = state.join("studios/acme-shop");
    let repo = scratch.work.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let out = Command::new("git")
        .args(["init", "-b", "main"])
        .arg(&repo)
        .output()
        .expect("git init");
    assert!(out.status.success(), "git init failed");
    git(&repo, &["commit", "--allow-empty", "-m", "init"]);
    // A small stall limit: a frozen screen trips it in seconds.
    let cast = fixture(
        &scratch,
        "cast.toml",
        &format!("stall_limit_s = 5\n{CAST_TOML}"),
    );
    let req = fixture(&scratch, "req.md", "# requirement\nbuild the shop\n");
    let acc = fixture(&scratch, "acc.md", "# acceptance\n");
    let report = fixture(&scratch, "report.md", "# delivery report\n");

    bl(
        &scratch,
        &state,
        &format!(
            "studio new --title acme-shop --repo {} --cast {}",
            repo.display(),
            cast.display()
        ),
    );
    bl(&scratch, &state, "stage start");
    wait_terminal(&client, "bl-acme-shop-producer").await;
    wait_terminal(&client, "bl-acme-shop-director").await;

    bl(
        &scratch,
        &state,
        &format!(
            "req revise --file {} --summary v1 --as producer",
            req.display()
        ),
    );
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
    let actor = wait_terminal(&client, "bl-acme-shop-t1").await;

    // The working screen held past the limit raises `stalled` once:
    // the notice queues for the task's reviewer — the director —
    // and the frozen screen is saved beside the ledger.
    send_text(&client, &actor.address, "work:working").await;
    let raised = wait_event(&state, "attention.raised", "actor-t1-c1");
    assert_eq!(raised["reason"], "stalled", "{raised}");
    let screen_file = studio.join(format!("attention/{}.txt", raised["seq"]));
    assert!(
        fs::read_to_string(&screen_file)
            .unwrap_or_default()
            .contains("fake-work: working"),
        "the saved screen: {}",
        screen_file.display()
    );
    wait_inbox(&state, "fake-inbox-director", "-stalled.md");

    // A changed screen clears the episode.
    send_text(&client, &actor.address, "work:waiting").await;
    wait_kind(&state, "attention.cleared", 1);
    let cleared = ledger(&state)
        .into_iter()
        .find(|e| e["kind"] == "attention.cleared")
        .expect("attention.cleared");
    assert_eq!(cleared["target"], raised["seq"], "{cleared}");
    assert_eq!(cleared["reason"], "screen changed", "{cleared}");

    // A blocked task goes on with the same actor: `task resume`
    // returns it to running, the actor hears the `resumed` notice,
    // and the delivery it then makes lands and reviews normally.
    bl(
        &scratch,
        &state,
        "block --reason needs-an-answer --as actor --task t1",
    );
    wait_inbox(&state, "fake-inbox-director", "-blocked.md");
    bl(
        &scratch,
        &state,
        "task resume --task t1 --reason answered --as director",
    );
    wait_inbox(&state, "fake-inbox-actor-t1", "-resumed.md");
    let work = studio.join("work/t1");
    let a = commit_in(&work, "a");
    let b = commit_in(&work, "b");
    bl(
        &scratch,
        &state,
        &format!(
            "deliver --commits {a}..{b} --report {} --as actor-t1-c1 --task t1",
            report.display()
        ),
    );
    wait_inbox(&state, "fake-inbox-director", "-delivered.md");

    drop(client);
    scratch.finish().expect("scratch teardown");
}

/// The S6b restart on the fake runner: a `server stop` ends the core,
/// every role terminal and the stage itself. A fresh serve on the same
/// session plus `stage start` must recover the studio from its ledger
/// alone: each wanted role records `role.lost` then `role.resumed`
/// `recovered` on the same session, the stale permission attention
/// clears, the `recovered` notices and a message queued while the core
/// was down are delivered, a delivery still lands — and a role closed
/// before the restart stays closed.
#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn real_stage_restart() {
    let mut scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let state = scratch.root.join("bl-state");
    let repo = scratch.work.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let out = Command::new("git")
        .args(["init", "-b", "main"])
        .arg(&repo)
        .output()
        .expect("git init");
    assert!(out.status.success(), "git init failed");
    git(&repo, &["commit", "--allow-empty", "-m", "init"]);
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    let req = fixture(&scratch, "req.md", "# requirement\nbuild the shop\n");
    let acc = fixture(&scratch, "acc.md", "# acceptance\n");
    let note = fixture(&scratch, "note.md", "director: hold this\n");
    let report = fixture(&scratch, "report.md", "# delivery report\n");

    bl(
        &scratch,
        &state,
        &format!(
            "studio new --title acme-shop --repo {} --cast {}",
            repo.display(),
            cast.display()
        ),
    );
    bl(&scratch, &state, "stage start");
    wait_terminal(&client, "bl-acme-shop-producer").await;
    wait_terminal(&client, "bl-acme-shop-director").await;

    bl(
        &scratch,
        &state,
        &format!(
            "req revise --file {} --summary v1 --as producer",
            req.display()
        ),
    );
    for (task, actor) in [("t1", "actor-a"), ("t2", "actor-b"), ("t3", "actor-a")] {
        bl(
            &scratch,
            &state,
            &format!(
                "task new --title work-{task} --kind feature --acceptance-file {} --as director",
                acc.display()
            ),
        );
        bl(
            &scratch,
            &state,
            &format!("cast --task {task} --actor {actor} --reason cast --as director"),
        );
    }
    wait_terminal(&client, "bl-acme-shop-t1").await;
    wait_terminal(&client, "bl-acme-shop-t2").await;
    wait_terminal(&client, "bl-acme-shop-t3").await;

    // t3's actor closes deliberately before the restart: not wanted,
    // so recovery must leave it closed.
    bl(
        &scratch,
        &state,
        "task abandon --task t3 --reason done --as director",
    );
    wait_absent(&client, "bl-acme-shop-t3").await;
    wait_event(&state, "role.closed", "actor-t3-c1");

    // t1's actor holds an open permission attention when the core dies.
    let t1 = wait_terminal(&client, "bl-acme-shop-t1").await;
    send_text(&client, &t1.address, "work:blocked").await;
    let raised = wait_event(&state, "attention.raised", "actor-t1-c1");
    assert_eq!(raised["reason"], "permission prompt", "{raised}");

    // The core stops — every terminal, including the stage's, is gone.
    // A `tell` lands on the ledger while the world is down; the fresh
    // stage owes t2's actor that message after recovery.
    scratch.shutdown();
    bl(
        &scratch,
        &state,
        &format!("tell actor-t2-c1 --file {} --as director", note.display()),
    );
    drop(client);
    scratch.respawn();
    scratch.ready().await;
    let client = connect(&scratch).await;
    bl(&scratch, &state, "stage start");

    // Every wanted role: lost, then resumed `recovered` on its recorded
    // session, exactly one live terminal each.
    let wanted = [
        ("producer", "bl-acme-shop-producer"),
        ("director", "bl-acme-shop-director"),
        ("actor-t1-c1", "bl-acme-shop-t1"),
        ("actor-t2-c1", "bl-acme-shop-t2"),
    ];
    for (role, _) in wanted {
        wait_event(&state, "role.lost", role);
        wait_event(&state, "role.resumed", role);
    }
    let events = ledger(&state);
    let find = |kind: &str, role: &str| -> &serde_json::Value {
        events
            .iter()
            .find(|e| e["kind"] == kind && e["role"] == role)
            .unwrap_or_else(|| panic!("no {kind} for {role}: {events:?}"))
    };
    for (role, name) in wanted {
        let (started, lost, resumed) = (
            find("role.started", role),
            find("role.lost", role),
            find("role.resumed", role),
        );
        assert!(
            lost["seq"].as_u64().unwrap() > started["seq"].as_u64().unwrap()
                && resumed["seq"].as_u64().unwrap() > lost["seq"].as_u64().unwrap(),
            "{role}: started/lost/resumed out of order: {events:?}"
        );
        assert_eq!(resumed["reason"], "recovered", "{resumed}");
        assert_eq!(
            resumed["session_id"], started["session_id"],
            "{role} resumed on a different session: {events:?}"
        );
        let t = wait_terminal(&client, name).await;
        assert_eq!(
            resumed["terminal"].as_str(),
            Some(t.address.as_str()),
            "{resumed}"
        );
    }
    let live = client
        .terminal_list(ListParams::default())
        .await
        .expect("terminal.list")
        .terminals;
    for (_, name) in wanted {
        assert_eq!(
            live.iter()
                .filter(|t| t.name.as_deref() == Some(name))
                .count(),
            1,
            "{name} does not have exactly one live terminal"
        );
    }

    // The stale permission attention clears `role lost`; the closed
    // actor stays closed — never lost, never resumed, no terminal.
    let cleared = events
        .iter()
        .find(|e| e["kind"] == "attention.cleared" && e["target"] == raised["seq"])
        .unwrap_or_else(|| panic!("no attention.cleared for the prompt: {events:?}"));
    assert_eq!(cleared["reason"], "role lost", "{cleared}");
    assert!(
        !events.iter().any(|e| {
            e["role"] == "actor-t3-c1" && (e["kind"] == "role.lost" || e["kind"] == "role.resumed")
        }),
        "the closed actor was lost or resumed: {events:?}"
    );
    assert!(
        terminal_named(&client, "bl-acme-shop-t3").await.is_none(),
        "the closed actor's terminal came back"
    );

    // The recovered notices reach each role; the queued tell and a new
    // delivery land through the normal gate.
    for inbox in [
        "fake-inbox-producer",
        "fake-inbox-director",
        "fake-inbox-actor-t1",
        "fake-inbox-actor-t2",
    ] {
        wait_inbox(&state, inbox, "-recovered.md");
    }
    wait_inbox(&state, "fake-inbox-actor-t2", "-note.md");
    let work = state.join("studios/acme-shop/work/t1");
    let a = commit_in(&work, "a");
    let b = commit_in(&work, "b");
    bl(
        &scratch,
        &state,
        &format!(
            "deliver --commits {a}..{b} --report {} --check build=pass --as actor-t1-c1 --task t1",
            report.display()
        ),
    );
    wait_inbox(&state, "fake-inbox-director", "-delivered.md");

    drop(client);
    scratch.finish().expect("scratch teardown");
}

/// The ledger gains a `task.checked`: it records by `stage` under
/// `by`, not under `role`, so `wait_event`'s role filter cannot
/// see it — assert the byline while waiting for the record.
fn wait_checked(state: &Path) -> serde_json::Value {
    let deadline = Instant::now() + DEADLINE;
    loop {
        if let Some(e) = ledger(state)
            .into_iter()
            .find(|e| e["kind"] == "task.checked")
        {
            assert_eq!(e["by"]["role"], "stage", "{e}");
            return e;
        }
        assert!(Instant::now() < deadline, "no task.checked in the ledger");
        std::thread::sleep(POLL);
    }
}

/// `p` exists within the deadline — for the work tree the stage's
/// work-area step makes out of a cast.
fn wait_path(p: &Path) {
    let deadline = Instant::now() + DEADLINE;
    while !p.exists() {
        assert!(Instant::now() < deadline, "{} never appeared", p.display());
        std::thread::sleep(POLL);
    }
}

/// `true` while a process whose full command line contains `needle`
/// lives — `pgrep -f` over the whole box.
fn proc_has(needle: &str) -> bool {
    Command::new("pgrep")
        .args(["-f", needle])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// `needle` shows in a process list within the deadline.
fn wait_proc(needle: &str) {
    let deadline = Instant::now() + DEADLINE;
    while !proc_has(needle) {
        assert!(Instant::now() < deadline, "no process matching {needle:?}");
        std::thread::sleep(POLL);
    }
}

/// The generated body file under `messages/` for a notice `label`.
fn message_file(studio: &Path, label: &str) -> String {
    let deadline = Instant::now() + DEADLINE;
    let suffix = format!("-{label}.md");
    loop {
        let found = fs::read_dir(studio.join("messages")).ok().and_then(|rd| {
            rd.filter_map(|e| e.ok())
                .find(|e| e.file_name().to_string_lossy().ends_with(&suffix))
                .map(|e| e.path())
        });
        if let Some(p) = found {
            return fs::read_to_string(p).unwrap_or_default();
        }
        assert!(
            Instant::now() < deadline,
            "no messages/*{suffix} under {}",
            studio.display()
        );
        std::thread::sleep(POLL);
    }
}

/// The S9b check fixture: a studio on a real repo, cast `top` keys
/// before the tables and `checks` stanzas appended — up through one
/// delivered-able task: requirement, `t1` cast on actor-a, the
/// actor's `work/t1` tree made. Answers the studio dir.
fn checks_studio(scratch: &ScratchCore, state: &Path, top: &str, checks: &str) -> PathBuf {
    let studio = state.join("studios/acme-shop");
    let repo = scratch.work.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let out = Command::new("git")
        .args(["init", "-b", "main"])
        .arg(&repo)
        .output()
        .expect("git init");
    assert!(out.status.success(), "git init failed");
    git(&repo, &["commit", "--allow-empty", "-m", "init"]);
    let cast = fixture(
        scratch,
        "cast.toml",
        &format!("{top}{CAST_TOML}\n{checks}\n"),
    );
    let req = fixture(scratch, "req.md", "# requirement\nbuild the shop\n");
    let acc = fixture(scratch, "acc.md", "# acceptance\n");
    bl(
        scratch,
        state,
        &format!(
            "studio new --title acme-shop --repo {} --cast {}",
            repo.display(),
            cast.display()
        ),
    );
    bl(scratch, state, "stage start");
    bl(
        scratch,
        state,
        &format!(
            "req revise --file {} --summary v1 --as producer",
            req.display()
        ),
    );
    bl(
        scratch,
        state,
        &format!(
            "task new --title cart --kind feature --acceptance-file {} --as director",
            acc.display()
        ),
    );
    bl(
        scratch,
        state,
        "cast --task t1 --actor actor-a --reason first --as director",
    );
    wait_path(&studio.join("work/t1"));
    studio
}

/// t1's actor delivers a real `a..b` range from its work tree,
/// with `extra` flags like `--check`.
fn deliver_t1(scratch: &ScratchCore, state: &Path, studio: &Path, extra: &str) {
    let report = fixture(scratch, "report.md", "# delivery report\n");
    let work = studio.join("work/t1");
    let (a, b) = (commit_in(&work, "a"), commit_in(&work, "b"));
    let extra = if extra.is_empty() {
        String::new()
    } else {
        format!(" {extra}")
    };
    bl(
        scratch,
        state,
        &format!(
            "deliver --commits {a}..{b} --report {}{extra} --as actor-t1-c1 --task t1",
            report.display()
        ),
    );
}

/// The S9b stage checks: a declared `exit 1` runs in the stage
/// itself — recorded `task.checked` fail beside the actor's own
/// `units=pass` — and both reach the reviewer's notice and the
/// task's `status` line.
#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn real_stage_check_fails() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let state = scratch.root.join("bl-state");
    let studio = checks_studio(
        &scratch,
        &state,
        "",
        "[[checks]]\nname = \"units\"\nrun = \"exit 1\"\ntimeout_s = 30",
    );
    wait_terminal(&client, "bl-acme-shop-director").await;
    deliver_t1(&scratch, &state, &studio, "--check units=pass");

    // The stage ran it: a `task.checked` fail from `stage`, and the
    // log it names sits under `checks/`.
    let checked = wait_checked(&state);
    assert_eq!(checked["task"], "t1", "{checked}");
    assert_eq!(checked["delivery"], 1, "{checked}");
    assert_eq!(checked["name"], "units", "{checked}");
    assert_eq!(checked["exit_code"], 1, "{checked}");
    assert_ne!(checked["timed_out"].as_bool(), Some(true), "{checked}");
    let log = studio.join(checked["output"].as_str().expect("output"));
    assert!(log.exists(), "no check log at {}", log.display());

    // The reviewer's notice waited for the record, then lists what
    // the stage ran apart from what the actor reported.
    wait_inbox(&state, "fake-inbox-director", "-delivered.md");
    let notice = message_file(&studio, "delivered");
    for want in ["Stage ran:", "units: fail", "actor reported: units=pass"] {
        assert!(notice.contains(want), "notice misses {want}: {notice}");
    }
    // `status` marks the stage's fail on the task line.
    let status = bl(&scratch, &state, "status");
    assert!(status.contains("units:fail"), "{status}");

    drop(client);
    scratch.finish().expect("scratch teardown");
}

/// The check's environment is the stage's stripped to the
/// passthrough list plus the declared `check_env` names: a proxy
/// and a `BACKLOT_*` name the stage itself carries never reach the
/// check's log — while the one declared name does.
#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn real_stage_check_env_is_stripped() {
    // The extras sit on the core, so every terminal — the stage
    // included — carries them in its own environment.
    let scratch = ScratchCore::spawn_with(vec![
        ("CHECK_MARK".into(), "yes".into()),
        ("HTTPS_PROXY".into(), "http://proxy.invalid:1".into()),
        ("BACKLOT_ROLE".into(), "x".into()),
    ])
    .await;
    let client = connect(&scratch).await;
    let state = scratch.root.join("bl-state");
    let studio = checks_studio(
        &scratch,
        &state,
        "check_env = [\"CHECK_MARK\"]\n",
        "[[checks]]\nname = \"envs\"\nrun = \"env\"\ntimeout_s = 30",
    );
    deliver_t1(&scratch, &state, &studio, "");

    let checked = wait_checked(&state);
    assert_eq!(checked["name"], "envs", "{checked}");
    assert_eq!(checked["exit_code"], 0, "{checked}");
    let log = fs::read_to_string(studio.join(checked["output"].as_str().expect("output")))
        .expect("check log");
    // The declared name crossed from the stage's own env — proof
    // the extras really were on the stage and only the declared
    // one passed.
    assert!(log.contains("CHECK_MARK=yes"), "{log}");
    assert!(log.contains("PATH="), "{log}");
    for banned in ["HTTPS_PROXY", "BACKLOT_ROLE", "BACKLOT_STATE_DIR"] {
        assert!(!log.contains(banned), "{banned} reached the check: {log}");
    }

    drop(client);
    scratch.finish().expect("scratch teardown");
}

/// A check past its `timeout_s` is recorded `timed_out` — and the
/// TERM/KILL escalation hits the whole process group, so its
/// `sleep` is gone too.
#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn real_stage_check_timeout_kills_the_process_group() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let state = scratch.root.join("bl-state");
    let studio = checks_studio(
        &scratch,
        &state,
        "",
        "[[checks]]\nname = \"slow\"\nrun = \"sleep 33\"\ntimeout_s = 1",
    );
    deliver_t1(&scratch, &state, &studio, "");

    let checked = wait_checked(&state);
    assert_eq!(checked["name"], "slow", "{checked}");
    assert_eq!(checked["timed_out"], true, "{checked}");
    // The escalation hit the group: no `sleep 33` survives.
    let deadline = Instant::now() + Duration::from_secs(10);
    while proc_has("sleep 33") {
        assert!(
            Instant::now() < deadline,
            "a `sleep 33` survived its check's timeout"
        );
        std::thread::sleep(POLL);
    }

    drop(client);
    scratch.finish().expect("scratch teardown");
}

/// A stage stopped mid-check records nothing for the run; the
/// fresh stage owes the check again and the rerun lands exactly
/// one `task.checked`.
#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn real_stage_check_reruns_after_a_stage_restart() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let state = scratch.root.join("bl-state");
    let studio = checks_studio(
        &scratch,
        &state,
        "",
        "[[checks]]\nname = \"slow\"\nrun = \"sleep 12\"\ntimeout_s = 60",
    );
    deliver_t1(&scratch, &state, &studio, "");

    // Mid-check: its sleep is live. Stop the stage alone — the
    // orphaned run's result dies with it.
    wait_proc("sleep 12");
    let stage = wait_terminal(&client, "backlot-stage").await;
    client
        .terminal_kill(TerminalParams {
            terminal: stage.address.clone(),
            caller: None,
        })
        .await
        .expect("terminal.kill backlot-stage");
    while terminal_named(&client, "backlot-stage").await.is_some() {
        tokio::time::sleep(POLL).await;
    }
    bl(&scratch, &state, "stage start");
    wait_terminal(&client, "backlot-stage").await;

    // The fresh stage owes the check; the rerun records once.
    let checked = wait_checked(&state);
    assert_eq!(checked["name"], "slow", "{checked}");
    assert_ne!(checked["timed_out"].as_bool(), Some(true), "{checked}");
    assert_eq!(checked["exit_code"], 0, "{checked}");
    let records = ledger(&state)
        .iter()
        .filter(|e| {
            e["kind"] == "task.checked"
                && e["task"] == "t1"
                && e["delivery"] == 1
                && e["name"] == "slow"
        })
        .count();
    assert_eq!(records, 1, "the check was recorded {records} times");

    drop(client);
    scratch.finish().expect("scratch teardown");
}

/// Killing only the `backlot-stage` terminal leaves the roles live on
/// the core: a new `stage start` adopts them — no `role.lost`, no
/// second terminal per role.
#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn real_stage_restart_stage_only() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;
    let state = scratch.root.join("bl-state");
    let repo = scratch.work.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let out = Command::new("git")
        .args(["init", "-b", "main"])
        .arg(&repo)
        .output()
        .expect("git init");
    assert!(out.status.success(), "git init failed");
    git(&repo, &["commit", "--allow-empty", "-m", "init"]);
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    let req = fixture(&scratch, "req.md", "# requirement\nbuild the shop\n");
    let acc = fixture(&scratch, "acc.md", "# acceptance\n");

    bl(
        &scratch,
        &state,
        &format!(
            "studio new --title acme-shop --repo {} --cast {}",
            repo.display(),
            cast.display()
        ),
    );
    bl(&scratch, &state, "stage start");
    let stage = wait_terminal(&client, "backlot-stage").await;
    let producer = wait_terminal(&client, "bl-acme-shop-producer").await;
    let director = wait_terminal(&client, "bl-acme-shop-director").await;
    bl(
        &scratch,
        &state,
        &format!(
            "req revise --file {} --summary v1 --as producer",
            req.display()
        ),
    );
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
    let actor = wait_terminal(&client, "bl-acme-shop-t1").await;

    // The stage terminal dies alone; a fresh stage start adopts the
    // live roles — nobody is lost and nobody is relaunched.
    client
        .terminal_kill(TerminalParams {
            terminal: stage.address.clone(),
            caller: None,
        })
        .await
        .expect("terminal.kill backlot-stage");
    while terminal_named(&client, "backlot-stage").await.is_some() {
        tokio::time::sleep(POLL).await;
    }
    bl(&scratch, &state, "stage start");
    let new_stage = wait_terminal(&client, "backlot-stage").await;
    assert_ne!(
        new_stage.address, stage.address,
        "the stage did not restart"
    );

    // A few ticks in, the ledger holds no loss and every role keeps
    // the terminal it had.
    tokio::time::sleep(Duration::from_secs(4)).await;
    let events = ledger(&state);
    assert!(
        !events
            .iter()
            .any(|e| e["kind"] == "role.lost" || e["kind"] == "role.resumed"),
        "a stage-only restart recorded lifecycle loss: {events:?}"
    );
    for (name, old) in [
        ("bl-acme-shop-producer", &producer.address),
        ("bl-acme-shop-director", &director.address),
        ("bl-acme-shop-t1", &actor.address),
    ] {
        let t = terminal_named(&client, name)
            .await
            .unwrap_or_else(|| panic!("{name} is gone"));
        assert_eq!(&t.address, old, "{name} was relaunched");
    }

    drop(client);
    scratch.finish().expect("scratch teardown");
}
