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
    assert_eq!(
        doc["cwd"],
        code.canonicalize().unwrap().to_string_lossy().as_ref()
    );
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
    assert_eq!(
        doc["cwd"],
        home.canonicalize().unwrap().to_string_lossy().as_ref()
    );
    let tilde = json(&env, "repos --json --from ~/code");
    assert_eq!(
        tilde["cwd"],
        home.join("code")
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .as_ref()
    );
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
    assert_eq!(
        doc["cwd"],
        home.join("a")
            .join("b")
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .as_ref()
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
