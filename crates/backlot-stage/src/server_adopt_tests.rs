//! Fail-first tests for `backlot server adopt`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::cli::{self, Env, Fail};
use crate::cli_testkit::{CAST_TOML, git};
use crate::server_fake_core::{FakeHcCore, base_env};
use crate::test_support::TestDir;
use crate::workareas::worktree_listed;

fn args(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| (*s).to_string()).collect()
}

fn seed_studio(base: &Path, name: &str, repo: Option<&Path>) {
    let cast = base.join(format!("cast-{name}.toml"));
    fs::create_dir_all(base).unwrap();
    fs::write(&cast, CAST_TOML).unwrap();
    let repo_s = repo
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/home/user/acme-shop".to_string());
    let mut vars = BTreeMap::new();
    vars.insert("BACKLOT_STATE_DIR".into(), base.to_string_lossy().into());
    let env = Env { vars };
    cli::run(
        &args(&[
            "studio",
            "new",
            "--title",
            name,
            "--repo",
            &repo_s,
            "--cast",
            &cast.to_string_lossy(),
            "--studio",
            name,
        ]),
        &env,
    )
    .expect("seed studio");
    fs::remove_file(&cast).unwrap();
}

#[test]
fn adopt_refuses_running_legacy_stage_and_stops_when_stop_stages_passed() {
    let dir = TestDir::new("bla-stop");
    let root = dir.join("xdg-state/backlot");
    let core_dir = root.join("core-a");
    seed_studio(&core_dir, "acme", None);
    fs::write(
        core_dir.join("config.toml"),
        "[runner_env.cmd]\nX = \"1\"\n",
    )
    .unwrap();

    let core = FakeHcCore::spawn(dir.join("hc.sock"), "core-a");
    core.add_terminal(
        Some("backlot-stage"),
        &[("backlot.role", "stage"), ("herdr.tab", "stage")],
        &["backlot", "stage", "run"],
        "live",
    );

    let mut vars = base_env(&dir);
    vars.insert("HC_SOCKET".into(), core.socket.to_string_lossy().into());
    let env = Env { vars };

    // Without `--stop-stages`, adopt refuses cleanly and changes nothing.
    let err = cli::run(&args(&["server", "adopt"]), &env).unwrap_err();
    assert!(matches!(err, Fail::Refused(_)), "{err:?}");
    assert!(
        err.message().contains("backlot-stage") && err.message().contains("--stop-stages"),
        "{}",
        err.message()
    );
    assert!(core.killed().is_empty());
    assert!(!root.join("server/studios").exists());

    // With `--stop-stages`, adopt kills legacy stage and migrates cleanly.
    let out =
        cli::run(&args(&["server", "adopt", "--stop-stages"]), &env).expect("adopt --stop-stages");
    assert!(out.contains("core-a"), "{out}");
    assert_eq!(core.killed(), vec!["t1"]);
    assert!(root.join("server/studios/acme/events.jsonl").is_file());
    assert!(root.join("server/config.toml").is_file());
    assert!(
        fs::symlink_metadata(core_dir.join("studios"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(core_dir.join("server-pointer.json").is_file());
}

#[test]
fn adopt_promotes_primary_preserves_git_worktree_archives_secondary_and_blocks_stage() {
    let dir = TestDir::new("bla-full");
    let root = dir.join("xdg-state/backlot");
    let primary = root.join("core-primary");
    let empty_core = root.join("core-empty");
    let secondary = root.join("core-old");

    // Real git repository + live worktree inside `core-primary/studios/acme/work/t1`.
    let repo = dir.join("repo");
    fs::create_dir_all(&repo).unwrap();
    crate::cli_testkit::git(&repo, &["init", "-b", "main"]);
    fs::write(repo.join("README.md"), "acme\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "init"]);

    seed_studio(&primary, "acme", Some(&repo));
    let wt_via_core = primary.join("studios/acme/work/t1");
    fs::create_dir_all(wt_via_core.parent().unwrap()).unwrap();
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            "backlot/acme/t1",
            &wt_via_core.to_string_lossy(),
            "main",
        ],
    );
    let gitdir_file = repo.join(".git/worktrees/t1/gitdir");
    let gitdir_before = fs::read_to_string(&gitdir_file).unwrap();

    fs::create_dir_all(primary.join("hollywood")).unwrap();
    fs::write(primary.join("hollywood/events.jsonl"), "{\"seq\":1}\n").unwrap();
    fs::create_dir_all(primary.join("archive/done")).unwrap();
    fs::write(primary.join("archive/done/events.jsonl"), "{\"seq\":1}\n").unwrap();
    fs::write(
        primary.join("config.toml"),
        "[stage]\nanswer_trust = false\n",
    )
    .unwrap();
    fs::write(primary.join("agent.env"), "FOO=bar\n").unwrap();
    fs::write(primary.join("processed-reqs.jsonl"), "{\"id\":\"r1\"}\n").unwrap();

    fs::create_dir_all(&empty_core).unwrap();
    seed_studio(&secondary, "beta", None);

    let env = Env {
        vars: base_env(&dir),
    };
    let out = cli::run(&args(&["server", "adopt", "--core", "core-primary"]), &env)
        .expect("server adopt");
    assert!(out.contains("core-primary"), "{out}");

    let server = root.join("server");
    assert!(server.join("studios/acme/events.jsonl").is_file());
    assert!(server.join("hollywood/events.jsonl").is_file());
    assert!(server.join("archive/done/events.jsonl").is_file());
    assert!(server.join("config.toml").is_file());
    assert!(server.join("agent.env").is_file());
    assert!(server.join("processed-reqs.jsonl").is_file());

    // Relative symlinks keep live paths working and `.git/worktrees/*/gitdir` untouched.
    for entry in [
        "studios",
        "hollywood",
        "archive",
        "config.toml",
        "agent.env",
        "processed-reqs.jsonl",
    ] {
        let link = primary.join(entry);
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "{} must be a symlink",
            link.display()
        );
        assert_eq!(
            fs::read_link(&link).unwrap(),
            Path::new(&format!("../server/{entry}"))
        );
    }
    assert_eq!(fs::read_to_string(&gitdir_file).unwrap(), gitdir_before);
    let wt_list = git(&repo, &["worktree", "list", "--porcelain"]);
    let wt_via_server = server.join("studios/acme/work/t1");
    assert!(
        worktree_listed(&wt_list, &wt_via_server, "backlot/acme/t1"),
        "server path must recognize core symlink worktree:\n{wt_list}"
    );

    // Pinned `BACKLOT_STATE_DIR=<primary>` still sees `acme` via symlinks.
    let mut pinned_vars = base_env(&dir);
    pinned_vars.insert("BACKLOT_STATE_DIR".into(), primary.to_string_lossy().into());
    let st = cli::run(
        &args(&["status", "--studio", "acme"]),
        &Env { vars: pinned_vars },
    )
    .expect("pinned status after adopt");
    assert!(st.contains("acme"), "{st}");

    // Secondary & empty cores archived under `server/adopted-<stamp>/`, retired with tombstones.
    let adopted_dir = fs::read_dir(&server)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("adopted-"))
        })
        .expect("server/adopted-<stamp> created");
    assert!(adopted_dir.join("core-empty").is_dir());
    assert!(
        adopted_dir
            .join("core-old/studios/beta/events.jsonl")
            .is_file()
    );
    assert!(!secondary.join("studios").exists());

    for retired in [&primary, &empty_core, &secondary] {
        let ptr_path = retired.join("server-pointer.json");
        assert!(ptr_path.is_file(), "missing {}", ptr_path.display());
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&ptr_path).unwrap()).unwrap();
        assert_eq!(v["session"], "backlot");
    }

    // Unpinned `studio list` skips retired pointer dirs so `acme` is not listed twice and `beta` is gone.
    let list_out = cli::run(&args(&["studio", "list"]), &env).expect("studio list");
    assert_eq!(
        list_out.lines().filter(|l| l.starts_with("acme ")).count(),
        1,
        "{list_out}"
    );
    assert!(!list_out.contains("beta"), "{list_out}");

    // `stage start` against either retired core refuses with the exact pointer message.
    let fake_primary = FakeHcCore::spawn(dir.join("hc1.sock"), "core-primary");
    let mut vars1 = base_env(&dir);
    vars1.insert(
        "HC_SOCKET".into(),
        fake_primary.socket.to_string_lossy().into(),
    );
    let err1 = cli::run(&args(&["stage", "start"]), &Env { vars: vars1 }).unwrap_err();
    assert_eq!(
        err1.message(),
        "managed by backlot-server (--session backlot)"
    );

    let mut vars_pinned = base_env(&dir);
    vars_pinned.insert(
        "BACKLOT_STATE_DIR".into(),
        secondary.to_string_lossy().into(),
    );
    let err2 = cli::run(&args(&["stage", "start"]), &Env { vars: vars_pinned }).unwrap_err();
    assert_eq!(
        err2.message(),
        "managed by backlot-server (--session backlot)"
    );
}

#[test]
fn adopt_dry_run_prints_plan_and_changes_nothing_on_disk() {
    let dir = TestDir::new("bla-dry");
    let root = dir.join("xdg-state/backlot");
    let primary = root.join("core-a");
    let empty_core = root.join("core-b");
    seed_studio(&primary, "acme", None);
    fs::write(primary.join("config.toml"), "[stage]\n").unwrap();
    fs::create_dir_all(&empty_core).unwrap();

    let core = FakeHcCore::spawn(dir.join("hc.sock"), "core-a");
    core.add_terminal(
        Some("backlot-stage"),
        &[("backlot.role", "stage"), ("herdr.tab", "stage")],
        &["backlot", "stage", "run"],
        "live",
    );

    let mut vars = base_env(&dir);
    vars.insert("HC_SOCKET".into(), core.socket.to_string_lossy().into());
    let env = Env { vars };

    let out = cli::run(
        &args(&["server", "adopt", "--dry-run", "--stop-stages"]),
        &env,
    )
    .expect("dry-run output");
    assert!(out.contains("dry-run") || out.contains("would "), "{out}");
    assert!(out.contains("core-a"), "{out}");
    assert!(out.contains("server/adopted-"), "{out}");
    assert!(out.contains("server-pointer.json"), "{out}");

    // Nothing touched on disk, and no terminal killed.
    assert!(core.killed().is_empty());
    assert!(!root.join("server").exists());
    assert!(!primary.join("server-pointer.json").exists());
    assert!(!empty_core.join("server-pointer.json").exists());
    assert!(primary.join("studios/acme/events.jsonl").is_file());
    assert!(
        !fs::symlink_metadata(primary.join("studios"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(empty_core.is_dir());
}
