//! Exact-argv tests for every adapter: new and resume, each
//! permission profile, with and without a first prompt, plus the
//! refusal paths and the marker scrub.

use super::*;

fn actor(runner: &str, profile: &str) -> ActorIdentity {
    ActorIdentity {
        runner: runner.into(),
        runner_version: None,
        model: "m".into(),
        effort: "high".into(),
        permission_profile: profile.into(),
    }
}

fn ctx<'a>(
    stage_env: &'a BTreeMap<String, String>,
    prompt: Option<&'a str>,
    resume: Option<&'a str>,
) -> LaunchCtx<'a> {
    LaunchCtx {
        terminal: "bl-s-t1",
        cwd: "/home/user/acme-shop",
        prompt,
        resume,
        stage_env,
    }
}

/// `argv` with any `--session-id`/`env -u` variables blanked, so the
/// expected arrays can stay literal.
fn normalized(argv: &[String]) -> Vec<String> {
    argv.iter()
        .map(|a| {
            if a.len() == 36 && a.chars().filter(|c| *c == '-').count() == 4 {
                "<uuid>".into()
            } else {
                a.clone()
            }
        })
        .collect()
}

#[test]
fn claude_new_argv() {
    let env = BTreeMap::new();
    let l = plan(&actor("claude", "standard"), &ctx(&env, None, None)).unwrap();
    assert_eq!(
        normalized(&l.argv)[..7],
        [
            "claude",
            "--model",
            "m",
            "--effort",
            "high",
            "--permission-mode",
            "manual"
        ]
    );
    assert_eq!(l.argv[7], "--session-id");
    let id = l.session_id().unwrap();
    assert_eq!(l.argv[8], id);
    assert_eq!(l.argv.len(), 9);
    // With a prompt it lands last.
    let l = plan(&actor("claude", "standard"), &ctx(&env, Some("hi"), None)).unwrap();
    assert_eq!(l.argv[9], "hi");
    // Worktree edits are auto-accepted.
    let l = plan(&actor("claude", "worktree"), &ctx(&env, None, None)).unwrap();
    assert_eq!(l.argv[6], "acceptEdits");
}

#[test]
fn claude_resume_argv() {
    let env = BTreeMap::new();
    let c = ctx(&env, Some("go"), Some("ses-9"));
    let l = plan(&actor("claude", "standard"), &c).unwrap();
    assert_eq!(
        l.argv,
        [
            "claude",
            "-r",
            "ses-9",
            "--model",
            "m",
            "--effort",
            "high",
            "--permission-mode",
            "manual",
            "go"
        ]
    );
    assert_eq!(l.session_id().as_deref(), Some("ses-9"));
}

#[test]
fn claude_scrubs_inherited_markers() {
    let mut env = BTreeMap::new();
    for k in [
        "AI_AGENT",
        "CLAUDE_CODE_CHILD_SESSION",
        "CLAUDE_CODE_SESSION",
        "CLAUDE_EFFORT",
        "CLAUDE_PID",
        "CLAUDECODE",
        "HC_SESSION",
        "UNRELATED",
    ] {
        env.insert(k.to_string(), "1".into());
    }
    let l = plan(&actor("claude", "standard"), &ctx(&env, None, None)).unwrap();
    // BTreeMap order: AI_AGENT, CLAUDECODE, CLAUDE_CODE_CHILD_SESSION,
    // CLAUDE_CODE_SESSION, CLAUDE_EFFORT, CLAUDE_PID.
    let prefix = [
        "env",
        "-u",
        "AI_AGENT",
        "-u",
        "CLAUDECODE",
        "-u",
        "CLAUDE_CODE_CHILD_SESSION",
        "-u",
        "CLAUDE_CODE_SESSION",
        "-u",
        "CLAUDE_EFFORT",
        "-u",
        "CLAUDE_PID",
        "claude",
    ];
    assert_eq!(l.argv[..14], prefix);
    // A clean environment leaves argv unprefixed.
    let env = BTreeMap::new();
    let l = plan(&actor("claude", "standard"), &ctx(&env, None, None)).unwrap();
    assert_eq!(l.argv[0], "claude");
}

#[test]
fn codex_argv() {
    let env = BTreeMap::new();
    let trust = "-c";
    let trust_val = "projects={\"/home/user/acme-shop\"={trust_level=\"trusted\"}}";
    for profile in ["standard", "worktree"] {
        let l = plan(&actor("codex", profile), &ctx(&env, None, None)).unwrap();
        assert_eq!(
            l.argv,
            [
                "codex",
                "-m",
                "m",
                "-c",
                "model_reasoning_effort=high",
                trust,
                trust_val,
                "-s",
                "workspace-write",
                "-a",
                "on-request"
            ]
        );
        assert_eq!(l.session, Session::PostLaunch);
        // Resume keeps the flags and adds the prompt last.
        let c = ctx(&env, Some("more"), Some("0199abc"));
        let l = plan(&actor("codex", profile), &c).unwrap();
        assert_eq!(
            l.argv,
            [
                "codex",
                "resume",
                "0199abc",
                "-m",
                "m",
                "-c",
                "model_reasoning_effort=high",
                trust,
                trust_val,
                "-s",
                "workspace-write",
                "-a",
                "on-request",
                "more"
            ]
        );
    }
    let c = ctx(&env, Some("first"), None);
    let l = plan(&actor("codex", "standard"), &c).unwrap();
    assert_eq!(l.argv.last().unwrap(), "first");
}

#[test]
fn devin_argv() {
    let env = BTreeMap::new();
    for profile in ["standard", "worktree"] {
        let c = ctx(&env, Some("/state/prompt.txt"), None);
        let l = plan(&actor("devin", profile), &c).unwrap();
        assert_eq!(
            l.argv,
            [
                "devin",
                "--model",
                "m",
                "--permission-mode",
                "smart",
                "--respect-workspace-trust",
                "false",
                "--prompt-file",
                "/state/prompt.txt"
            ]
        );
        assert_eq!(l.session, Session::PostLaunch);
        // A resume ignores --prompt-file entirely.
        let c = ctx(&env, Some("/state/prompt.txt"), Some("dev-7"));
        let l = plan(&actor("devin", profile), &c).unwrap();
        assert_eq!(
            l.argv,
            [
                "devin",
                "-r",
                "dev-7",
                "--model",
                "m",
                "--permission-mode",
                "smart",
                "--respect-workspace-trust",
                "false"
            ]
        );
    }
    let l = plan(&actor("devin", "standard"), &ctx(&env, None, None)).unwrap();
    assert!(!l.argv.iter().any(|a| a == "--prompt-file"));
    // The effort is part of the model id; no flag carries it.
    assert!(!l.argv.iter().any(|a| a.contains("high")));
}

#[test]
fn opencode_argv_and_env() {
    let env = BTreeMap::new();
    for profile in ["standard", "worktree"] {
        let l = plan(&actor("opencode", profile), &ctx(&env, None, None)).unwrap();
        assert_eq!(l.argv[0], "opencode");
        assert_eq!(l.argv[1], "--standalone");
        assert_eq!(l.argv[2], "-s");
        let id = l.session_id().unwrap();
        assert!(id.starts_with("ses_"), "{id}");
        assert_eq!(l.argv[3], id);
        assert_eq!(l.argv.len(), 4);
        assert_eq!(
            l.env.get("OPENCODE_CONFIG_CONTENT").unwrap(),
            "{\"model\":\"m\"}"
        );
        // Resume uses the recorded id; a prompt lands under --prompt.
        let c = ctx(&env, Some("hi"), Some("ses_saved"));
        let l = plan(&actor("opencode", profile), &c).unwrap();
        assert_eq!(
            l.argv,
            [
                "opencode",
                "--standalone",
                "-s",
                "ses_saved",
                "--prompt",
                "hi"
            ]
        );
    }
}

/// A profile no runner knows is a refusal, whatever the runner.
#[test]
fn an_unknown_profile_refuses_every_runner() {
    let env = BTreeMap::new();
    for runner in ["claude", "codex", "devin", "opencode"] {
        let e = plan(&actor(runner, "nowhere"), &ctx(&env, None, None)).unwrap_err();
        assert_eq!(e.reason, PROFILE_NOT_SUPPORTED, "{runner}");
        assert!(e.detail.contains("nowhere"), "{e:?}");
    }
}

/// `session_in_argv` reads the chosen id back off a live terminal.
#[test]
fn session_ids_read_back_from_argv() {
    let env = BTreeMap::new();
    let l = plan(&actor("claude", "standard"), &ctx(&env, None, None)).unwrap();
    assert_eq!(session_in_argv("claude", &l.argv), l.session_id());
    // A resume id reads back too, through `-r`.
    let c = ctx(&env, None, Some("old-1"));
    let l = plan(&actor("claude", "standard"), &c).unwrap();
    assert_eq!(session_in_argv("claude", &l.argv).as_deref(), Some("old-1"));
    let l = plan(&actor("opencode", "standard"), &ctx(&env, None, None)).unwrap();
    assert_eq!(session_in_argv("opencode", &l.argv), l.session_id());
    // Runners that do not choose give nothing.
    assert_eq!(session_in_argv("codex", &["codex".into()]), None);
}
