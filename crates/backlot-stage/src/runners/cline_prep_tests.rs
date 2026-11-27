//! cline's per-launch prep: the role's command rules, the hook file
//! the work area gets, the role's MCP mask and signals file, and the
//! worktree's `info/exclude`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use backlot_model::event::CheckDecl;

use super::cline::{ClinePrep, allow_commands, prepare};
use super::cline_tests::{MODEL, ctx, ident};
use super::{Prep, plan};
use crate::test_support::TestDir;

fn check(name: &str, run: &str) -> CheckDecl {
    CheckDecl::new(name, run, 600)
}

/// The commands per role: `backlot` for everyone plus the shared
/// read-only set; an actor's `git add`/`git commit`, the director's
/// `git merge`; each declared check verbatim for both — and a rule a
/// single-quoted shell string cannot hold is dropped, never mangled.
/// `git branch` is only ever its two listing forms.
#[test]
fn allowed_commands_per_role() {
    let checks = [check("unit", "cargo test --lib")];
    let read_only = [
        "git status",
        "git log",
        "git show",
        "git diff",
        "git branch --list",
        "git branch --show-current",
        "ls",
        "rg",
        "wc",
        "head",
        "backlot msg show",
    ];
    let producer: Vec<String> = std::iter::once("backlot".to_string())
        .chain(read_only.iter().map(|r| r.to_string()))
        .collect();
    assert_eq!(allow_commands("producer", &checks), producer);
    let actor: Vec<String> = std::iter::once("backlot".to_string())
        .chain(read_only.iter().map(|r| r.to_string()))
        .chain(["git add".to_string(), "git commit".to_string()])
        .chain(["cargo test --lib".to_string()])
        .collect();
    assert_eq!(allow_commands("actor-t1-c1", &checks), actor);
    let director: Vec<String> = std::iter::once("backlot".to_string())
        .chain(read_only.iter().map(|r| r.to_string()))
        .chain(["git merge".to_string()])
        .chain(["cargo test --lib".to_string()])
        .collect();
    assert_eq!(allow_commands("director", &checks), director);
    for rules in [
        allow_commands("actor-t1-c1", &checks),
        allow_commands("director", &checks),
        allow_commands("writer", &checks),
    ] {
        assert!(!rules.iter().any(|r| r == "git branch"), "{rules:?}");
    }
    // A check with a quote cannot sit in the script's quoted list.
    let odd = [check("odd", "echo 'x'")];
    let actor_odd: Vec<String> = std::iter::once("backlot".to_string())
        .chain(read_only.iter().map(|r| r.to_string()))
        .chain(["git add".to_string(), "git commit".to_string()])
        .collect();
    assert_eq!(allow_commands("actor-t1-c1", &odd), actor_odd);
    // Hollywood: exactly the overseer's set + `backlot progress`, `withdraw`, and `studio cast set --dry-run`.
    let h = allow_commands("hollywood", &[]);
    assert_eq!(h.len(), 10, "{h:?}");
    assert!(h.contains(&"backlot progress".to_string()));
    assert!(h.contains(&"backlot hollywood report".to_string()));
    assert!(h.contains(&"backlot hollywood withdraw".to_string()));
    assert!(h.contains(&"backlot studio cast set --dry-run".to_string()));
    assert!(h.contains(&"backlot guide".to_string()));
    assert!(!h.contains(&"backlot".to_string()));
}

fn tmp(tag: &str) -> TestDir {
    TestDir::new(&format!("bl-cline-{tag}"))
}

fn prep_in(dir: &Path) -> ClinePrep {
    ClinePrep {
        hook: dir.join(".cline/hooks/PreToolUse.sh"),
        hook_content: super::cline_hook::script(&super::cline_hook::Spec {
            worktree: dir.to_str().unwrap(),
            signals: &dir.join("role/cline-signals.jsonl"),
            allowed: &["backlot".to_string()],
            role: "actor-t1-c1",
        }),
        mcp: dir.join("role/cline-mcp.json"),
        signals: dir.join("role/cline-signals.jsonl"),
        cwd: dir.to_path_buf(),
    }
}

/// `prepare` writes the hook (mode 0755), the MCP mask and an empty
/// signals file, and keeps `/.cline/` in the worktree's exclude —
/// once, whatever the file had before.
#[test]
fn prepare_writes_its_files() {
    let dir = tmp("prep");
    fs::create_dir_all(dir.join(".git/info")).unwrap();
    fs::write(dir.join(".git/info/exclude"), "# mine\n*.log\n").unwrap();
    let prep = prep_in(&dir);
    assert!(!prepare(&prep).unwrap(), "nothing to heal");
    let hook = fs::read_to_string(&prep.hook).unwrap();
    assert!(hook.starts_with("#!/bin/sh"), "{hook}");
    assert!(hook.contains(dir.to_str().unwrap()), "{hook}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&prep.hook).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755, "{mode:o}");
    }
    assert_eq!(
        fs::read_to_string(&prep.mcp).unwrap(),
        "{\"mcpServers\":{}}\n"
    );
    assert_eq!(fs::read_to_string(&prep.signals).unwrap(), "");
    let exclude = fs::read_to_string(dir.join(".git/info/exclude")).unwrap();
    assert_eq!(exclude, "# mine\n*.log\n/.cline/\n");
    // Idempotent: a second prepare leaves the exclude as it was and
    // empties the signals again.
    fs::write(&prep.signals, "stale\n").unwrap();
    prepare(&prep).unwrap();
    let exclude = fs::read_to_string(dir.join(".git/info/exclude")).unwrap();
    assert_eq!(exclude, "# mine\n*.log\n/.cline/\n");
    assert_eq!(fs::read_to_string(&prep.signals).unwrap(), "");
}

/// A linked worktree's `.git` is a `gitdir:` pointer — the exclude
/// lives in the git directory it names.
#[test]
fn prepare_follows_a_worktree_gitdir() {
    let dir = tmp("prep-gitdir");
    let gitdir = dir.join("main/.git/worktrees/actor");
    fs::create_dir_all(&gitdir).unwrap();
    fs::write(dir.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
    let prep = prep_in(&dir);
    prepare(&prep).unwrap();
    let exclude = fs::read_to_string(gitdir.join("info/exclude")).unwrap();
    assert_eq!(exclude, "/.cline/\n");
}

/// A folder that is not a git repository still gets its hook and MCP
/// mask: there is no exclude file to write, and that is no failure.
#[test]
fn prepare_without_a_repository() {
    let dir = tmp("prep-nogit");
    let prep = prep_in(&dir);
    assert!(!prepare(&prep).unwrap());
    assert!(prep.hook.is_file());
    assert!(prep.mcp.is_file());
}

/// The launch carries the prep and the MCP env: the hook in the role's
/// work area, the mask file and the signals file under
/// `roles/<role>/`, and `CLINE_MCP_SETTINGS_PATH` pointing at the
/// mask.
#[test]
fn plan_carries_its_prep() {
    let env = BTreeMap::new();
    let c = ctx(&env, None, None);
    let l = plan(&ident(MODEL, Some("medium"), "standard"), &c).unwrap();
    assert_eq!(
        l.env.get("CLINE_MCP_SETTINGS_PATH").map(String::as_str),
        Some("/home/user/state/studios/acme-shop/roles/producer/cline-mcp.json")
    );
    let Some(Prep::Cline(p)) = l.prep else {
        panic!("a cline launch carries its prep")
    };
    assert_eq!(
        p.hook,
        Path::new("/home/user/acme-shop/.cline/hooks/PreToolUse.sh")
    );
    assert_eq!(
        p.signals,
        Path::new("/home/user/state/studios/acme-shop/roles/producer/cline-signals.jsonl")
    );
    assert_eq!(p.cwd, Path::new("/home/user/acme-shop"));
    assert!(
        p.hook_content.contains("backlot"),
        "the rules are in the hook"
    );
    assert!(p.hook_content.contains("/home/user/acme-shop"));
}
