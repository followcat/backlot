//! Shared harness for the in-process CLI tests: a scratch state dir
//! and `run` wrappers that unwrap to output, refusal or usage error.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
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
        Err(Fail::Refused(m)) => panic!("refused on '{s}': {m}"),
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
