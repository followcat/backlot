//! `--cast -` reads stdin once for every `studio new` reader: the
//! dry run's check and notes, and the create path's record.

use std::fs;

use crate::cli::{Env, run_stdin};
use crate::cli_testkit::{git, scratch};
use crate::store;

const DEVIN_CAST: &str = r#"
mode = "standard"
rework_cap = 2

[producer]
runner = "devin"
model = "model-p"
effort = "high"
permission_profile = "worktree"

[director]
runner = "devin"
model = "model-d"
effort = "high"
permission_profile = "worktree"

[writer]
runner = "devin"
model = "model-w"
effort = "high"
permission_profile = "worktree"

[[candidates]]
name = "actor-a"
runner = "devin"
model = "model-a"
effort = "high"
permission_profile = "worktree"
"#;

fn repo_and_env(scratch: &crate::cli_testkit::Scratch, env: &Env) -> (String, Env) {
    let repo = scratch.files.join("repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    let bin = scratch.files.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let devin = bin.join("devin");
    fs::write(&devin, "#!/bin/sh\necho devin 1.0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&devin, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut vars = env.vars.clone();
    let path = vars.get("PATH").cloned().unwrap_or_default();
    vars.insert(
        "PATH".into(),
        if path.is_empty() {
            bin.display().to_string()
        } else {
            format!("{}:{path}", bin.display())
        },
    );
    (repo.to_string_lossy().into_owned(), Env { vars })
}

fn studio_new_args(repo: &str, dry_run: bool) -> Vec<String> {
    let mut args: Vec<String> = [
        "studio",
        "new",
        "--title",
        "Shop",
        "--repo",
        repo,
        "--cast",
        "-",
        "--studio",
        "acme-shop",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    if dry_run {
        args.push("--dry-run".into());
    }
    args
}

#[test]
fn studio_new_dry_run_reads_a_stdin_cast_once_and_prints_notes() {
    let (scratch, env) = scratch();
    let (repo, env) = repo_and_env(&scratch, &env);

    let out = run_stdin(&studio_new_args(&repo, true), &env, Some(DEVIN_CAST)).unwrap();

    assert!(out.contains("dry run:"), "{out}");
    assert!(out.contains("nothing recorded"), "{out}");
    assert!(
        out.contains(
            "note: permission_profile = \"worktree\" is identical to \"standard\" on devin"
        ),
        "{out}"
    );
    assert!(!scratch.dir.join("studios").exists());
}

#[test]
fn studio_new_create_reads_a_stdin_cast_once_and_records_it() {
    let (scratch, env) = scratch();
    let (repo, env) = repo_and_env(&scratch, &env);

    let out = run_stdin(&studio_new_args(&repo, false), &env, Some(DEVIN_CAST)).unwrap();

    assert!(out.contains("studio.created"), "{out}");
    let state = store::load_state(&store::studio_dir(&scratch.dir, "acme-shop")).unwrap();
    let studio = state.studio.as_ref().expect("studio.created recorded");
    assert_eq!(studio.roles.producer.runner, "devin");
    assert_eq!(studio.roles.director.model, "model-d");
    assert_eq!(studio.roles.writer.permission_profile, "worktree");
    assert_eq!(studio.candidates[0].name, "actor-a");
}
