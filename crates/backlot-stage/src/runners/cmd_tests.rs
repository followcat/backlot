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

use super::cmd::{CmdPrep, allow_rules, mcp_denies, prepare, session_at, settings_content, slug};
use super::{ActorIdentity, BIRTH_SLACK_MS, LaunchCtx, Prep, plan, session_in_argv};
use crate::test_support::TestDir;

fn tmp(tag: &str) -> TestDir {
    TestDir::new(&format!("bl-cmd-{tag}"))
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

/// An acronym before a lowercase word: `AIworks` is `a-iworks` — the
/// acronym's last capital starts the lowercase word, so the split
/// lands before `Iworks`, not after `AI`. The external run
/// harness's ad-hoc `tr` acronym computed `aiworks` and missed
/// the session; the adapter's own function was never wrong.
#[test]
fn slug_splits_an_acronym_boundary() {
    assert_eq!(slug("/home/user/AIworks"), "home-user-a-iworks");
    assert_eq!(slug("/home/user/s12e"), "home-user-s12e");
    assert_eq!(slug("/home/user/acme-shop"), "home-user-acme-shop");
}

/// `Shell(...)` rules per role: the base `Shell(backlot)` plus the
/// shared read-only set for everyone; an actor's `git add`/`git
/// commit`, the director's `git merge`; the declared checks verbatim
/// — and a `run` the rule syntax cannot express is simply never
/// there. `git branch` is only ever its two listing forms.
#[test]
fn rules_per_role() {
    let checks = [
        check("unit", "cargo test --lib"),
        check("lint", "cargo clippy"),
    ];
    let read_only = [
        "Shell(git status:*)",
        "Shell(git log:*)",
        "Shell(git show:*)",
        "Shell(git diff:*)",
        "Shell(git branch --list:*)",
        "Shell(git branch --show-current:*)",
        "Shell(ls:*)",
        "Shell(rg:*)",
        "Shell(wc:*)",
        "Shell(head:*)",
        "Shell(backlot msg show:*)",
    ];
    let director: Vec<String> = std::iter::once("Shell(backlot)".to_string())
        .chain(read_only.iter().map(|r| r.to_string()))
        .chain([
            "Shell(git merge:*)".to_string(),
            "Shell(cargo test --lib:*)".to_string(),
            "Shell(cargo clippy:*)".to_string(),
        ])
        .collect();
    assert_eq!(allow_rules("director", &checks), director);
    let actor: Vec<String> = std::iter::once("Shell(backlot)".to_string())
        .chain(read_only.iter().map(|r| r.to_string()))
        .chain([
            "Shell(git add:*)".to_string(),
            "Shell(git commit:*)".to_string(),
            "Shell(cargo test --lib:*)".to_string(),
            "Shell(cargo clippy:*)".to_string(),
        ])
        .collect();
    assert_eq!(allow_rules("actor-t1-c1", &checks), actor);
    // Producer and writer keep the base rule plus the read-only set.
    for role in ["producer", "writer"] {
        let rules = allow_rules(role, &checks);
        assert_eq!(rules.first().map(String::as_str), Some("Shell(backlot)"));
        assert!(
            read_only.iter().all(|r| rules.iter().any(|x| x == r)),
            "{role} misses a read-only rule: {rules:?}"
        );
        assert!(!rules.iter().any(|r| r.contains("git add")), "{role}");
        assert!(
            !rules.iter().any(|r| r == "Shell(git branch:*)"),
            "{role}: no bare branch family"
        );
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

/// D17a: Hollywood's list carries the read-only help rules, and never
/// the bare `Shell(backlot)` family — a help read is not a licence to
/// run every backlot command.
#[test]
fn hollywood_may_read_its_own_cli() {
    let rules = allow_rules("hollywood", &[]);
    for rule in [
        "Shell(backlot --help:*)",
        "Shell(backlot help:*)",
        "Shell(backlot progress:*)",
    ] {
        assert!(rules.iter().any(|r| r == rule), "{rules:?}");
    }
    assert!(
        !rules.iter().any(|r| r == "Shell(backlot)"),
        "{rules:?}: no command family"
    );
}

/// A settings file that does not exist yet is written with the
/// minimal schema cmd itself accepts: `permissions` only.
#[test]
fn settings_written_fresh() {
    let allow = vec!["Shell(backlot)".to_string()];
    let text = String::from_utf8(settings_content(None, &allow, &[], &[]).unwrap()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        v["permissions"],
        serde_json::json!({
            "allow": ["Shell(backlot)"],
            "deny": [],
            "additionalDirectories": [],
        })
    );
    // Corrupt content is healed, not preserved.
    let healed =
        String::from_utf8(settings_content(Some(b"not json"), &allow, &[], &[]).unwrap()).unwrap();
    assert_eq!(healed, text);
}

/// `permissions.additionalDirectories` is what the tool permission
/// check reads — `--add-dir` only feeds the model's `<workspace>`
/// context — so the stage writes the directory into the file, both
/// fresh and spliced into an existing one.
#[test]
fn settings_carry_additional_directories() {
    let allow = vec!["Shell(backlot)".to_string()];
    let dirs = vec!["/home/user/state/studios/acme-shop".to_string()];
    let fresh = String::from_utf8(settings_content(None, &allow, &[], &dirs).unwrap()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&fresh).unwrap();
    assert_eq!(
        v["permissions"]["additionalDirectories"],
        serde_json::json!(["/home/user/state/studios/acme-shop"])
    );
    // An existing file keeps its other keys and replaces the list.
    let existing = "{\n  \"authInfo\": \"fake-token\",\n  \"permissions\": {\"allow\": [], \"deny\": [], \"additionalDirectories\": [\"/old\"]}\n}\n";
    let spliced =
        String::from_utf8(settings_content(Some(existing.as_bytes()), &allow, &[], &dirs).unwrap())
            .unwrap();
    assert!(
        spliced.contains("\"authInfo\": \"fake-token\""),
        "{spliced}"
    );
    let v: serde_json::Value = serde_json::from_str(&spliced).unwrap();
    assert_eq!(
        v["permissions"]["additionalDirectories"],
        serde_json::json!(["/home/user/state/studios/acme-shop"])
    );
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
        String::from_utf8(settings_content(Some(existing.as_bytes()), &allow, &deny, &[]).unwrap())
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
        settings_content(Some(b"{\"authInfo\": \"fake-token\"}"), &allow, &[], &[]).unwrap(),
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
        additional_dirs: vec!["/home/user/state/studios/acme-shop".into()],
        global_mcp: home.join(".commandcode/mcp.json"),
        signals: dir.join("signals.jsonl"),
        readonly: false,
    };
    assert_eq!(prepare(&prep).unwrap(), None);
    let mode = fs::metadata(&prep.settings).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&prep.settings).unwrap()).unwrap();
    assert_eq!(v["permissions"]["allow"][0], "Shell(backlot)");
    assert_eq!(
        v["permissions"]["deny"],
        serde_json::json!(["mcp__paid-tools"])
    );
    assert_eq!(
        v["permissions"]["additionalDirectories"],
        serde_json::json!(["/home/user/state/studios/acme-shop"])
    );
    assert!(
        v["hooks"]["SessionStart"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .contains("hook --runner cmd --file")
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
        additional_dirs: vec!["/state/studios/acme-shop".into()],
        global_mcp: mcp,
        signals: dir.join("signals.jsonl"),
        readonly: false,
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

/// A file's birth time as `session_at` reads it — birth, or mtime
/// where birth is unsupported.
fn birth_ms(path: &Path) -> u64 {
    let md = fs::metadata(path).unwrap();
    let t = md.created().or_else(|_| md.modified()).unwrap();
    t.duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// A transcript born in the same clock tick as the start record —
/// just before `started_ms` — is still the role's: the kernel's file
/// clock lags `SystemTime::now()` by up to a tick, so the birth can
/// precede the start. One older than the slack is not. The start is
/// derived from the file's own metadata, so the test never sleeps.
#[test]
fn session_at_allows_a_birth_in_the_start_tick() {
    let dir = tmp("sessions-birth");
    let projects = dir.join("projects");
    let slug_dir = slug(CWD);
    transcript(&projects, &slug_dir, U1, CWD);
    let born = birth_ms(&projects.join(&slug_dir).join(format!("{U1}.jsonl")));
    // Born 1 ms before the start: within the slack, still found.
    assert_eq!(
        session_at(&projects, CWD, born + 1, &BTreeSet::new()).as_deref(),
        Some(U1)
    );
    // Born past the slack: too old for this role.
    assert_eq!(
        session_at(&projects, CWD, born + BIRTH_SLACK_MS + 1, &BTreeSet::new()),
        None
    );
}

/// `-r`/`--resume`/`--session` all read back a resumed id.
#[test]
fn argv_reads_back_the_resumed_id() {
    assert_eq!(
        session_in_argv("cmd", &["cmd".into(), "-r".into(), "id-9".into()]).as_deref(),
        Some("id-9")
    );
    assert_eq!(
        session_in_argv("cmd", &["cmd".into(), "--resume".into(), "named".into()]).as_deref(),
        Some("named")
    );
    assert_eq!(
        session_in_argv("cmd", &["cmd".into(), "--session".into(), "ab12".into()]).as_deref(),
        Some("ab12")
    );
    assert_eq!(
        session_in_argv("cmd", &["cmd".into(), "-m".into(), "m".into()]),
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
        answer_trust: true,
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
    assert_eq!(p.additional_dirs, ["/home/user/state/studios/acme-shop"]);
}

/// A studio role's prepared settings name its studio state directory
/// under `permissions.additionalDirectories` — the key the tool
/// permission check reads, the one that stops the outside-workspace
/// prompt. `--add-dir` alone does not.
#[test]
#[cfg(unix)]
fn prepare_for_a_studio_role_writes_the_studio_dir() {
    let dir = tmp("prep-studio");
    let home = dir.join("home");
    fs::create_dir_all(&home).unwrap();
    let cwd = dir.join("work");
    fs::create_dir_all(&cwd).unwrap();
    let cwd = cwd.to_string_lossy().into_owned();
    let studio_dir = "/home/user/state/studios/acme-shop";
    let env = BTreeMap::from([("HOME".to_string(), home.to_string_lossy().into_owned())]);
    let ident = ActorIdentity {
        runner: "cmd".into(),
        runner_version: None,
        model: "m".into(),
        effort: None,
        permission_profile: "standard".into(),
    };
    let ctx = LaunchCtx {
        terminal: "bl-s-actor-t1-c1",
        cwd: &cwd,
        prompt: None,
        resume: None,
        studio_dir,
        stage_env: &env,
        role: "actor-t1-c1",
        checks: &[],
        answer_trust: true,
    };
    let l = plan(&ident, &ctx).unwrap();
    let Some(Prep::Cmd(p)) = l.prep else {
        panic!("cmd launch carries its prep")
    };
    assert_eq!(p.additional_dirs, [studio_dir]);
    prepare(&p).unwrap();
    let v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&p.settings).unwrap()).unwrap();
    assert_eq!(
        v["permissions"]["additionalDirectories"],
        serde_json::json!([studio_dir])
    );
}

/// Hollywood's prepared settings name `<base>/studios`, the sibling
/// of its `<base>/hollywood` cwd — the directory its `--add-dir`
/// passes, so every studio's state files are readable without a
/// prompt.
#[test]
#[cfg(unix)]
fn prepare_for_hollywood_names_the_studios_dir() {
    let dir = tmp("prep-hollywood");
    let home = dir.join("home");
    fs::create_dir_all(&home).unwrap();
    let base = dir.join("state");
    let hdir = base.join("hollywood");
    fs::create_dir_all(&hdir).unwrap();
    let cwd = hdir.to_string_lossy().into_owned();
    let studios = base.join("studios").to_string_lossy().into_owned();
    let env = BTreeMap::from([("HOME".to_string(), home.to_string_lossy().into_owned())]);
    let ident = ActorIdentity {
        runner: "cmd".into(),
        runner_version: None,
        model: "m".into(),
        effort: None,
        permission_profile: "standard".into(),
    };
    let ctx = LaunchCtx {
        terminal: "bl-hollywood",
        cwd: &cwd,
        prompt: None,
        resume: None,
        studio_dir: &cwd,
        stage_env: &env,
        role: "hollywood",
        checks: &[],
        answer_trust: true,
    };
    let l = plan(&ident, &ctx).unwrap();
    let Some(Prep::Cmd(p)) = l.prep else {
        panic!("cmd launch carries its prep")
    };
    assert_eq!(p.additional_dirs, [studios.as_str()]);
    prepare(&p).unwrap();
    let v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&p.settings).unwrap()).unwrap();
    assert_eq!(
        v["permissions"]["additionalDirectories"],
        serde_json::json!([studios])
    );
}

#[test]
#[cfg(unix)]
fn cmd_hooks_splice_preserves_auth_info_and_user_hooks() {
    let dir = tmp("prep-splice");
    let home = dir.join("home");
    fs::create_dir_all(&home).unwrap();
    let cwd = dir.join("work");
    fs::create_dir_all(cwd.join(".commandcode")).unwrap();
    let settings = cwd.join(".commandcode/settings.json");
    let existing = "{\n  \"authInfo\": {\n    \"token\": \"keep-bytes\"\n  },\n  \"hooks\": {\n    \"PreToolUse\": [\n      {\n        \"hooks\": [\n          {\n            \"type\": \"command\",\n            \"command\": \"echo user-pre\"\n          }\n        ]\n      }\n    ]\n  }\n}\n";
    fs::write(&settings, existing).unwrap();
    let prep = CmdPrep {
        settings: settings.clone(),
        allow: vec!["Shell(backlot)".into()],
        additional_dirs: vec!["/home/user/state/studios/acme-shop".into()],
        global_mcp: home.join(".commandcode/mcp.json"),
        signals: dir.join("signals.jsonl"),
        readonly: false,
    };
    assert_eq!(prepare(&prep).unwrap(), None);
    let out = fs::read_to_string(&settings).unwrap();
    assert!(
        out.contains("\"authInfo\": {\n    \"token\": \"keep-bytes\"\n  }"),
        "{out}"
    );
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    let pre = parsed["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre.len(), 2);
    assert_eq!(pre[0]["hooks"][0]["command"], "echo user-pre");
    assert!(
        pre[1]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .contains("hook --runner cmd --file")
    );
    // Re-running prepare replaces backlot's entry instead of duplicating it.
    assert_eq!(prepare(&prep).unwrap(), None);
    let parsed2: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(parsed2["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);
}

#[test]
#[cfg(unix)]
fn cmd_excludes_new_settings_and_skips_tracked() {
    let dir = tmp("prep-git");
    let home = dir.join("home");
    fs::create_dir_all(&home).unwrap();
    let repo = dir.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let run_git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["-c", "user.name=alice", "-c", "user.email=alice@host-a"])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}");
    };
    run_git(&["init", "-b", "main"]);
    fs::write(repo.join("f"), "base\n").unwrap();
    run_git(&["add", "."]);
    run_git(&["commit", "-m", "base"]);

    let settings = repo.join(".commandcode/settings.json");
    let prep = CmdPrep {
        settings: settings.clone(),
        allow: vec!["Shell(backlot)".into()],
        additional_dirs: vec!["/home/user/state/studios/acme-shop".into()],
        global_mcp: home.join(".commandcode/mcp.json"),
        signals: dir.join("signals.jsonl"),
        readonly: false,
    };
    assert_eq!(prepare(&prep).unwrap(), None);
    let exclude = fs::read_to_string(repo.join(".git/info/exclude")).unwrap();
    assert!(
        exclude
            .lines()
            .any(|l| l.trim() == ".commandcode/settings.json"),
        "{exclude}"
    );

    let orig = "{\"team\":true}\n";
    fs::write(&settings, orig).unwrap();
    run_git(&["add", "-f", ".commandcode/settings.json"]);
    run_git(&["commit", "-m", "track cmd settings"]);
    assert_eq!(
        prepare(&prep).unwrap(),
        Some(super::PrepNote::HooksUnavailable(
            ".commandcode/settings.json"
        ))
    );
    assert_eq!(fs::read_to_string(&settings).unwrap(), orig);
}
