//! The cursor adapter's pieces: the per-role `allow` rules, the
//! byte-exact `cli-config.json` permissions splice, `prepare`'s
//! directory and file modes, and the chats-tree session lookup.
//! A fake `authInfo` key proves the splice preserves what it never
//! reads — and no error ever carries a byte of it.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use backlot_model::event::CheckDecl;

use super::cursor::{CursorPrep, allow_rules, config_content, prepare, session_at, slug};
use super::{ActorIdentity, LaunchCtx, Prep, plan};

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bl-cursor-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn check(name: &str, run: &str) -> CheckDecl {
    CheckDecl {
        name: name.into(),
        run: run.into(),
        timeout_s: 600,
    }
}

/// The slug cursor computes for a workdir — non-alphanumeric runs
/// become one `-`, edges trimmed.
#[test]
fn slug_is_the_bundles() {
    assert_eq!(slug("/home/user/acme-shop"), "home-user-acme-shop");
    assert_eq!(slug("/home/user/a__b//c"), "home-user-a-b-c");
    assert_eq!(slug("/---"), "");
    assert_eq!(
        slug("/home/user/with space/x.y"),
        "home-user-with-space-x-y"
    );
}

/// `allow` rules per role: the base `Shell(backlot)`; an actor's
/// six git commands; the director's read-only set and `git merge`,
/// never `git branch`; the declared checks verbatim — and a `run`
/// the rule syntax cannot express is simply never there.
#[test]
fn rules_per_role() {
    let checks = [
        check("unit", "cargo test --lib"),
        check("lint", "cargo clippy"),
    ];
    assert_eq!(
        allow_rules("director", &checks),
        [
            "Shell(backlot)",
            "Shell(git status)",
            "Shell(git diff)",
            "Shell(git log)",
            "Shell(git show)",
            "Shell(git merge)",
            "Shell(cargo test --lib)",
            "Shell(cargo clippy)"
        ]
    );
    assert_eq!(
        allow_rules("actor-t1-c1", &checks),
        [
            "Shell(backlot)",
            "Shell(git add)",
            "Shell(git commit)",
            "Shell(git status)",
            "Shell(git diff)",
            "Shell(git log)",
            "Shell(git show)",
            "Shell(cargo test --lib)",
            "Shell(cargo clippy)"
        ]
    );
    // Producer and writer keep the base rule only.
    for role in ["producer", "writer"] {
        assert_eq!(allow_rules(role, &checks), ["Shell(backlot)"], "{role}");
    }
    // `(` `)` `,` `*?[]{}` `:` and `\` are rule syntax or glob —
    // none may reach the file.
    for bad in [
        "make (all)",
        "a,b",
        "cargo test -- -p*",
        "cargo test?",
        "ls [ab]",
        "echo {x}",
        "echo a:b",
        "echo a\\b",
    ] {
        let rules = allow_rules("director", &[check("odd", bad)]);
        assert!(!rules.iter().any(|r| r.contains(bad)), "{bad}: {rules:?}");
    }
    // Nothing broad ever lands.
    for role in ["director", "actor-t1-c1", "producer"] {
        for rule in allow_rules(role, &checks) {
            assert_ne!(rule, "Shell(git)", "{role}");
            assert!(!rule.contains('*'), "{role}: {rule}");
        }
    }
}

/// A config that does not exist yet is written with the minimal
/// schema cursor itself writes: `version`, `editor`, `permissions`.
#[test]
fn config_written_fresh() {
    let allow = vec!["Shell(backlot)".to_string()];
    let text = String::from_utf8(config_content(None, &allow)).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["version"], 1);
    assert_eq!(v["editor"], serde_json::json!({"vimMode": false}));
    assert_eq!(
        v["permissions"],
        serde_json::json!({"allow": ["Shell(backlot)"], "deny": []})
    );
    // Corrupt content is healed, not preserved.
    let healed = String::from_utf8(config_content(Some(b"not json"), &allow)).unwrap();
    assert_eq!(healed, text);
}

/// An existing config keeps every other byte — whitespace and key
/// order included — while `permissions` is replaced. The fake
/// `authInfo` member survives untouched and is never parsed.
#[test]
fn config_splice_preserves_unrelated_bytes() {
    let existing = "{\n  \"authInfo\": {\"k\": \"fake-token\", \"nested\": [1,{\"deep\": true}]},\n  \"permissions\": {\"allow\": [\"Shell(old)\"], \"deny\": []},\n  \"selectedModel\": {\"x\": 1}\n}\n";
    let allow = vec!["Shell(backlot)".to_string()];
    let out = String::from_utf8(config_content(Some(existing.as_bytes()), &allow)).unwrap();
    assert!(
        out.contains("\"authInfo\": {\"k\": \"fake-token\", \"nested\": [1,{\"deep\": true}]},"),
        "{out}"
    );
    assert!(
        out.contains("\"permissions\": {\"allow\":[\"Shell(backlot)\"],\"deny\":[]}"),
        "{out}"
    );
    assert!(out.contains("\"selectedModel\": {\"x\": 1}"), "{out}");
    // No `permissions` key yet — the member is appended before the
    // closing brace.
    let added = String::from_utf8(config_content(
        Some(b"{\"authInfo\": \"fake-token\"}"),
        &allow,
    ))
    .unwrap();
    assert!(added.contains("\"authInfo\": \"fake-token\""), "{added}");
    assert!(added.contains("\"permissions\":"), "{added}");
    let v: serde_json::Value = serde_json::from_str(&added).unwrap();
    assert_eq!(v["permissions"]["allow"][0], "Shell(backlot)");
}

/// `prepare` makes the isolated tree and writes both files — mode
/// 700 on directories, 600 on files — listing every server the
/// global `mcp.json` names. An error names only paths, never file
/// content.
#[test]
#[cfg(unix)]
fn prepare_writes_the_isolated_tree() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmp("prep");
    let home = dir.join("home");
    fs::create_dir_all(home.join(".cursor")).unwrap();
    fs::write(
        home.join(".cursor/mcp.json"),
        r#"{"mcpServers":{"web-tools":{"x":1},"other":{"y":2}}}"#,
    )
    .unwrap();
    let prep = CursorPrep {
        root: dir.join("root"),
        project: slug("/home/user/acme-shop"),
        allow: allow_rules("director", &[]),
        global_mcp: home.join(".cursor/mcp.json"),
    };
    prepare(&prep).unwrap();
    for d in [
        "root/config",
        "root/sandbox",
        "root/data/projects/home-user-acme-shop",
    ] {
        let mode = fs::metadata(prep.root.parent().unwrap().join(d))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700, "{d}");
    }
    let config = prep.root.join("config/cli-config.json");
    let disabled = prep
        .root
        .join("data/projects/home-user-acme-shop/mcp-disabled.json");
    for f in [&config, &disabled] {
        let mode = fs::metadata(f).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "{}", f.display());
    }
    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
    assert_eq!(
        v["permissions"]["allow"],
        serde_json::json!([
            "Shell(backlot)",
            "Shell(git status)",
            "Shell(git diff)",
            "Shell(git log)",
            "Shell(git show)",
            "Shell(git merge)"
        ])
    );
    let names: Vec<String> = serde_json::from_str(&fs::read_to_string(&disabled).unwrap()).unwrap();
    assert_eq!(names, ["other", "web-tools"], "sorted server names");
}

/// A drifted directory mode heals on the next `prepare`; a missing
/// global `mcp.json` writes an empty disable list.
#[test]
#[cfg(unix)]
fn prepare_heals_modes_and_handles_no_mcp() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmp("prep-heal");
    let config = dir.join("root/config");
    fs::create_dir_all(&config).unwrap();
    fs::set_permissions(&config, fs::Permissions::from_mode(0o755)).unwrap();
    let prep = CursorPrep {
        root: dir.join("root"),
        project: "home-user-acme-shop".into(),
        allow: vec!["Shell(backlot)".into()],
        global_mcp: dir.join("home/.cursor/mcp.json"),
    };
    prepare(&prep).unwrap();
    assert_eq!(
        fs::metadata(&config).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let names: Vec<String> = serde_json::from_str(
        &fs::read_to_string(dir.join("root/data/projects/home-user-acme-shop/mcp-disabled.json"))
            .unwrap(),
    )
    .unwrap();
    assert!(names.is_empty());
}

/// `prepare` errors carry a path and the cause only — a fake
/// `authInfo` sitting in the existing config, or the raw bytes of a
/// malformed `mcp.json`, never reach the error string.
#[test]
#[cfg(unix)]
fn prepare_errors_never_carry_config_content() {
    let dir = tmp("prep-err");
    let config_dir = dir.join("root/config");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("cli-config.json"),
        "{\"authInfo\": \"fake-secret\"}",
    )
    .unwrap();
    let mcp = dir.join("home/.cursor/mcp.json");
    fs::create_dir_all(mcp.parent().unwrap()).unwrap();
    fs::write(&mcp, "{ not valid json").unwrap();
    let prep = CursorPrep {
        root: dir.join("root"),
        project: "x".into(),
        allow: vec!["Shell(backlot)".into()],
        global_mcp: mcp.clone(),
    };
    let err = prepare(&prep).unwrap_err();
    assert!(err.contains("mcp.json"), "{err}");
    assert!(!err.contains("fake-secret"), "{err}");
    assert!(!err.contains("not valid"), "{err}");
}

/// A fake `chats/` tree: `<hash>/<uuid>/meta.json` holding the
/// chat's creation time and cwd.
fn chat(chats: &Path, hash: &str, uuid: &str, created_ms: u64, cwd: &str) {
    let dir = chats.join(hash).join(uuid);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("meta.json"),
        format!("{{\"createdAtMs\":{created_ms},\"cwd\":\"{cwd}\"}}"),
    )
    .unwrap();
}

const U1: &str = "11111111-1111-4111-8111-111111111111";
const U2: &str = "22222222-2222-4222-8222-222222222222";
const U3: &str = "33333333-3333-4333-8333-333333333333";
const CWD: &str = "/home/user/acme-shop";

/// The newest chat created at or after the role's start in its own
/// cwd is the session — an older one, a different cwd and a taken
/// id never are.
#[test]
fn session_at_finds_the_newest_matching_chat() {
    let dir = tmp("chats");
    let chats = dir.join("chats");
    chat(&chats, "hash-a", U1, 100, CWD);
    chat(&chats, "hash-a", U2, 200, CWD);
    let taken = BTreeSet::new();
    assert_eq!(session_at(&chats, CWD, 1, &taken).as_deref(), Some(U2));
    // U2 taken: U1 serves; both taken: nothing.
    let taken = BTreeSet::from([U2.to_string()]);
    assert_eq!(session_at(&chats, CWD, 1, &taken).as_deref(), Some(U1));
    // Created before the role started: never.
    assert_eq!(
        session_at(&chats, CWD, 150, &BTreeSet::from([U2.to_string()])).as_deref(),
        None
    );
    assert_eq!(session_at(&chats, CWD, 300, &BTreeSet::new()), None);
    // A different cwd's chat is not the role's.
    chat(&chats, "hash-b", U3, 400, "/home/user/other-repo");
    assert_eq!(
        session_at(&chats, CWD, 1, &BTreeSet::new()).as_deref(),
        Some(U2)
    );
}

/// Malformed entries — a non-uuid dir, a missing or corrupt
/// meta.json — are skipped, never fatal.
#[test]
fn session_at_skips_the_malformed() {
    let dir = tmp("chats-bad");
    let chats = dir.join("chats");
    fs::create_dir_all(chats.join("hash-a/not-a-uuid")).unwrap();
    chat(&chats, "hash-a", U1, 100, CWD);
    fs::write(chats.join("hash-a").join(U1).join("meta.json"), "not json").unwrap();
    fs::create_dir_all(chats.join("hash-b").join(U2)).unwrap();
    assert_eq!(session_at(&chats, CWD, 1, &BTreeSet::new()), None);
    assert_eq!(
        session_at(&dir.join("gone"), CWD, 1, &BTreeSet::new()),
        None
    );
}

/// The launch's `Prep` carries the paths and rules `prepare` needs:
/// the role root, the cwd's slug, the rule list and the global
/// `mcp.json` path from the stage's HOME.
#[test]
fn plan_carries_its_prep() {
    let env = BTreeMap::from([("HOME".to_string(), "/home/user".to_string())]);
    let ctx = LaunchCtx {
        terminal: "t",
        cwd: "/home/user/acme-shop",
        prompt: None,
        resume: None,
        studio_dir: "/home/user/state/studios/acme-shop",
        stage_env: &env,
        role: "director",
        checks: &[],
    };
    let ident = ActorIdentity {
        runner: "cursor".into(),
        runner_version: None,
        model: "m".into(),
        effort: "high".into(),
        permission_profile: "standard".into(),
    };
    let l = plan(&ident, &ctx).unwrap();
    let Some(Prep::Cursor(p)) = l.prep else {
        panic!("cursor launch carries its prep")
    };
    assert_eq!(
        p.root,
        Path::new("/home/user/state/studios/acme-shop/roles/director/cursor")
    );
    assert_eq!(p.project, "home-user-acme-shop");
    assert_eq!(
        p.allow,
        [
            "Shell(backlot)",
            "Shell(git status)",
            "Shell(git diff)",
            "Shell(git log)",
            "Shell(git show)",
            "Shell(git merge)"
        ]
    );
    assert_eq!(p.global_mcp, Path::new("/home/user/.cursor/mcp.json"));
}
