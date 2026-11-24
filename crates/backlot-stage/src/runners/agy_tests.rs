//! The agy adapter's pieces: the launch argv (`--mode`,
//! `--effort`, `--model`, `--settings`, `-i` fresh only) and the
//! per-session settings file's `toolPermission` splice. The session
//! lookup lives in `agy_session_tests`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::agy::{AgyPrep, prepare, settings_content};
use super::{ActorIdentity, LaunchCtx, Prep, plan, session_in_argv};
use crate::test_support::TestDir;

fn tmp(tag: &str) -> TestDir {
    TestDir::new(&format!("bl-agy-{tag}"))
}

fn ctx<'a>(env: &'a BTreeMap<String, String>, cwd: &'a str) -> LaunchCtx<'a> {
    LaunchCtx {
        terminal: "bl-s-actor-t1-c1",
        cwd,
        prompt: None,
        resume: None,
        studio_dir: "/home/user/state/studios/acme-shop",
        stage_env: env,
        role: "actor-t1-c1",
        checks: &[],
    }
}

fn ident(effort: Option<&str>) -> ActorIdentity {
    ActorIdentity {
        runner: "agy".into(),
        runner_version: None,
        model: "gemini-4-neon-high".into(),
        effort: effort.map(str::to_string),
        permission_profile: "standard".into(),
    }
}

/// A fresh launch's argv: `--mode accept-edits` for either
/// profile, `--effort`/`--model` from the identity, `--settings`
/// the workdir file, and `-i` carrying the prompt. The plan's
/// prep names the same file and the session is post-launch.
#[test]
fn plan_shapes_the_fresh_launch() {
    let env = BTreeMap::new();
    let mut c = ctx(&env, "/home/user/acme-shop");
    c.prompt = Some("do the thing");
    for profile in ["standard", "worktree"] {
        let l = plan(
            &ActorIdentity {
                permission_profile: profile.into(),
                ..ident(Some("high"))
            },
            &c,
        )
        .unwrap();
        assert_eq!(
            l.argv,
            [
                "agy",
                "--mode",
                "accept-edits",
                "--effort",
                "high",
                "--model",
                "gemini-4-neon-high",
                "--settings",
                "/home/user/acme-shop/.agy-settings.json",
                "-i",
                "do the thing",
            ],
            "{profile}"
        );
        assert!(matches!(l.session, super::Session::PostLaunch));
        let Some(Prep::Agy(p)) = l.prep else {
            panic!("agy launch carries its prep")
        };
        assert_eq!(
            p.settings,
            Path::new("/home/user/acme-shop/.agy-settings.json")
        );
    }
}

/// A resume swaps `-i` for `--conversation <id>` and carries no
/// prompt — the stage sends it after the composer opens, devin's
/// rule. The settings file still rides: prep runs on resumes too.
#[test]
fn plan_shapes_the_resume() {
    let env = BTreeMap::new();
    let mut c = ctx(&env, "/home/user/acme-shop");
    c.resume = Some("0373977e-3d0c-4628-9218-13ba9695cd55");
    c.prompt = Some("ignored");
    let l = plan(&ident(Some("max")), &c).unwrap();
    assert_eq!(
        l.argv,
        [
            "agy",
            "--conversation",
            "0373977e-3d0c-4628-9218-13ba9695cd55",
            "--mode",
            "accept-edits",
            "--effort",
            "max",
            "--model",
            "gemini-4-neon-high",
            "--settings",
            "/home/user/acme-shop/.agy-settings.json",
        ]
    );
    assert!(l.prep.is_some());
}

/// An unknown permission profile is the plan refusal; an identity
/// with no effort still plans — the cast rule requires one, so a
/// missing value only means a manually-written cast.
#[test]
fn plan_refuses_a_bad_profile_and_keeps_going_without_effort() {
    let env = BTreeMap::new();
    let c = ctx(&env, "/home/user/acme-shop");
    let err = plan(
        &ActorIdentity {
            permission_profile: "yolo".into(),
            ..ident(Some("low"))
        },
        &c,
    )
    .unwrap_err();
    assert_eq!(err.reason, super::PROFILE_NOT_SUPPORTED);
    let l = plan(&ident(None), &c).unwrap();
    assert!(l.argv.windows(2).any(|w| w == ["--effort", ""]));
}

/// The settings file's bytes: `toolPermission = "always-proceed"`
/// and `permissions.deny` fresh, or spliced byte-exact into an
/// existing object — every other member keeps its whitespace and order.
#[test]
fn settings_splices_tool_permission_only() {
    let expected_deny = [
        "command(git push)",
        "command(gh pr)",
        "command(gh release)",
        "command(gh repo)",
        "command(gh issue)",
        "command(gh api)",
        "command(hc update)",
        "command(hc machine)",
        "command(hc server stop)",
        "command(scp)",
        "command(rsync)",
        "command(ssh)",
        "command(npm publish)",
        "command(cargo publish)",
    ];
    let check_rules = |bytes: &[u8]| {
        let v: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        assert_eq!(v["toolPermission"], "always-proceed");
        let deny: Vec<&str> = v["permissions"]["deny"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap())
            .collect();
        assert_eq!(deny, expected_deny);
        for rule in deny {
            assert!(!rule.contains(" *"), "no trailing wildcard in {rule}");
        }
    };

    let fresh = settings_content(None).unwrap();
    assert!(fresh.ends_with(b"\n"));
    check_rules(&fresh);

    // An existing object keeps its other members and bytes verbatim.
    let existing = b"{\n  \"model\": \"gemini-4-neon\",\n  \"toolPermission\": \"prompt\",\n  \"verbosity\": \"low\"\n}";
    let spliced = settings_content(Some(existing)).unwrap();
    let text = String::from_utf8(spliced.clone()).unwrap();
    assert!(text.contains("\"model\": \"gemini-4-neon\""), "{text}");
    assert!(
        text.contains("\"toolPermission\": \"always-proceed\""),
        "{text}"
    );
    assert!(!text.contains("\"prompt\""), "{text}");
    assert!(text.contains("\"verbosity\": \"low\""), "{text}");
    check_rules(&spliced);

    // Existing permissions.allow survives alongside injected deny.
    let with_allow = b"{\n  \"model\": \"gemini-4-neon\",\n  \"permissions\": {\"allow\": [\"command(echo)\"]}\n}";
    let spliced_allow = settings_content(Some(with_allow)).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&spliced_allow).unwrap();
    assert_eq!(
        v["permissions"]["allow"],
        serde_json::json!(["command(echo)"])
    );
    check_rules(&spliced_allow);

    // A non-object file is healed, not preserved.
    let healed = settings_content(Some(b"not json\n")).unwrap();
    assert_eq!(healed, fresh);
}

/// `prepare` writes `<cwd>/.agy-settings.json` mode 600 before the
/// launch — agy reads it once at startup, so a failure is an
/// attention, never a half-allowed role.
#[test]
fn prepare_writes_the_session_settings() {
    let dir = tmp("prep");
    let settings = dir.join("work/.agy-settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    let p = AgyPrep {
        settings: settings.clone(),
        hooks: dir.join("work/.agents/hooks.json"),
        signals: dir.join("signals.jsonl"),
    };
    assert_eq!(prepare(&p).unwrap(), None);
    let text = fs::read_to_string(&settings).unwrap();
    assert!(text.contains("always-proceed"), "{text}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&settings).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=alice", "-c", "user.email=alice@host-a"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn hooks_file_splices_byte_exact_excludes_worktree_and_skips_tracked() {
    let dir = tmp("hooks-git");
    let repo = dir.join("repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    fs::write(repo.join("f"), "base\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "base"]);

    let wt = dir.join("wt");
    git(
        &repo,
        &["worktree", "add", "-b", "wt", wt.to_str().unwrap()],
    );
    let hooks = wt.join(".agents/hooks.json");
    let sig = dir.join("signals.jsonl");
    let prep = AgyPrep {
        settings: wt.join(".agy-settings.json"),
        hooks: hooks.clone(),
        signals: sig.clone(),
    };
    assert_eq!(prepare(&prep).unwrap(), None);
    // Exclude lands in the main repo's info/exclude (via commondir) so git status stays clean.
    let exclude = fs::read_to_string(repo.join(".git/info/exclude")).unwrap();
    assert!(
        exclude.lines().any(|l| l.trim() == ".agents/hooks.json"),
        "{exclude}"
    );
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(&wt)
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    let status_out = String::from_utf8_lossy(&status.stdout);
    assert!(!status_out.contains(".agents/hooks.json"), "{status_out}");

    // Existing custom named hooks stay byte-for-byte identical when backlot splices its key.
    let custom = "{\n  \"userHook\": {\n    \"PreToolUse\": []\n  }\n}\n";
    fs::write(&hooks, custom).unwrap();
    assert_eq!(prepare(&prep).unwrap(), None);
    let spliced = fs::read_to_string(&hooks).unwrap();
    assert!(
        spliced.starts_with("{\n  \"userHook\": {\n    \"PreToolUse\": []\n  }"),
        "{spliced}"
    );
    let parsed: serde_json::Value = serde_json::from_str(&spliced).unwrap();
    assert_eq!(parsed["backlot"]["PreToolUse"][0]["matcher"], "");
    let pre_cmd = parsed["backlot"]["PreToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(pre_cmd.contains("hook --runner agy --file"), "{pre_cmd}");
    assert!(
        pre_cmd.ends_with(r#" || printf '{"decision":"allow"}\n'"#),
        "{pre_cmd}"
    );
    for ev in ["SessionStart", "PreInvocation", "PostInvocation", "Stop"] {
        let ev_cmd = parsed["backlot"][ev][0]["command"].as_str().unwrap();
        assert!(!ev_cmd.contains("printf"), "{ev}: {ev_cmd}");
    }
    let post_cmd = parsed["backlot"]["PostToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(!post_cmd.contains("printf"), "{post_cmd}");

    // Tracked .agents/hooks.json is never touched and reports HooksUnavailable.
    let tracked_repo = dir.join("tracked");
    fs::create_dir_all(tracked_repo.join(".agents")).unwrap();
    git(&tracked_repo, &["init", "-b", "main"]);
    let tracked_hooks = tracked_repo.join(".agents/hooks.json");
    let orig = "{\"teamHook\":{}}\n";
    fs::write(&tracked_hooks, orig).unwrap();
    git(&tracked_repo, &["add", ".agents/hooks.json"]);
    git(&tracked_repo, &["commit", "-m", "track hooks"]);
    let tprep = AgyPrep {
        settings: tracked_repo.join(".agy-settings.json"),
        hooks: tracked_hooks.clone(),
        signals: sig,
    };
    assert_eq!(
        prepare(&tprep).unwrap(),
        Some(super::PrepNote::HooksUnavailable(".agents/hooks.json"))
    );
    assert_eq!(fs::read_to_string(&tracked_hooks).unwrap(), orig);
}

/// The resumed terminal's id sits after `--conversation`; `-c` is
/// a boolean (continue the most recent) and carries none.
#[test]
fn the_resumed_argv_names_its_session() {
    let argv = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        session_in_argv("agy", &argv(&["agy", "--conversation", "abc-1", "-i", "p"])),
        Some("abc-1".to_string())
    );
    assert_eq!(session_in_argv("agy", &argv(&["agy", "-c"])), None);
    assert_eq!(session_in_argv("agy", &argv(&["agy", "--continue"])), None);
}

#[test]
#[cfg(unix)]
fn pre_tool_use_missing_binary_falls_back_to_allow_under_sh() {
    let dir = tmp("missing-bin-fallback");
    let sig = dir.join("signals.jsonl");
    let bytes = super::hook_files::agy_hooks_content(None, &sig);
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let generated = parsed["backlot"]["PreToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    let idx = generated.find(" hook --runner agy ").unwrap();
    let with_missing_bin = format!("'/nonexistent/backlot-missing'{}", &generated[idx..]);

    let out = std::process::Command::new("sh")
        .arg("-c")
        .arg(&with_missing_bin)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "status={:?} stderr={}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout, "{\"decision\":\"allow\"}\n");
}
