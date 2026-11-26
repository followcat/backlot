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
        answer_trust: true,
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
        "command(gh release)",
        "command(gh repo)",
        "command(gh issue)",
        "command(gh api)",
        "command(hc update)",
        "command(hc machine)",
        "command(hc server stop)",
        "command(hc run)",
        "command(hc exec)",
        "command(hc send)",
        "command(hc keys)",
        "command(hc connect)",
        "command(systemd-run)",
        "command(tmux new -d)",
        "command(tmux new-session -d)",
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

    let fresh = settings_content(None, Some("/home/user/acme-shop")).unwrap();
    assert!(fresh.ends_with(b"\n"));
    check_rules(&fresh);
    let fresh_v: serde_json::Value = serde_json::from_slice(&fresh).unwrap();
    assert_eq!(
        fresh_v["trustedWorkspaces"],
        serde_json::json!(["/home/user/acme-shop"])
    );

    // An existing object keeps its other members and bytes verbatim.
    let existing = b"{\n  \"model\": \"gemini-4-neon\",\n  \"toolPermission\": \"prompt\",\n  \"verbosity\": \"low\"\n}";
    let spliced = settings_content(Some(existing), Some("/home/user/acme-shop")).unwrap();
    let text = String::from_utf8(spliced.clone()).unwrap();
    assert!(text.contains("\"model\": \"gemini-4-neon\""), "{text}");
    assert!(
        text.contains("\"toolPermission\": \"always-proceed\""),
        "{text}"
    );
    assert!(!text.contains("\"prompt\""), "{text}");
    assert!(text.contains("\"verbosity\": \"low\""), "{text}");
    check_rules(&spliced);
    let spliced_v: serde_json::Value = serde_json::from_slice(&spliced).unwrap();
    assert_eq!(
        spliced_v["trustedWorkspaces"],
        serde_json::json!(["/home/user/acme-shop"])
    );

    // Existing permissions.allow survives alongside injected deny.
    let with_allow = b"{\n  \"model\": \"gemini-4-neon\",\n  \"permissions\": {\"allow\": [\"command(echo)\"]}\n}";
    let spliced_allow = settings_content(Some(with_allow), Some("/home/user/acme-shop")).unwrap();
    let v: serde_json::Value = serde_json::from_slice(&spliced_allow).unwrap();
    assert_eq!(
        v["permissions"]["allow"],
        serde_json::json!(["command(echo)"])
    );
    assert_eq!(
        v["trustedWorkspaces"],
        serde_json::json!(["/home/user/acme-shop"])
    );
    check_rules(&spliced_allow);

    // A non-object file is healed, not preserved.
    let healed = settings_content(Some(b"not json\n"), Some("/home/user/acme-shop")).unwrap();
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
        trusted_workspace: Some(dir.join("work")),
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
        trusted_workspace: Some(wt.clone()),
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
        trusted_workspace: Some(tracked_repo.clone()),
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

#[test]
fn fresh_and_resume_launches_trust_the_work_area() {
    let dir = tmp("trust-fresh-resume");
    let work = dir.join("work");
    fs::create_dir_all(&work).unwrap();
    let work_str = work.to_string_lossy().into_owned();
    let canon = fs::canonicalize(&work)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let env = BTreeMap::new();

    for resume in [None, Some("0373977e-3d0c-4628-9218-13ba9695cd55")] {
        let mut c = ctx(&env, &work_str);
        c.resume = resume;
        if resume.is_none() {
            c.prompt = Some("do the thing");
        }
        let l = plan(&ident(Some("high")), &c).unwrap();
        let Some(Prep::Agy(p)) = l.prep else {
            panic!("agy launch carries its prep");
        };
        let _ = fs::remove_file(&p.settings);
        assert_eq!(prepare(&p).unwrap(), None);
        let v: serde_json::Value = serde_json::from_slice(&fs::read(&p.settings).unwrap()).unwrap();
        assert_eq!(
            v["trustedWorkspaces"],
            serde_json::json!([canon]),
            "resume={resume:?}: {v}"
        );
    }
}

#[test]
fn prepare_keeps_existing_trusted_workspaces_and_avoids_duplicating_cwd() {
    let dir = tmp("trust-existing");
    let work = dir.join("work");
    fs::create_dir_all(&work).unwrap();
    let work_str = work.to_string_lossy().into_owned();
    let canon = fs::canonicalize(&work)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let env = BTreeMap::new();

    let l = plan(&ident(Some("high")), &ctx(&env, &work_str)).unwrap();
    let Some(Prep::Agy(p)) = l.prep else {
        panic!("agy launch carries its prep");
    };
    fs::write(
        &p.settings,
        "{\n  \"model\": \"gemini-4-neon\",\n  \"trustedWorkspaces\": [\"/home/user/other\"]\n}\n",
    )
    .unwrap();
    assert_eq!(prepare(&p).unwrap(), None);
    let text = fs::read_to_string(&p.settings).unwrap();
    assert!(text.contains("\"model\": \"gemini-4-neon\""), "{text}");
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        v["trustedWorkspaces"],
        serde_json::json!(["/home/user/other", canon])
    );

    // A second prepare (e.g. resume) keeps existing entries and never duplicates cwd.
    assert_eq!(prepare(&p).unwrap(), None);
    let v2: serde_json::Value = serde_json::from_slice(&fs::read(&p.settings).unwrap()).unwrap();
    assert_eq!(
        v2["trustedWorkspaces"],
        serde_json::json!(["/home/user/other", canon])
    );
}

async fn fake_stage_client() -> hc_proto::client::Client {
    use hc_proto::client::Client;
    use hc_proto::codec::Reader;
    use hc_proto::frame::{Frame, Kind};
    use serde_json::{Value, json};
    use tokio::io::AsyncWriteExt;

    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        let (read, mut write) = tokio::io::split(core_side);
        let mut reader = Reader::new(read);
        let _ = reader.read_line().await.unwrap().unwrap();
        let hello = serde_json::to_string(&json!({
            "jsonrpc": "2.0", "id": 0,
            "result": {
                "generation": 1, "version": "fake-core 0.0",
                "framed": true,
                "core": {"id": "core-fake-1", "name": "fake-core"},
                "methods": ["terminal.run", "terminal.kill", "terminal.rm", "terminal.read", "terminal.list"],
                "features": [],
            }
        }))
        .unwrap();
        write.write_all(hello.as_bytes()).await.unwrap();
        write.write_all(b"\n").await.unwrap();
        let mut next = 0u32;
        while let Ok(Some(frame)) = reader.read_frame().await {
            if frame.channel != 0 || frame.kind != Kind::Data {
                continue;
            }
            let req: Value = serde_json::from_slice(&frame.payload).unwrap();
            let id = req["id"].clone();
            let result = match req["method"].as_str().unwrap_or("") {
                "terminal.run" => {
                    next += 1;
                    json!({
                        "address": format!("t-{next}"),
                        "name": req["params"]["name"],
                        "labels": req["params"]["labels"],
                        "argv": req["params"]["argv"],
                        "cwd": req["params"]["cwd"],
                        "state": "live",
                        "created_ms": 0,
                        "screen": "",
                        "cols": 200, "rows": 120,
                    })
                }
                _ => json!({}),
            };
            let reply = serde_json::to_vec(&json!({
                "jsonrpc": "2.0", "id": id, "result": result,
            }))
            .unwrap();
            write
                .write_all(&Frame::data(0, reply).encode())
                .await
                .unwrap();
        }
    });
    let (r, w) = tokio::io::split(client_side);
    Client::open(Box::pin(r), Box::pin(w), None).await.unwrap()
}

#[tokio::test]
async fn stage_hollywood_and_handoff_agy_launches_trust_work_area() {
    use backlot_model::hollywood::HOLLYWOOD_TERMINAL;
    use serde_json::Value;

    let dir = tmp("stage-hw-trust");
    let cfg_home = dir.join("cfg");
    fs::create_dir_all(cfg_home.join("backlot")).unwrap();
    fs::write(
        cfg_home.join("backlot/config.toml"),
        "[hollywood]\nrunner = \"agy\"\nmodel = \"gemini-4-neon-high\"\neffort = \"high\"\n",
    )
    .unwrap();

    let client = fake_stage_client().await;

    let mut stage = crate::daemon::Stage::for_test(client);
    stage.base = dir.path().to_path_buf();
    stage.state_dir = dir.path().to_string_lossy().into_owned();
    stage.env_vars.insert(
        "XDG_CONFIG_HOME".into(),
        cfg_home.to_string_lossy().into_owned(),
    );

    let hdir = crate::store::hollywood_dir(dir.path());
    let mut slot = true;
    stage.hollywood(&[], &[], &mut slot, 1000).await.unwrap();

    let settings_path = hdir.join(".agy-settings.json");
    let canon_hdir = fs::canonicalize(&hdir)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let v: Value = serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap();
    assert_eq!(
        v["trustedWorkspaces"],
        serde_json::json!([canon_hdir]),
        "hollywood: {v}"
    );

    // Handoff candidate launch also writes trustedWorkspaces.
    fs::remove_file(&settings_path).unwrap();
    crate::handoff::stage_handoff_request(
        &hdir,
        "agy",
        "gemini-4-neon-high",
        Some("high"),
        20_000,
        false,
        "test",
        None,
        2000,
    )
    .unwrap();
    let live = vec![crate::reconcile::LiveTerminal {
        address: "t-1".into(),
        name: HOLLYWOOD_TERMINAL.into(),
        labels: BTreeMap::from([("backlot.role".into(), "hollywood".into())]),
        cwd: canon_hdir.clone(),
        ..Default::default()
    }];
    let mut slot = true;
    stage.hollywood(&live, &[], &mut slot, 2100).await.unwrap();
    let v_handoff: Value = serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap();
    assert_eq!(
        v_handoff["trustedWorkspaces"],
        serde_json::json!([canon_hdir]),
        "handoff: {v_handoff}"
    );

    // answer_trust = false leaves Hollywood's settings without trustedWorkspaces.
    fs::remove_file(&settings_path).unwrap();
    stage.stage_cfg.set(crate::config::StageCfg {
        answer_trust: false,
    });
    let mut slot = true;
    crate::handoff::request_cancel(&hdir).unwrap();
    stage.hollywood(&live, &[], &mut slot, 2200).await.unwrap();
    let v_off: Value = serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap();
    assert!(
        v_off.get("trustedWorkspaces").is_none(),
        "answer_trust = false must omit trustedWorkspaces: {v_off}"
    );
}

#[test]
fn settings_splices_trusted_workspaces_and_respects_answer_trust() {
    let cwd = "/home/user/acme-shop";

    // Fresh launch with answer_trust on vs off.
    let on: serde_json::Value =
        serde_json::from_slice(&settings_content(None, Some(cwd)).unwrap()).unwrap();
    assert_eq!(on["trustedWorkspaces"], serde_json::json!([cwd]));

    let off: serde_json::Value =
        serde_json::from_slice(&settings_content(None, None).unwrap()).unwrap();
    assert!(off.get("trustedWorkspaces").is_none(), "{off}");

    // Existing list is kept byte-preservingly around other members, cwd added once.
    let existing =
        b"{\n  \"model\": \"gemini-4-neon\",\n  \"trustedWorkspaces\": [\"/home/user/other\"],\n  \"verbosity\": \"low\"\n}\n";
    let spliced = settings_content(Some(existing), Some(cwd)).unwrap();
    let text = String::from_utf8(spliced.clone()).unwrap();
    assert!(text.contains("\"model\": \"gemini-4-neon\""), "{text}");
    assert!(text.contains("\"verbosity\": \"low\""), "{text}");
    let v: serde_json::Value = serde_json::from_slice(&spliced).unwrap();
    assert_eq!(
        v["trustedWorkspaces"],
        serde_json::json!(["/home/user/other", cwd])
    );

    // Re-running when cwd is already listed does not duplicate it.
    let again = settings_content(Some(&spliced), Some(cwd)).unwrap();
    let v2: serde_json::Value = serde_json::from_slice(&again).unwrap();
    assert_eq!(
        v2["trustedWorkspaces"],
        serde_json::json!(["/home/user/other", cwd])
    );

    // answer_trust = false leaves existing settings without modifying or adding trustedWorkspaces.
    let existing_no_tw = b"{\n  \"model\": \"gemini-4-neon\",\n  \"verbosity\": \"low\"\n}\n";
    let untouched = settings_content(Some(existing_no_tw), None).unwrap();
    let untouched_v: serde_json::Value = serde_json::from_slice(&untouched).unwrap();
    assert!(
        untouched_v.get("trustedWorkspaces").is_none(),
        "{untouched_v}"
    );
    let kept = settings_content(Some(existing), None).unwrap();
    let kept_v: serde_json::Value = serde_json::from_slice(&kept).unwrap();
    assert_eq!(
        kept_v["trustedWorkspaces"],
        serde_json::json!(["/home/user/other"])
    );

    // `plan` sets trusted_workspace for fresh and resume when answer_trust = true, None when false.
    let env = BTreeMap::new();
    for resume in [None, Some("0373977e-3d0c-4628-9218-13ba9695cd55")] {
        let mut c = ctx(&env, cwd);
        c.resume = resume;
        let Prep::Agy(on_prep) = plan(&ident(Some("high")), &c).unwrap().prep.unwrap() else {
            panic!("expected AgyPrep");
        };
        assert_eq!(
            on_prep.trusted_workspace.as_deref(),
            Some(Path::new(cwd)),
            "resume={resume:?}"
        );
        c.answer_trust = false;
        let Prep::Agy(off_prep) = plan(&ident(Some("high")), &c).unwrap().prep.unwrap() else {
            panic!("expected AgyPrep");
        };
        assert_eq!(off_prep.trusted_workspace, None, "resume={resume:?}");
    }
}

#[cfg(unix)]
#[test]
fn prepare_canonicalizes_symlinked_work_area_and_respects_none() {
    let dir = tmp("trust-canon");
    let real = dir.join("real-work");
    let link = dir.join("link-work");
    fs::create_dir_all(&real).unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let canon = fs::canonicalize(&real)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let env = BTreeMap::new();

    let l = plan(&ident(Some("high")), &ctx(&env, link.to_str().unwrap())).unwrap();
    let Some(Prep::Agy(p)) = l.prep else {
        panic!("expected AgyPrep");
    };
    assert_eq!(prepare(&p).unwrap(), None);
    let v: serde_json::Value =
        serde_json::from_slice(&fs::read(real.join(".agy-settings.json")).unwrap()).unwrap();
    assert_eq!(v["trustedWorkspaces"], serde_json::json!([canon]));

    // With answer_trust = false, prepare omits trustedWorkspaces.
    fs::remove_file(real.join(".agy-settings.json")).unwrap();
    let mut off_ctx = ctx(&env, link.to_str().unwrap());
    off_ctx.answer_trust = false;
    let Prep::Agy(off_p) = plan(&ident(Some("high")), &off_ctx).unwrap().prep.unwrap() else {
        panic!("expected AgyPrep");
    };
    assert_eq!(prepare(&off_p).unwrap(), None);
    let v_off: serde_json::Value =
        serde_json::from_slice(&fs::read(real.join(".agy-settings.json")).unwrap()).unwrap();
    assert!(v_off.get("trustedWorkspaces").is_none(), "{v_off}");
}

#[tokio::test]
async fn stage_studio_launches_fresh_resume_and_recast_trust_work_area() {
    use backlot_model::event::{Candidate, EventKind, RoleConfigs, StudioMode};
    use backlot_model::{EventBy, NewEvent};
    use serde_json::{Value, json};

    let dir = tmp("stage-studio-trust");
    let repo = dir.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let sdir = crate::store::studio_dir(dir.path(), "acme-shop");
    let agy_actor = ident(Some("high"));
    let created = EventKind::StudioCreated {
        title: "acme-shop".into(),
        repo: repo.to_string_lossy().into_owned(),
        members: Vec::new(),
        no_git: true,
        mode: StudioMode::Standard,
        roles: Box::new(RoleConfigs {
            producer: agy_actor.clone(),
            director: agy_actor.clone(),
            writer: agy_actor.clone(),
        }),
        candidates: vec![Candidate {
            name: "actor-a".into(),
            runner: agy_actor.runner.clone(),
            runner_version: None,
            model: agy_actor.model.clone(),
            effort: agy_actor.effort.clone(),
            permission_profile: agy_actor.permission_profile.clone(),
        }],
        rework_cap: 2,
        stall_limit_s: 600,
        nudge_after_s: 180,
        escalate_after_s: 600,
        check_env: Vec::new(),
        checks: Vec::new(),
        task_budget_s: None,
        limits: Default::default(),
        max_actors: backlot_model::event::default_max_actors(),
        push: Default::default(),
    };
    for (role, kind) in [
        ("user", created),
        (
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "first".into(),
            },
        ),
        (
            "director",
            backlot_model::testkit::task_created("t1", "feature", 1),
        ),
        (
            "director",
            EventKind::TaskCast {
                task: "t1".into(),
                candidate: "actor-a".into(),
                actor: agy_actor.clone(),
                reason: "picked".into(),
                priority: None,
            },
        ),
    ] {
        crate::store::append(&sdir, |_, _| {
            Ok::<_, std::convert::Infallible>(crate::store::Pending {
                event: NewEvent::new(EventBy::role(role), kind),
                files: Vec::new(),
            })
        })
        .unwrap();
    }

    let client = fake_stage_client().await;

    let mut stage = crate::daemon::Stage::for_test(client);
    stage.base = dir.path().to_path_buf();
    stage.state_dir = dir.path().to_string_lossy().into_owned();
    stage.exe_dir = dir.join("exe").to_string_lossy().into_owned();
    stage.path_env = "/usr/bin".into();

    let canon_repo = fs::canonicalize(&repo)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let settings_path = repo.join(".agy-settings.json");

    let make_start = |resume: Option<crate::reconcile::Resume>| {
        crate::reconcile::Action::Start(crate::reconcile::Start {
            role: "actor-t1-c1".into(),
            kind: crate::reconcile::RoleKind::Actor,
            task: Some("t1".into()),
            terminal: "bl-acme-shop-actor-t1-c1".into(),
            repo: repo.to_string_lossy().into_owned(),
            members: Vec::new(),
            no_git: true,
            actor: agy_actor.clone(),
            resume,
        })
    };

    // Fresh launch.
    let state = crate::store::load_state(&sdir).unwrap();
    stage
        .act(
            "acme-shop",
            &sdir,
            &state,
            &make_start(None),
            &mut true,
            &std::collections::BTreeSet::new(),
        )
        .await
        .unwrap();
    let v_fresh: Value = serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap();
    assert_eq!(v_fresh["trustedWorkspaces"], json!([canon_repo]));

    // Resume (--conversation) launch.
    fs::remove_file(&settings_path).unwrap();
    let state = crate::store::load_state(&sdir).unwrap();
    stage
        .act(
            "acme-shop",
            &sdir,
            &state,
            &make_start(Some(crate::reconcile::Resume {
                session: "conv-123".into(),
                reason: "recovered",
            })),
            &mut true,
            &std::collections::BTreeSet::new(),
        )
        .await
        .unwrap();
    let v_resume: Value = serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap();
    assert_eq!(v_resume["trustedWorkspaces"], json!([canon_repo]));

    // Replacement / recast fresh launch (with existing file: no duplication).
    let state = crate::store::load_state(&sdir).unwrap();
    stage
        .act(
            "acme-shop",
            &sdir,
            &state,
            &make_start(None),
            &mut true,
            &std::collections::BTreeSet::new(),
        )
        .await
        .unwrap();
    let v_recast: Value = serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap();
    assert_eq!(v_recast["trustedWorkspaces"], json!([canon_repo]));

    // answer_trust = false leaves fresh settings without trustedWorkspaces.
    fs::remove_file(&settings_path).unwrap();
    stage.stage_cfg.set(crate::config::StageCfg {
        answer_trust: false,
    });
    let state = crate::store::load_state(&sdir).unwrap();
    stage
        .act(
            "acme-shop",
            &sdir,
            &state,
            &make_start(None),
            &mut true,
            &std::collections::BTreeSet::new(),
        )
        .await
        .unwrap();
    let v_off: Value = serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap();
    assert!(v_off.get("trustedWorkspaces").is_none(), "{v_off}");
}
