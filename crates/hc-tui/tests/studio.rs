//! Binary-level tests for the studio commands and the dispatch hook in
//! `main`: the compiled `backlot` binary under a scratch state dir.
//! Nothing here touches a real hc session — the same isolation as
//! `tests/cli.rs` applies.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SCRATCH_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Scratch HOME/XDG/BACKLOT_STATE_DIR for one test.
struct Scratch {
    root: PathBuf,
    state: PathBuf,
}

impl Scratch {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let unique = format!(
            "backlot-studio-test-{}-{}-{nanos}",
            std::process::id(),
            SCRATCH_COUNTER.fetch_add(1, Ordering::Relaxed),
        );
        let root = std::env::temp_dir().join(unique);
        let state = root.join("backlot-state");
        for dir in ["home", "files"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        fs::create_dir_all(&state).unwrap();
        Self { root, state }
    }

    fn fixture(&self, name: &str, content: &str) -> String {
        let p = self.root.join("files").join(name);
        fs::write(&p, content).unwrap();
        p.to_string_lossy().into_owned()
    }

    fn events(&self) -> PathBuf {
        self.state.join("studios/acme-shop/events.jsonl")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn command(scratch: &Scratch, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_backlot"));
    cmd.args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", scratch.root.join("home"))
        .env("BACKLOT_STATE_DIR", &scratch.state)
        .env("BACKLOT_STUDIO", "acme-shop")
        .env("HC_SOCKET", scratch.root.join("missing.sock"))
        .env("TERM", "xterm-256color")
        .current_dir(&scratch.root);
    cmd
}

fn run(scratch: &Scratch, args: &[&str]) -> Output {
    command(scratch, args).output().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn stderr(o: &Output) -> String {
    String::from_utf8(o.stderr.clone()).unwrap()
}

fn ok(scratch: &Scratch, args: &[&str]) -> String {
    let o = run(scratch, args);
    assert!(
        o.status.success(),
        "{args:?} exited {:?}: {}",
        o.status.code(),
        stderr(&o)
    );
    stdout(&o)
}

const CAST_TOML: &str = r#"
[producer]
runner = "runner-p"
model = "model-p"
effort = "high"
permission_profile = "standard"

[director]
runner = "runner-d"
model = "model-d"
effort = "high"
permission_profile = "standard"

[writer]
runner = "runner-w"
model = "model-w"
effort = "medium"
permission_profile = "standard"

[[candidates]]
name = "actor-a"
runner = "runner-a"
model = "model-a"
effort = "high"
permission_profile = "worktree"
"#;

/// The spec's scripted studio on the compiled binary: new → req →
/// plan → task → cast → deliver → changes → deliver → accepted, then
/// `status` and `records`.
#[test]
fn scripted_studio_loop() {
    let s = Scratch::new();
    let cast = s.fixture("cast.toml", CAST_TOML);
    let o = ok(
        &s,
        &[
            "studio",
            "new",
            "--title",
            "Shop",
            "--repo",
            "/home/user/acme-shop",
            "--cast",
            &cast,
        ],
    );
    assert!(o.contains("studio.created"), "{o}");

    let req = s.fixture("req.md", "# requirement\n");
    ok(
        &s,
        &[
            "req",
            "revise",
            "--file",
            &req,
            "--summary",
            "first",
            "--as",
            "producer",
        ],
    );
    ok(&s, &["plan", "request", "--as", "producer"]);
    let plan = s.fixture("plan.md", "# plan\n");
    ok(
        &s,
        &[
            "plan", "publish", "--kind", "plan", "--file", &plan, "--as", "writer",
        ],
    );
    let acc = s.fixture("acc.md", "# acceptance\n");
    ok(
        &s,
        &[
            "task",
            "new",
            "--title",
            "cart",
            "--kind",
            "feature",
            "--acceptance-file",
            &acc,
            "--as",
            "director",
        ],
    );
    ok(
        &s,
        &[
            "cast", "--task", "t1", "--actor", "actor-a", "--reason", "r", "--as", "director",
        ],
    );

    let r1 = s.fixture("r1.md", "report one\n");
    let o = ok(
        &s,
        &[
            "deliver",
            "--commits",
            "a..b",
            "--report",
            &r1,
            "--check",
            "tests=pass",
            "--task",
            "t1",
            "--as",
            "actor",
        ],
    );
    assert!(o.contains("task.delivered t1 d1"), "{o}");

    let f = s.fixture(
        "f.json",
        r#"[{"id":"D1","summary":"misses tax","severity":"medium"}]"#,
    );
    ok(
        &s,
        &[
            "review",
            "--task",
            "t1",
            "--delivery",
            "1",
            "--verdict",
            "changes-requested",
            "--conclusion",
            "close",
            "--findings",
            &f,
            "--as",
            "director",
        ],
    );
    let r2 = s.fixture("r2.md", "report two\n");
    ok(
        &s,
        &[
            "deliver",
            "--commits",
            "b..c",
            "--report",
            &r2,
            "--task",
            "t1",
            "--as",
            "actor",
        ],
    );
    ok(
        &s,
        &[
            "review",
            "--task",
            "t1",
            "--delivery",
            "2",
            "--verdict",
            "accepted",
            "--conclusion",
            "done",
            "--as",
            "director",
        ],
    );

    let status = ok(&s, &["status"]);
    assert!(status.contains("acme-shop"), "{status}");
    assert!(status.contains("t1"), "{status}");
    assert!(status.contains("accepted"), "{status}");

    let status_json = ok(&s, &["status", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&status_json).unwrap();
    assert_eq!(v["tasks"]["t1"]["status"], "accepted");
    assert_eq!(v["tasks"]["t1"]["rework_rounds"], 1);

    let records = ok(&s, &["records"]);
    assert!(records.contains("not enough data"), "{records}");
    let records_json = ok(&s, &["records", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&records_json).unwrap();
    assert_eq!(v[0]["tasks"], 1);
    assert_eq!(v[0]["first_delivery"]["changes_requested"], 1);

    // Exit codes: a refused command is 1, a usage error is 2.
    let o = run(
        &s,
        &[
            "review",
            "--task",
            "t1",
            "--delivery",
            "3",
            "--verdict",
            "accepted",
            "--conclusion",
            "x",
            "--as",
            "director",
        ],
    );
    assert_eq!(o.status.code(), Some(1), "{}", stdout(&o));
    assert!(!stderr(&o).is_empty());
    let o = run(
        &s,
        &[
            "deliver",
            "--commits",
            "x..y",
            "--task",
            "t1",
            "--as",
            "actor",
        ],
    );
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("missing --report"), "{}", stderr(&o));

    // Events land in one ordered JSONL log.
    let log = fs::read_to_string(s.events()).unwrap();
    let seqs: Vec<u64> = log
        .lines()
        .map(|l| {
            serde_json::from_str::<serde_json::Value>(l).unwrap()["seq"]
                .as_u64()
                .unwrap()
        })
        .collect();
    assert_eq!(seqs.len(), 10, "{seqs:?}");
    assert!(seqs.windows(2).all(|w| w[1] == w[0] + 1), "{seqs:?}");
}

/// Eight processes racing `plan request` keep one total order under
/// the flock: 200 events, unique increasing seqs, nothing lost.
#[test]
fn concurrent_appends_across_processes() {
    let s = Scratch::new();
    let cast = s.fixture("cast.toml", CAST_TOML);
    ok(
        &s,
        &[
            "studio",
            "new",
            "--title",
            "Shop",
            "--repo",
            "/home/user/acme-shop",
            "--cast",
            &cast,
        ],
    );

    let procs = 8;
    let per = 25;
    let mut children = Vec::new();
    for _ in 0..procs {
        children.push(std::thread::spawn({
            let root = s.root.clone();
            let state = s.state.clone();
            move || {
                for _ in 0..per {
                    let o = Command::new(env!("CARGO_BIN_EXE_backlot"))
                        .args(["plan", "request", "--as", "producer"])
                        .env_clear()
                        .env("PATH", "/usr/bin:/bin")
                        .env("HOME", root.join("home"))
                        .env("BACKLOT_STATE_DIR", &state)
                        .env("BACKLOT_STUDIO", "acme-shop")
                        .current_dir(&root)
                        .output()
                        .unwrap();
                    assert!(
                        o.status.success(),
                        "{}",
                        String::from_utf8(o.stderr).unwrap()
                    );
                }
            }
        }));
    }
    for c in children {
        c.join().unwrap();
    }

    let log = fs::read_to_string(s.events()).unwrap();
    let mut seqs: Vec<u64> = log
        .lines()
        .map(|l| {
            serde_json::from_str::<serde_json::Value>(l).unwrap()["seq"]
                .as_u64()
                .unwrap()
        })
        .collect();
    assert_eq!(seqs.len(), 1 + procs * per);
    // In-file order is already the total order.
    assert!(seqs.windows(2).all(|w| w[1] == w[0] + 1), "{seqs:?}");
    seqs.sort_unstable();
    seqs.dedup();
    let total = (1 + procs * per) as u64;
    assert_eq!(seqs, (1..=total).collect::<Vec<_>>());
}

/// Studio names are `[a-z0-9_-]{1,16}`: bad names exit 2 before any
/// path is built, and an append on a missing studio exits 1 without
/// creating anything — `../../escape` can never leave the state dir.
#[test]
fn studio_names_are_checked_before_paths() {
    let s = Scratch::new();
    for bad in ["../x", "../../escape", "Upper", "seventeen_charname"] {
        let o = run(&s, &["status", "--studio", bad]);
        assert_eq!(o.status.code(), Some(2), "{bad}: {}", stderr(&o));
        let o = run(
            &s,
            &["plan", "request", "--studio", bad, "--as", "producer"],
        );
        assert_eq!(o.status.code(), Some(2), "{bad}: {}", stderr(&o));
    }
    let studios = s.state.join("studios");
    assert!(!studios.exists() || fs::read_dir(studios).unwrap().next().is_none());
    assert!(!s.root.join("escape").exists());

    let o = run(
        &s,
        &["plan", "request", "--studio", "nosuch", "--as", "producer"],
    );
    assert_eq!(o.status.code(), Some(1), "{}", stdout(&o));
    assert!(stderr(&o).contains("no studio 'nosuch'"), "{}", stderr(&o));
    assert!(!s.state.join("studios/nosuch").exists());
}

/// The hook leaves TUI arguments exactly as before: `--version`,
/// `--session x` (fails at the terminal, not in studio parsing), and
/// an unknown word (exit 2 from the TUI parser).
#[test]
fn hook_leaves_tui_args_alone() {
    let s = Scratch::new();
    let o = run(&s, &["--version"]);
    assert!(o.status.success());
    assert_eq!(
        stdout(&o).trim(),
        format!("backlot {}", env!("CARGO_PKG_VERSION"))
    );

    let o = run(&s, &["--session", "x"]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("terminal"), "{}", stderr(&o));

    let o = run(&s, &["frobnicate"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("unexpected argument 'frobnicate'"));

    // A studio word routes to the stage: `backlot studio` with no
    // subcommand is a usage error from the studio parser.
    let o = run(&s, &["studio"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("studio"), "{}", stderr(&o));
}
