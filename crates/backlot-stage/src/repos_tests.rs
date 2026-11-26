//! `backlot repos` tests (G4): entry classification, the `$HOME`
//! default, the `--from` scope rules and `in_use_studio` — all in
//! scratch trees, real git for the repo kinds.

use std::fs;
use std::path::Path;

use crate::cli::Env;
use crate::cli_testkit::*;

/// A scratch state dir plus a `home` the listing scopes to.
fn home_env() -> (Scratch, Env, std::path::PathBuf) {
    let (scratch, mut env) = scratch();
    let home = scratch.files.join("home");
    fs::create_dir_all(&home).unwrap();
    env.vars
        .insert("HOME".into(), home.to_string_lossy().into_owned());
    (scratch, env, home)
}

/// `path` in the `--json` document, as a value.
fn json(env: &Env, args: &str) -> serde_json::Value {
    let out = ok(env, args);
    serde_json::from_str(&out).unwrap_or_else(|e| panic!("bad json '{out}': {e}"))
}

fn entry<'a>(doc: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    doc["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == name)
        .unwrap_or_else(|| panic!("no entry '{name}' in {doc}"))
}

/// A git repo with one commit, or with none when `commit` is false.
fn repo(path: &Path, commit: bool) {
    fs::create_dir_all(path).unwrap();
    git(path, &["init", "-b", "main"]);
    if commit {
        fs::write(path.join("f"), "base\n").unwrap();
        git(path, &["add", "."]);
        git(path, &["commit", "-m", "base"]);
    }
}

#[test]
fn repos_json_classifies_entries() {
    let (_scratch, env, home) = home_env();
    let code = home.join("code");
    repo(&code.join("beta-api"), true);
    repo(&code.join("proto-kv"), false);
    fs::create_dir_all(code.join("services")).unwrap();
    fs::write(code.join("README"), "not a dir\n").unwrap();

    let doc = json(&env, &format!("repos --json --from {}", code.display()));
    crate::test_support::assert_same_path(doc["cwd"].as_str().unwrap_or(""), &code);
    // git_ready, then git_empty, then plain dirs — each by name.
    let names: Vec<&str> = doc["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["beta-api", "proto-kv", "services"]);
    assert_eq!(entry(&doc, "beta-api")["kind"], "git_ready");
    assert_eq!(entry(&doc, "beta-api")["branch"], "main");
    assert_eq!(entry(&doc, "proto-kv")["kind"], "git_empty");
    assert_eq!(entry(&doc, "proto-kv")["branch"], "main");
    assert_eq!(entry(&doc, "services")["kind"], "dir");
    assert!(entry(&doc, "services")["branch"].is_null());
    // Files are not entries.
    assert!(
        doc["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["name"] != "README")
    );
}

#[test]
fn repos_default_and_tilde_are_home() {
    let (_scratch, env, home) = home_env();
    fs::create_dir_all(home.join("code")).unwrap();
    let doc = json(&env, "repos --json");
    crate::test_support::assert_same_path(doc["cwd"].as_str().unwrap_or(""), &home);
    let tilde = json(&env, "repos --json --from ~/code");
    crate::test_support::assert_same_path(tilde["cwd"].as_str().unwrap_or(""), home.join("code"));
}

#[test]
fn repos_from_stays_inside_home() {
    let (_scratch, env, home) = home_env();
    fs::create_dir_all(home.join("a").join("b")).unwrap();
    // `..` that resolves back inside $HOME is fine.
    let doc = json(
        &env,
        &format!("repos --json --from {}/a/../a/b", home.display()),
    );
    crate::test_support::assert_same_path(
        doc["cwd"].as_str().unwrap_or(""),
        home.join("a").join("b"),
    );
    // `..` escaping $HOME is refused — another user's tree never lists.
    let err = refused(
        &env,
        &format!("repos --json --from {}/a/../..", home.display()),
    );
    assert!(err.contains("outside $HOME"), "{err}");
    assert!(refused(&env, "repos --json --from /").contains("outside $HOME"));
    // A relative --from is a usage error.
    assert!(usage(&env, "repos --from code").contains("absolute"));
    // A missing directory is refused.
    assert!(refused(&env, "repos --from ~/nope").contains("--from"));
}

/// A studio's recorded repo marks `in_use_studio`; a sibling stays
/// `null`.
#[test]
fn repos_marks_a_repo_in_use() {
    let (scratch, env, home) = home_env();
    let code = home.join("code");
    repo(&code.join("beta-api"), true);
    fs::create_dir_all(code.join("free")).unwrap();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!(
            "studio new --title t --repo {} --cast {cast}",
            code.join("beta-api").display()
        ),
    );
    let doc = json(&env, &format!("repos --json --from {}", code.display()));
    assert_eq!(entry(&doc, "beta-api")["in_use_studio"], "acme-shop");
    assert!(entry(&doc, "free")["in_use_studio"].is_null());
}

/// A `dir` entry's `members` names its direct-child repositories —
/// the workspace member list the dialog reads (D4); a studio on a
/// workspace marks each member `in_use_studio`.
#[test]
fn dir_entries_carry_their_member_repositories() {
    let (scratch, env, home) = home_env();
    let mono = home.join("mono");
    repo(&mono.join("beta-api"), true);
    repo(&mono.join("web"), true);
    fs::create_dir_all(mono.join("docs")).unwrap();

    let doc = json(&env, &format!("repos --json --from {}", home.display()));
    let members: Vec<&str> = entry(&doc, "mono")["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m.as_str().unwrap())
        .collect();
    assert_eq!(members, ["beta-api", "web"]);
    // Repository entries carry no `members` key at all.
    let inner = json(&env, &format!("repos --json --from {}", mono.display()));
    assert!(entry(&inner, "beta-api")["members"].is_null());
    assert!(entry(&inner, "docs")["members"].is_null());

    // A workspace studio marks each member in use — never the
    // workspace directory's own (nonexistent) repository.
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!(
            "studio new --title t --repo {} --cast {cast}",
            mono.display()
        ),
    );
    let doc = json(&env, &format!("repos --json --from {}", mono.display()));
    assert_eq!(entry(&doc, "beta-api")["in_use_studio"], "acme-shop");
    assert_eq!(entry(&doc, "web")["in_use_studio"], "acme-shop");
    assert!(entry(&doc, "docs")["in_use_studio"].is_null());
}

/// A `.git` mark git refuses — an empty directory — lists as a
/// plain `dir`, the same answer `paths::classify` gives the launch,
/// so the dialog's shape can never disagree with the dry-run's
/// check. A broken child stops counting a parent as a workspace.
#[test]
fn a_broken_git_mark_lists_as_a_plain_directory() {
    let (_scratch, env, home) = home_env();
    fs::create_dir_all(home.join("aiflows").join(".git")).unwrap();
    repo(&home.join("acme-shop"), true);
    fs::create_dir_all(home.join("mono").join("rotten").join(".git")).unwrap();

    let doc = json(&env, &format!("repos --json --from {}", home.display()));
    assert_eq!(entry(&doc, "aiflows")["kind"], "dir");
    assert_eq!(entry(&doc, "acme-shop")["kind"], "git_ready");
    assert_eq!(entry(&doc, "mono")["kind"], "dir");
    assert!(entry(&doc, "mono")["members"].is_null());
}

/// A `dir` entry's `members` names distinct repositories only:
/// linked worktrees of one repository collapse to a single member
/// — the same list `paths::member_repos` hands the launch (D4).
#[test]
fn linked_worktrees_count_once_in_members() {
    let (_scratch, env, home) = home_env();
    let mono = home.join("mono");
    let core = mono.join("core");
    repo(&core, true);
    git(
        &core,
        &[
            "worktree",
            "add",
            "-b",
            "wt",
            &mono.join("core-verify").to_string_lossy(),
        ],
    );
    repo(&mono.join("web"), true);

    let doc = json(&env, &format!("repos --json --from {}", home.display()));
    let members: Vec<&str> = entry(&doc, "mono")["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m.as_str().unwrap())
        .collect();
    assert_eq!(members, ["core", "web"]);
}

/// Dot-directories stay out of a plain listing — `$HOME` opens on
/// real work, not on `.cache` — and `--all` brings them back.
#[test]
fn repos_hides_dot_directories_unless_all() {
    let (_scratch, env, home) = home_env();
    fs::create_dir_all(home.join(".cache")).unwrap();
    fs::create_dir_all(home.join(".config").join("app-a")).unwrap();
    repo(&home.join("code"), true);

    let doc = json(&env, "repos --json");
    let names: Vec<&str> = doc["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["code"], "dot-directories hidden: {doc}");
    let table = ok(&env, "repos");
    assert!(
        !table.contains(".cache"),
        "the table hides them too: {table}"
    );

    let doc = json(&env, "repos --json --all");
    let names: Vec<&str> = doc["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    // The git repository still leads; the dot-directories follow by
    // name.
    assert_eq!(names, ["code", ".cache", ".config"]);
}

/// Git repositories lead, a workspace over member repositories
/// follows, and a plain `will git init` directory comes last — never
/// pre-selected above real work on `$HOME`.
#[test]
fn repos_sorts_git_then_workspace_then_plain() {
    let (_scratch, env, home) = home_env();
    repo(&home.join("zeta-repo"), true);
    fs::create_dir_all(home.join("alpha-dir")).unwrap();
    repo(&home.join("mid-work").join("member"), true);

    let doc = json(&env, "repos --json");
    let names: Vec<&str> = doc["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["zeta-repo", "mid-work", "alpha-dir"]);
    assert_eq!(entry(&doc, "mid-work")["kind"], "dir");
    assert_eq!(
        entry(&doc, "mid-work")["members"],
        serde_json::json!(["member"])
    );
}

/// Explicit reproduction check for the linked-worktree workspace shape
/// (`acme-suite/core` + `acme-suite/core-wt1`): `members` lists `core` once.
#[test]
fn acme_suite_linked_worktree_dedup_members() {
    let (_scratch, env, home) = home_env();
    let suite = home.join("acme-suite");
    let core = suite.join("core");
    repo(&core, true);
    crate::cli_testkit::git(
        &core,
        &[
            "worktree",
            "add",
            "-b",
            "wt1",
            &suite.join("core-wt1").to_string_lossy(),
        ],
    );
    let doc = json(&env, &format!("repos --json --from {}", home.display()));
    let members: Vec<&str> = entry(&doc, "acme-suite")["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m.as_str().unwrap())
        .collect();
    assert_eq!(members, ["core"]);
}
