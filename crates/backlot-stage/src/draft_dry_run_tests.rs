//! Fail-first tests for `studio new --dry-run` noting when a cast asks
//! for `worktree` on runners whose `standard` and `worktree` profiles are
//! identical (`devin`, `agy`, `cline`, `codex`, `opencode`).

use std::fs;
use std::path::Path;

use crate::cli::Env;
use crate::cli_testkit::{Scratch, fixture, git, ok, scratch};

fn repo(scratch: &Scratch) -> String {
    let dir = scratch.files.join("repo");
    fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-b", "main"]);
    dir.to_string_lossy().into_owned()
}

fn stub_runners(scratch: &Scratch, env: &Env, bins: &[&str]) -> Env {
    let bin = scratch.files.join("bin");
    fs::create_dir_all(&bin).unwrap();
    for name in bins {
        let p = bin.join(name);
        fs::write(&p, "#!/bin/sh\necho 1.2.3\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let mut vars = env.vars.clone();
    vars.remove("BACKLOT_STUDIO");
    let orig_path = vars.get("PATH").cloned().unwrap_or_default();
    let new_path = if orig_path.is_empty() {
        bin.display().to_string()
    } else {
        format!("{}:{orig_path}", bin.display())
    };
    vars.insert("PATH".into(), new_path);
    Env { vars }
}

fn write_and_run_dry(
    scratch: &Scratch,
    env: &Env,
    repo_dir: &str,
    studio: &str,
    cast_toml: &str,
) -> String {
    let cast = fixture(scratch, &format!("{studio}.toml"), cast_toml);
    let out = ok(
        env,
        &format!(
            "studio new --title Shop --repo {repo_dir} --cast {cast} --dry-run --studio {studio}"
        ),
    );
    assert!(out.contains("dry run:"), "{out}");
    assert!(
        !Path::new(&env.vars["BACKLOT_STATE_DIR"])
            .join("studios")
            .join(studio)
            .exists()
    );
    out
}

#[test]
fn studio_new_dry_run_notes_runners_with_identical_worktree_profile() {
    let (scratch, base_env) = scratch();
    let env = stub_runners(
        &scratch,
        &base_env,
        &[
            "cmd",
            "codex",
            "opencode",
            "devin",
            "agy",
            "cline",
            "claude",
            "cursor-agent",
        ],
    );
    let repo_dir = repo(&scratch);

    // Identical-profile runners on `worktree` produce a note naming them.
    let mixed_cast = r#"
[producer]
runner = "cmd"
model = "m-p"
permission_profile = "worktree"

[director]
runner = "claude"
model = "m-d"
effort = "high"
permission_profile = "standard"

[writer]
runner = "cursor"
model = "m-w"
permission_profile = "worktree"

[[candidates]]
name = "actor-a"
runner = "codex"
model = "m-c"
effort = "medium"
permission_profile = "worktree"

[[candidates]]
name = "actor-b"
runner = "opencode"
model = "opencode/m"
permission_profile = "worktree"
"#;
    let out = write_and_run_dry(&scratch, &env, &repo_dir, "mixed-a", mixed_cast);
    assert!(out.contains("codex"), "{out}");
    assert!(out.contains("opencode"), "{out}");
    assert!(
        out.contains("same") || out.contains("identical"),
        "expected note explaining worktree matches standard: {out}"
    );
    assert!(
        !out.contains("cmd"),
        "cmd has a distinct worktree profile: {out}"
    );
    assert!(
        !out.contains("cursor"),
        "cursor has a distinct worktree profile: {out}"
    );

    // Cast using `worktree` only on cmd/cursor/claude (and standard on codex) prints no note.
    let distinct_cast = r#"
[producer]
runner = "cmd"
model = "m-p"
permission_profile = "worktree"

[director]
runner = "claude"
model = "m-d"
effort = "high"
permission_profile = "worktree"

[writer]
runner = "codex"
model = "m-w"
effort = "medium"
permission_profile = "standard"

[[candidates]]
name = "actor-a"
runner = "cursor"
model = "m-c"
permission_profile = "worktree"
"#;
    let clean = write_and_run_dry(&scratch, &env, &repo_dir, "distinct-a", distinct_cast);
    assert!(!clean.contains("note:"), "{clean}");
}
