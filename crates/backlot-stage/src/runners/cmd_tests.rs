//! The cmd adapter's pieces: cmd's project slug, the per-role
//! `Shell(...)` allow rules, the settings file's byte-exact
//! `permissions` splice, `prepare`'s file writes and the
//! `projects/<slug>/` session lookup. A fake `authInfo`-style key
//! proves the splice preserves what it never reads — and no error
//! ever carries a byte of it.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use backlot_model::event::CheckDecl;

use super::cmd::{
    CmdPrep, allow_rules, mcp_denies, prepare, session_at, session_in_argv, settings_content, slug,
};
use super::{ActorIdentity, LaunchCtx, Prep, plan};

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bl-cmd-{tag}-{}", std::process::id()));
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

/// The slug cmd computes for a workdir: lowercase, camel-boundary
/// splits (`AControl` → `a-control`), non-alphanumeric runs
/// collapsed to one `-`, ends trimmed, empty → `root`.
#[test]
fn slug_is_command_codes() {
    assert_eq!(slug("/home/user/acme-shop"), "home-user-acme-shop");
    assert_eq!(slug("/home/user/AControl/x"), "home-user-a-control-x");
    assert_eq!(slug("/home/user/xControl"), "home-user-x-control");
    assert_eq!(slug("/home/user/a__b//c"), "home-user-a-b-c");
    assert_eq!(slug("/---"), "root");
    assert_eq!(
        slug("/home/user/with space/x.y"),
        "home-user-with-space-x-y"
    );
}

/// `Shell(...)` rules per role: the base `Shell(backlot)`; an
/// actor's six git commands; the director's read-only set and
/// `git merge`, never `git branch`; the declared checks verbatim —
/// and a `run` the rule syntax cannot express is simply never
/// there.
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
            "Shell(git status:*)",
            "Shell(git diff:*)",
            "Shell(git log:*)",
            "Shell(git show:*)",
            "Shell(git merge:*)",
            "Shell(cargo test --lib:*)",
            "Shell(cargo clippy:*)"
        ]
    );
    assert_eq!(
        allow_rules("actor-t1-c1", &checks),
        [
            "Shell(backlot)",
            "Shell(git add:*)",
            "Shell(git commit:*)",
            "Shell(git status:*)",
            "Shell(git diff:*)",
            "Shell(git log:*)",
            "Shell(git show:*)",
            "Shell(cargo test --lib:*)",
            "Shell(cargo clippy:*)"
        ]
    );
    // Producer and writer keep the base rule only.
    for role in ["producer", "writer"] {
        assert_eq!(allow_rules(role, &checks), ["Shell(backlot)"], "{role}");
    }
    // `(` `)` `*?[]{}`, `\`, `:` and `,` are rule syntax, glob or
    // the rule's own separators — none may reach the file.
    for bad in [
        "make (all)",
        "cargo test -- -p*",
        "cargo test?",
        "ls [ab]",
        "echo {x}",
        "echo a\\b",
        "echo a:b",
        "echo a,b",
    ] {
        let rules = allow_rules("director", &[check("odd", bad)]);
        assert!(!rules.iter().any(|r| r.contains(bad)), "{bad}: {rules:?}");
    }
    for ok in ["echo a-b", "make test-fast"] {
        let rules = allow_rules("director", &[check("ok", ok)]);
        assert!(rules.iter().any(|r| r.contains(ok)), "{ok}: {rules:?}");
    }
    // Nothing broad ever lands — no command-family rule and no
    // glob inside the spec (the `:*` any-args suffix is the prefix
    // form itself, not a widening glob).
    for role in ["director", "actor-t1-c1", "producer"] {
        for rule in allow_rules(role, &checks) {
            assert_ne!(rule, "Shell(git:*)", "{role}");
            let spec = rule.strip_suffix(":*)").unwrap_or(rule.as_str());
            assert!(!spec.contains('*'), "{role}: {rule}");
        }
    }
}

/// A settings file that does not exist yet is written with the
/// minimal schema cmd itself accepts: `permissions` only.
#[test]
fn settings_written_fresh() {
    let allow = vec!["Shell(backlot)".to_string()];
    let text = String::from_utf8(settings_content(None, &allow, &[]).unwrap()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        v["permissions"],
        serde_json::json!({"allow": ["Shell(backlot)"], "deny": []})
    );
    // Corrupt content is healed, not preserved.
    let healed =
        String::from_utf8(settings_content(Some(b"not json"), &allow, &[]).unwrap()).unwrap();
    assert_eq!(healed, text);
}

/// An existing settings file keeps every other byte — whitespace
/// and key order included — while `permissions` is replaced. The
/// fake `authInfo` member survives untouched and is never parsed.
#[test]
fn settings_splice_preserves_unrelated_bytes() {
    let existing = "{\n  \"authInfo\": {\"k\": \"fake-token\"},\n  \"taste\": {\"x\": 1},\n  \"permissions\": {\"allow\": [\"Shell(old)\"], \"deny\": []}\n}\n";
    let allow = vec!["Shell(backlot)".to_string()];
    let deny = vec!["mcp__paid-tools".to_string()];
    let out =
        String::from_utf8(settings_content(Some(existing.as_bytes()), &allow, &deny).unwrap())
            .unwrap();
    assert!(
        out.contains("\"authInfo\": {\"k\": \"fake-token\"}"),
        "{out}"
    );
    assert!(out.contains("\"taste\": {\"x\": 1}"), "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["permissions"]["allow"][0], "Shell(backlot)");
    assert_eq!(v["permissions"]["deny"][0], "mcp__paid-tools");
    // No `permissions` key yet — the member is appended.
    let added = String::from_utf8(
        settings_content(Some(b"{\"authInfo\": \"fake-token\"}"), &allow, &[]).unwrap(),
    )
    .unwrap();
    assert!(added.contains("\"authInfo\": \"fake-token\""), "{added}");
    let v: serde_json::Value = serde_json::from_str(&added).unwrap();
    assert_eq!(v["permissions"]["allow"][0], "Shell(backlot)");
}

/// The global `mcp.json` yields its server NAMES as `mcp__<name>`
/// deny rules — sorted; a missing file denies nothing; a malformed
/// one is an error naming the path only, never the content.
#[test]
fn mcp_denies_names_the_servers() {
    let dir = tmp("mcp");
    let mcp = dir.join("mcp.json");
    fs::write(
        &mcp,
        r#"{"mcpServers":{"web-tools":{"secret":"fake-token"},"other":{"y":2}}}"#,
    )
    .unwrap();
    assert_eq!(mcp_denies(&mcp).unwrap(), ["mcp__other", "mcp__web-tools"]);
    assert!(mcp_denies(&dir.join("gone.json")).unwrap().is_empty());
    fs::write(&mcp, "{ not valid").unwrap();
    let err = mcp_denies(&mcp).unwrap_err();
    assert!(err.contains("mcp.json"), "{err}");
    assert!(!err.contains("not valid"), "{err}");
}

/// `prepare` writes `<cwd>/.commandcode/settings.json` mode 600
/// with the role's allow rules and the MCP deny mask — and an
/// empty `global_mcp` (no HOME in the stage env) is an error, not a
/// silent skip.
#[test]
#[cfg(unix)]
fn prepare_writes_the_settings_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmp("prep");
    let home = dir.join("home");
    fs::create_dir_all(home.join(".commandcode")).unwrap();
    fs::write(
        home.join(".commandcode/mcp.json"),
        r#"{"mcpServers":{"paid-tools":{"x":1}}}"#,
    )
    .unwrap();
    let cwd = dir.join("work");
    fs::create_dir_all(&cwd).unwrap();
    let prep = CmdPrep {
        settings: cwd.join(".commandcode/settings.json"),
        allow: allow_rules("director", &[check("unit", "cargo test")]),
        global_mcp: home.join(".commandcode/mcp.json"),
    };
    assert!(!prepare(&prep).unwrap());
    let mode = fs::metadata(&prep.settings).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&prep.settings).unwrap()).unwrap();
    assert_eq!(v["permissions"]["allow"][0], "Shell(backlot)");
    assert_eq!(
        v["permissions"]["deny"],
        serde_json::json!(["mcp__paid-tools"])
    );
    // An empty HOME lookup is an error, not a skipped mask.
    let prep = CmdPrep {
        global_mcp: PathBuf::new(),
        ..prep
    };
    let err = prepare(&prep).unwrap_err();
    assert!(err.contains("HOME"), "{err}");
}

/// `prepare` errors carry a path and the cause only — a fake
/// `authInfo` in the existing file or raw mcp bytes never reach
/// the error string.
#[test]
#[cfg(unix)]
fn prepare_errors_never_carry_content() {
    let dir = tmp("prep-err");
    let cwd = dir.join("work");
    fs::create_dir_all(cwd.join(".commandcode")).unwrap();
    fs::write(
        cwd.join(".commandcode/settings.json"),
        "{\"authInfo\": \"fake-secret\"}",
    )
    .unwrap();
    let mcp = dir.join("home/.commandcode/mcp.json");
    fs::create_dir_all(mcp.parent().unwrap()).unwrap();
    fs::write(&mcp, "{ not valid json").unwrap();
    let prep = CmdPrep {
        settings: cwd.join(".commandcode/settings.json"),
        allow: vec!["Shell(backlot)".into()],
        global_mcp: mcp,
    };
    let err = prepare(&prep).unwrap_err();
    assert!(err.contains("mcp.json"), "{err}");
    assert!(!err.contains("fake-secret"), "{err}");
    assert!(!err.contains("not valid"), "{err}");
}

const U1: &str = "11111111-1111-4111-8111-111111111111";
const U2: &str = "22222222-2222-4222-8222-222222222222";
const U3: &str = "33333333-3333-4333-8333-333333333333";
const CWD: &str = "/home/user/acme-shop";

/// A fake cmd transcript: `<slug>/<uuid>.jsonl` whose first line is
/// the session record.
fn transcript(projects: &Path, slug_dir: &str, uuid: &str, cwd: &str) {
    let dir = projects.join(slug_dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join(format!("{uuid}.jsonl")),
        format!("{{\"type\":\"session\",\"version\":3,\"id\":\"{uuid}\",\"cwd\":\"{cwd}\"}}\n"),
    )
    .unwrap();
}

/// The newest transcript created at or after the role's start in
/// its own cwd is the session — an older one, a different cwd, a
/// `.checkpoints` companion and a taken id never are.
#[test]
fn session_at_finds_the_newest_matching_transcript() {
    let dir = tmp("sessions");
    let projects = dir.join("projects");
    let slug_dir = slug(CWD);
    transcript(&projects, &slug_dir, U1, CWD);
    transcript(&projects, &slug_dir, U2, CWD);
    let taken = BTreeSet::new();
    // Both fresh files qualify — the later creation wins.
    let found = session_at(&projects, CWD, 1, &taken);
    assert!(found.is_some());
    // A taken newer id falls back to the older.
    let taken = BTreeSet::from([U2.to_string(), U1.to_string()]);
    assert_eq!(session_at(&projects, CWD, 1, &taken), None);
    // A transcript in another cwd — or under a different slug —
    // is never the role's.
    transcript(
        &projects,
        "home-user-other-repo",
        U3,
        "/home/user/other-repo",
    );
    assert_eq!(
        session_at(&projects, "/home/user/other-repo", 1, &BTreeSet::new()).as_deref(),
        Some(U3)
    );
    assert!(session_at(&projects, CWD, 1, &BTreeSet::from([U3.to_string()])).is_some());
}

/// Malformed entries — a missing projects dir, a corrupt first
/// line, a `.checkpoints.jsonl` companion — are skipped, never
/// fatal.
#[test]
fn session_at_skips_the_malformed() {
    let dir = tmp("sessions-bad");
    let projects = dir.join("projects");
    let slug_dir = slug(CWD);
    let pdir = projects.join(&slug_dir);
    fs::create_dir_all(&pdir).unwrap();
    fs::write(pdir.join("deadbeef.checkpoints.jsonl"), "x\n").unwrap();
    fs::write(pdir.join("deadbeef.jsonl"), "not json\n").unwrap();
    fs::write(pdir.join("config.json"), "{}").unwrap();
    assert_eq!(session_at(&projects, CWD, 1, &BTreeSet::new()), None);
    assert_eq!(
        session_at(&dir.join("gone"), CWD, 1, &BTreeSet::new()),
        None
    );
}

/// `-r`/`--resume`/`--session` all read back a resumed id.
#[test]
fn argv_reads_back_the_resumed_id() {
    assert_eq!(
        session_in_argv(&["cmd".into(), "-r".into(), "id-9".into()]).as_deref(),
        Some("id-9")
    );
    assert_eq!(
        session_in_argv(&["cmd".into(), "--resume".into(), "named".into()]).as_deref(),
        Some("named")
    );
    assert_eq!(
        session_in_argv(&["cmd".into(), "--session".into(), "ab12".into()]).as_deref(),
        Some("ab12")
    );
    assert_eq!(
        session_in_argv(&["cmd".into(), "-m".into(), "m".into()]),
        None
    );
}

/// The launch's `Prep` carries the paths and rules `prepare`
/// needs: the workdir's settings file, the rule list and the
/// global `mcp.json` path from the stage's HOME.
#[test]
fn plan_carries_its_prep() {
    let env = BTreeMap::from([("HOME".to_string(), "/home/user".to_string())]);
    let ctx = LaunchCtx {
        terminal: "bl-s-actor-t1-c1",
        cwd: "/home/user/acme-shop",
        prompt: None,
        resume: None,
        studio_dir: "/home/user/state/studios/acme-shop",
        stage_env: &env,
        role: "actor-t1-c1",
        checks: &[check("unit", "cargo test")],
    };
    let ident = ActorIdentity {
        runner: "cmd".into(),
        runner_version: None,
        model: "m".into(),
        effort: Some("high".into()),
        permission_profile: "standard".into(),
    };
    let l = plan(&ident, &ctx).unwrap();
    let Some(Prep::Cmd(p)) = l.prep else {
        panic!("cmd launch carries its prep")
    };
    assert_eq!(
        p.settings,
        Path::new("/home/user/acme-shop/.commandcode/settings.json")
    );
    assert!(p.allow.contains(&"Shell(git add:*)".to_string()));
    assert_eq!(p.global_mcp, Path::new("/home/user/.commandcode/mcp.json"));
}
