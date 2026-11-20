//! Shared harness for the opt-in real-core stage tests: the `backlot`
//! CLI under a scratch env, terminal waits, ledger reads and the
//! fixture helpers every real-core test drives. Studio-bound helpers
//! take `studio` — the pinned `BACKLOT_STUDIO` for `bl`, the ledger
//! directory for `ledger`, the doc block for `wait_role`/`wait_studio`
//! — so one file can drive several studios without duplicating them.
//!
//! Included with `#[path = "common/real_kit.rs"] mod kit;` — every
//! helper is `pub` and unused ones are simply dead code in a file
//! that does not call them.

#![allow(dead_code)]

use hc_proto::client::Client;
use hc_proto::doc::DocGetParams;
use hc_proto::endpoint::Endpoint;
use hc_proto::terminal::{ListParams, ReadParams, Terminal};

use serde_json::Value;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::scratch::ScratchCore;

/// Finite bound on any single real-core step.
pub const DEADLINE: Duration = Duration::from_secs(30);
pub const POLL: Duration = Duration::from_millis(150);

/// The `backlot` binary under test — cargo exports it for this target.
pub fn tui_bin() -> &'static str {
    env!("CARGO_BIN_EXE_backlot")
}

/// The scratch env every `backlot` invocation sees — session, socket,
/// state dir, nothing inherited — without a `BACKLOT_STUDIO` pin.
pub fn env(scratch: &ScratchCore, state: &Path) -> Vec<(String, String)> {
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
    ]
}

/// The scratch env with `BACKLOT_STUDIO` pinned — a command without an
/// explicit `--studio` lands on this studio, and an explicit different
/// one is refused.
pub fn bl_env(scratch: &ScratchCore, state: &Path, studio: &str) -> Vec<(String, String)> {
    let mut env = env(scratch, state);
    env.push(("BACKLOT_STUDIO".into(), studio.into()));
    env
}

/// `backlot <args>` under the given env, returned raw — `bl` and
/// `bl_fail` assert on the exit from here.
pub fn bl_run(env: Vec<(String, String)>, args: &str) -> std::process::Output {
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

/// `backlot <args>` under an explicit env; assert exit 0 and return
/// stdout. The cursor test needs an env of its own — no
/// `XDG_CONFIG_HOME`, the stub first on `PATH`.
pub fn bl_with(env: Vec<(String, String)>, args: &str) -> String {
    let out = bl_run(env, args);
    assert!(
        out.status.success(),
        "backlot {args} exited {}; stderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// `backlot <args>` pinned to `studio`; assert exit 0 and return stdout.
pub fn bl(scratch: &ScratchCore, state: &Path, studio: &str, args: &str) -> String {
    bl_with(bl_env(scratch, state, studio), args)
}

/// `backlot <args>` with no `BACKLOT_STUDIO` pin — naming another
/// studio (`studio new --studio`, `tell --studio`) needs one.
pub fn bl_unpinned(scratch: &ScratchCore, state: &Path, args: &str) -> String {
    bl_with(env(scratch, state), args)
}

/// `backlot <args>` pinned to `studio`, expected to refuse; assert a
/// non-zero exit and return stderr.
pub fn bl_fail(scratch: &ScratchCore, state: &Path, studio: &str, args: &str) -> String {
    let out = bl_run(bl_env(scratch, state, studio), args);
    assert!(
        !out.status.success(),
        "backlot {args} should refuse, got: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    String::from_utf8_lossy(&out.stderr).into_owned()
}

pub async fn connect(scratch: &ScratchCore) -> Client {
    Client::connect(&scratch.endpoint, None::<&std::ffi::OsStr>)
        .await
        .expect("connect to scratch core")
}

/// Every live terminal the core lists — the two-studio assertions
/// sweep it for `bl-` terminals and their `backlot.studio` labels.
pub async fn live_terminals(client: &Client) -> Vec<Terminal> {
    client
        .terminal_list(ListParams::default())
        .await
        .expect("terminal.list")
        .terminals
}

/// The live terminal with `name`, when the core shows one.
pub async fn terminal_named(client: &Client, name: &str) -> Option<Terminal> {
    client
        .terminal_list(ListParams::default())
        .await
        .ok()?
        .terminals
        .into_iter()
        .find(|t| t.name.as_deref() == Some(name))
}

pub async fn wait_terminal(client: &Client, name: &str) -> Terminal {
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
pub async fn wait_replaced(client: &Client, name: &str, old: &str) -> Terminal {
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
pub async fn wait_absent(client: &Client, name: &str) {
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
pub async fn wait_read(client: &Client, terminal: &str, needle: &str) -> String {
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
pub async fn send_text(client: &Client, terminal: &str, text: &str) {
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
pub async fn send_keys(client: &Client, terminal: &str, keys: &[&str]) {
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
pub async fn wait_count(client: &Client, terminal: &str, marker: &str, n: usize) -> String {
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

/// The `backlot` document's block for `studio` once `want` holds for
/// it — the per-studio view of the shared document.
pub async fn wait_studio(client: &Client, studio: &str, want: fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + DEADLINE;
    loop {
        let (_, doc) = wait_doc(client).await;
        let found = doc["studios"]
            .as_array()
            .and_then(|ss| ss.iter().find(|s| s["id"] == studio));
        if let Some(s) = found.filter(|s| want(s)) {
            return s.clone();
        }
        assert!(
            Instant::now() < deadline,
            "{studio} never matched in the doc: {doc}"
        );
        tokio::time::sleep(POLL).await;
    }
}

/// The studio's role entry in the document once `want` holds for it.
pub async fn wait_role(
    client: &Client,
    studio: &str,
    role: &str,
    want: fn(&Value) -> bool,
) -> Value {
    let deadline = Instant::now() + DEADLINE;
    loop {
        let s = wait_studio(client, studio, |_| true).await;
        let found = s["roles"]
            .as_array()
            .and_then(|rs| rs.iter().find(|r| r["role"] == role));
        if let Some(r) = found.filter(|r| want(r)) {
            return r.clone();
        }
        assert!(
            Instant::now() < deadline,
            "{role} never matched in the doc: {s}"
        );
        tokio::time::sleep(POLL).await;
    }
}

/// The `backlot` document's version and value once it exists.
pub async fn wait_doc(client: &Client) -> (u64, Value) {
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

pub async fn doc_version(client: &Client) -> u64 {
    client
        .doc_get(DocGetParams {
            name: "backlot".into(),
            on: None,
        })
        .await
        .expect("doc.get")
        .version
}

/// The studio's state directory under the shared state dir.
pub fn studio_dir(state: &Path, studio: &str) -> PathBuf {
    state.join("studios").join(studio)
}

/// Every ledger line for `studio`, parsed. A missing file reads as
/// an empty log — the store's own rule — so the waits poll the
/// ledger appearing instead of racing its first append.
pub fn ledger(state: &Path, studio: &str) -> Vec<Value> {
    let path = studio_dir(state, studio).join("events.jsonl");
    match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => panic!("{}: {e}", path.display()),
    }
    .lines()
    .filter(|l| !l.is_empty())
    .map(|l| serde_json::from_str(l).expect("ledger line"))
    .collect()
}

/// The studio's ledger gains an event of `kind` on `role` within the
/// deadline.
pub fn wait_event(state: &Path, studio: &str, kind: &str, role: &str) -> Value {
    let deadline = Instant::now() + DEADLINE;
    loop {
        if let Some(e) = ledger(state, studio)
            .into_iter()
            .find(|e| e["kind"] == kind && e["role"] == role)
        {
            return e;
        }
        assert!(
            Instant::now() < deadline,
            "no {kind} for {role} in the {studio} ledger"
        );
        std::thread::sleep(POLL);
    }
}

/// The studio's ledger reaches `n` events of `kind` on `role` — the
/// count catches a repeated event the single-event wait cannot see.
pub fn wait_events(state: &Path, studio: &str, kind: &str, role: &str, n: usize) {
    let deadline = Instant::now() + DEADLINE;
    loop {
        let count = ledger(state, studio)
            .iter()
            .filter(|e| e["kind"] == kind && e["role"] == role)
            .count();
        if count >= n {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "wanted {n} {kind} for {role} in the {studio} ledger"
        );
        std::thread::sleep(POLL);
    }
}

/// The studio's ledger reaches `n` events of `kind` regardless of
/// role — for kinds like `attention.cleared` that carry no `role`.
pub fn wait_kind(state: &Path, studio: &str, kind: &str, n: usize) {
    let deadline = Instant::now() + DEADLINE;
    loop {
        let count = ledger(state, studio)
            .iter()
            .filter(|e| e["kind"] == kind)
            .count();
        if count >= n {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "wanted {n} {kind} events in the {studio} ledger"
        );
        std::thread::sleep(POLL);
    }
}

/// The studio's ledger gains a `task.checked`: it records by `stage`
/// under `by`, not under `role`, so `wait_event`'s role filter cannot
/// see it — assert the byline while waiting for the record.
pub fn wait_checked(state: &Path, studio: &str) -> Value {
    let deadline = Instant::now() + DEADLINE;
    loop {
        if let Some(e) = ledger(state, studio)
            .into_iter()
            .find(|e| e["kind"] == "task.checked")
        {
            assert_eq!(e["by"]["role"], "stage", "{e}");
            return e;
        }
        assert!(
            Instant::now() < deadline,
            "no task.checked in the {studio} ledger"
        );
        std::thread::sleep(POLL);
    }
}

/// The all-fake cast every real-core test drives — the producer is a
/// `trust` model so the stage's dialog exchange is covered too.
pub const CAST_TOML: &str = r#"
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

pub fn fixture(scratch: &ScratchCore, name: &str, content: &str) -> PathBuf {
    let path = scratch.work.join(name);
    fs::write(&path, content).unwrap();
    path
}

/// Assert `t` carries the role labels design.md requires.
pub fn check_labels(t: &Terminal, studio: &str, role: &str, task: Option<&str>) {
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
        Some(studio),
        "{t:?}"
    );
    assert_eq!(l.get("herdr.seen").map(String::as_str), Some("0"), "{t:?}");
    assert_eq!(l.get("backlot.task").map(String::as_str), task, "{t:?}");
}

/// `git` in the fixture repo — the work-area assertions run real
/// git against it; commits get a fixture identity through the env.
pub fn git(repo: &Path, args: &[&str]) -> String {
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

/// `git init -b main` plus the empty `init` commit — each studio's
/// repository is its own scratch work tree.
pub fn git_repo(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    let out = Command::new("git")
        .args(["init", "-b", "main"])
        .arg(dir)
        .output()
        .expect("git init");
    assert!(out.status.success(), "git init failed");
    git(dir, &["commit", "--allow-empty", "-m", "init"]);
}

/// An empty commit on the current branch of `dir` — a work area of
/// the fixture repo — answering the new HEAD sha.
pub fn commit_in(dir: &Path, msg: &str) -> String {
    git(dir, &["commit", "--allow-empty", "-m", msg]);
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

/// The fake runner's studio-keyed inbox file: every line it is sent
/// lands in `fake-inbox-<studio>-<role>` — and in the legacy
/// `fake-inbox-<role>` for single-studio tests — under the state dir.
pub fn inbox_name(studio: &str, role: &str) -> String {
    format!("fake-inbox-{studio}-{role}")
}

/// Wait until `needle` shows in the state-dir file `name` — the fake
/// runner's inbox for a role.
pub fn wait_inbox(state: &Path, name: &str, needle: &str) -> String {
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

/// `p` exists within the deadline — for the work tree the stage's
/// work-area step makes out of a cast.
pub fn wait_path(p: &Path) {
    let deadline = Instant::now() + DEADLINE;
    while !p.exists() {
        assert!(Instant::now() < deadline, "{} never appeared", p.display());
        std::thread::sleep(POLL);
    }
}

/// `true` while a process whose full command line contains `needle`
/// lives — `pgrep -f` over the whole box.
pub fn proc_has(needle: &str) -> bool {
    Command::new("pgrep")
        .args(["-f", needle])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// `needle` shows in a process list within the deadline.
pub fn wait_proc(needle: &str) {
    let deadline = Instant::now() + DEADLINE;
    while !proc_has(needle) {
        assert!(Instant::now() < deadline, "no process matching {needle:?}");
        std::thread::sleep(POLL);
    }
}

/// The generated body file under `messages/` for a notice `label`.
pub fn message_file(studio: &Path, label: &str) -> String {
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
pub fn checks_studio(
    scratch: &ScratchCore,
    state: &Path,
    studio: &str,
    top: &str,
    checks: &str,
) -> PathBuf {
    let dir = studio_dir(state, studio);
    let repo = scratch.work.join("repo");
    git_repo(&repo);
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
        studio,
        &format!(
            "studio new --title {studio} --repo {} --cast {}",
            repo.display(),
            cast.display()
        ),
    );
    bl(scratch, state, studio, "stage start");
    bl(
        scratch,
        state,
        studio,
        &format!(
            "req revise --file {} --summary v1 --as producer",
            req.display()
        ),
    );
    bl(
        scratch,
        state,
        studio,
        &format!(
            "task new --title cart --kind feature --acceptance-file {} --as director",
            acc.display()
        ),
    );
    bl(
        scratch,
        state,
        studio,
        "cast --task t1 --actor actor-a --reason first --as director",
    );
    wait_path(&dir.join("work/t1"));
    dir
}

/// t1's actor delivers a real `a..b` range from its work tree,
/// with `extra` flags like `--check`.
pub fn deliver_t1(scratch: &ScratchCore, state: &Path, studio: &str, dir: &Path, extra: &str) {
    let report = fixture(scratch, "report.md", "# delivery report\n");
    let work = dir.join("work/t1");
    let (a, b) = (commit_in(&work, "a"), commit_in(&work, "b"));
    let extra = if extra.is_empty() {
        String::new()
    } else {
        format!(" {extra}")
    };
    bl(
        scratch,
        state,
        studio,
        &format!(
            "deliver --commits {a}..{b} --report {}{extra} --as actor-t1-c1 --task t1",
            report.display()
        ),
    );
}

/// The H2 ownership invariant on the whole inventory: every live
/// `bl-`-named terminal carries a `backlot.studio` label naming one
/// of `studios`, and its name begins `bl-<that studio>-`. The name
/// alone cannot decide `acme` from `acme-shop` — prefix-sharing
/// studio names are the worst case — so the label is checked first,
/// the name's studio prefix second, and the exact set of expected
/// names `want` last: nothing unlabelled, nothing borrowed, nothing
/// extra or missing.
pub async fn assert_terminal_partition(client: &Client, studios: &[&str], want: &[&str]) {
    let mut got: Vec<String> = Vec::new();
    for t in live_terminals(client).await {
        let Some(name) = t.name.as_deref() else {
            continue;
        };
        if !name.starts_with("bl-") {
            continue;
        }
        got.push(name.into());
        let owner = t
            .labels
            .get("backlot.studio")
            .map(String::as_str)
            .unwrap_or_else(|| panic!("{name} carries no backlot.studio label: {t:?}"));
        assert!(
            studios.contains(&owner),
            "{name} labels an unknown studio {owner:?}: {t:?}"
        );
        assert!(
            name.starts_with(&format!("bl-{owner}-")),
            "{name}'s label {owner} does not match its own name"
        );
    }
    got.sort();
    let mut want: Vec<String> = want.iter().map(|s| s.to_string()).collect();
    want.sort();
    assert_eq!(got, want, "the live bl- terminals are not the expected set");
}

/// Kill the `backlot-stage` terminal and wait until it is gone —
/// the stage-only restart: roles keep running, a fresh `stage start`
/// must adopt them.
pub async fn kill_stage(client: &Client) {
    let stage = wait_terminal(client, "backlot-stage").await;
    client
        .terminal_kill(hc_proto::terminal::TerminalParams {
            terminal: stage.address.clone(),
            caller: None,
        })
        .await
        .expect("terminal.kill backlot-stage");
    // `terminal.kill` drops it from the live list but leaves the
    // finished record — `wait_absent`'s all-inclusive list would
    // see it forever, so watch the live list here.
    let deadline = Instant::now() + DEADLINE;
    while terminal_named(client, "backlot-stage").await.is_some() {
        assert!(Instant::now() < deadline, "backlot-stage never died");
        tokio::time::sleep(POLL).await;
    }
}
