//! Shared harness for the in-process CLI tests: a scratch state dir
//! and `run` wrappers that unwrap to output, refusal or usage error.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::cli::{Env, Fail, run};

static NEXT: AtomicU64 = AtomicU64::new(0);

pub struct Scratch {
    pub dir: PathBuf,
    pub files: PathBuf,
}

pub fn scratch() -> (Scratch, Env) {
    let base = std::env::temp_dir().join(format!(
        "backlot-cli-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let files = base.join("files");
    fs::create_dir_all(&files).unwrap();
    let mut vars = BTreeMap::new();
    vars.insert(
        "BACKLOT_STATE_DIR".into(),
        base.to_string_lossy().into_owned(),
    );
    vars.insert("BACKLOT_STUDIO".into(), "acme-shop".into());
    (Scratch { dir: base, files }, Env { vars })
}

pub fn fixture(scratch: &Scratch, name: &str, content: &str) -> String {
    let p = scratch.files.join(name);
    fs::write(&p, content).unwrap();
    p.to_string_lossy().into_owned()
}

fn args(s: &str) -> Vec<String> {
    s.split(' ').map(String::from).collect()
}

pub fn ok(env: &Env, s: &str) -> String {
    match run(&args(s), env) {
        Ok(out) => out,
        Err(Fail::Usage(m)) => panic!("usage error on '{s}': {m}"),
        Err(Fail::Refused(m) | Fail::Core(m)) => panic!("refused on '{s}': {m}"),
    }
}

pub fn refused(env: &Env, s: &str) -> String {
    match run(&args(s), env) {
        Err(Fail::Refused(m)) => m,
        other => panic!("expected refusal on '{s}', got {other:?}"),
    }
}

pub fn usage(env: &Env, s: &str) -> String {
    match run(&args(s), env) {
        Err(Fail::Usage(m)) => m,
        other => panic!("expected usage error on '{s}', got {other:?}"),
    }
}

/// Run git in `dir`; answers trimmed stdout, panics on failure.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=alice", "-c", "user.email=alice@host-a"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A real `work/<task>` for `deliver` (S9b): a source repo under
/// `files/`, a worktree inside the studio dir on the task's own
/// `backlot/acme-shop/<task>` branch — the layout the stage's
/// work-area step produces.
pub fn work_area(scratch: &Scratch, task: &str) -> PathBuf {
    let repo = scratch.files.join(format!("repo-{task}"));
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    fs::write(repo.join("f"), "base\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "base"]);
    let work = crate::store::studio_dir(&scratch.dir, "acme-shop")
        .join("work")
        .join(task);
    fs::create_dir_all(work.parent().unwrap()).unwrap();
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            &format!("backlot/acme-shop/{task}"),
            work.to_str().unwrap(),
        ],
    );
    work
}

/// Commit `text` as `f` inside `work` and answer its full sha.
pub fn commit_in(work: &Path, text: &str) -> String {
    fs::write(work.join("f"), format!("{text}\n")).unwrap();
    git(work, &["add", "."]);
    git(work, &["commit", "-m", text]);
    git(work, &["rev-parse", "HEAD"])
}

/// One source repo under `files/repo` shared by the studio's work
/// areas (S9e): `work/integration` and `work/<task>` sit on branches
/// of the same object store, so a dependency's delivery can really
/// merge into `backlot/acme-shop/integration`.
pub fn shared_repo(scratch: &Scratch) -> PathBuf {
    let repo = scratch.files.join("repo");
    if repo.is_dir() {
        return repo;
    }
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    fs::write(repo.join("f"), "base\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "base"]);
    repo
}

/// `work/<name>` inside the studio dir on branch
/// `backlot/acme-shop/<name>` off the shared repo — `t1` for a task,
/// `integration` for the integration area.
pub fn shared_area(scratch: &Scratch, name: &str) -> PathBuf {
    let repo = shared_repo(scratch);
    let work = crate::store::studio_dir(&scratch.dir, "acme-shop")
        .join("work")
        .join(name);
    fs::create_dir_all(work.parent().unwrap()).unwrap();
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            &format!("backlot/acme-shop/{name}"),
            work.to_str().unwrap(),
        ],
    );
    work
}

pub const CAST_TOML: &str = r#"
mode = "standard"
rework_cap = 2

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

[[candidates]]
name = "actor-b"
runner = "runner-b"
model = "model-b"
effort = "low"
permission_profile = "worktree"
"#;
