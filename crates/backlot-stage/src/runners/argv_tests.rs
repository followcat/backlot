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
        studio_dir: "/home/user/state/studios/acme-shop",
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
        normalized(&l.argv)[..9],
        [
            "claude",
            "--allowedTools",
            "Bash(backlot *),Read(//home/user/state/studios/acme-shop/**)",
            "--model",
            "m",
            "--effort",
            "high",
            "--permission-mode",
            "manual"
        ]
    );
    assert_eq!(l.argv[9], "--session-id");
    let id = l.session_id().unwrap();
    assert_eq!(l.argv[10], id);
    assert_eq!(l.argv.len(), 11);
    // With a prompt it lands last.
    let l = plan(&actor("claude", "standard"), &ctx(&env, Some("hi"), None)).unwrap();
    assert_eq!(l.argv[11], "hi");
    // Worktree edits are auto-accepted.
    let l = plan(&actor("claude", "worktree"), &ctx(&env, None, None)).unwrap();
    assert_eq!(l.argv[8], "acceptEdits");
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
            "--allowedTools",
            "Bash(backlot *),Read(//home/user/state/studios/acme-shop/**)",
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

/// The `--allowedTools` pair is one flag and one comma-joined value
/// of exactly the two rules, on fresh launches and resumes alike —
/// and the value is always followed by another `--flag`, since the
/// variadic list would otherwise swallow the positional prompt.
#[test]
fn claude_allowed_tools_pair() {
    let env = BTreeMap::new();
    for c in [
        ctx(&env, None, None),
        ctx(&env, Some("hi"), None),
        ctx(&env, None, Some("ses-9")),
        ctx(&env, Some("go"), Some("ses-9")),
    ] {
        let l = plan(&actor("claude", "standard"), &c).unwrap();
        let at: Vec<usize> = l
            .argv
            .iter()
            .enumerate()
            .filter_map(|(i, a)| (a == "--allowedTools").then_some(i))
            .collect();
        assert_eq!(at.len(), 1, "{:?}", l.argv);
        let at = at[0];
        let rules: Vec<&str> = l.argv[at + 1].split(',').collect();
        assert_eq!(rules.len(), 2, "{}", l.argv[at + 1]);
        assert_eq!(rules[0], "Bash(backlot *)");
        assert_eq!(rules[1], "Read(//home/user/state/studios/acme-shop/**)");
        assert!(l.argv[at + 2].starts_with("--"), "{}", l.argv[at + 2]);
    }
    // The positional prompt is still the fresh launch's last element.
    let l = plan(&actor("claude", "standard"), &ctx(&env, Some("hi"), None)).unwrap();
    assert_eq!(l.argv.last().unwrap(), "hi");
}

/// Nothing broader is pre-allowed — no `Bash(*)`, no bare `Bash`, no
/// bypass flag or mode, no `--add-dir`, and no `Edit`/`Write` rules —
/// on either permission profile.
#[test]
fn claude_allows_nothing_broader() {
    let env = BTreeMap::new();
    for profile in ["standard", "worktree"] {
        for c in [ctx(&env, Some("hi"), None), ctx(&env, None, Some("ses-1"))] {
            let l = plan(&actor("claude", profile), &c).unwrap();
            for bad in [
                "Bash(*)",
                "skip-permissions",
                "bypassPermissions",
                "--add-dir",
            ] {
                assert!(
                    !l.argv.iter().any(|a| a.contains(bad)),
                    "{profile}: {bad} in {:?}",
                    l.argv
                );
            }
            for bad in ["Bash", "Edit", "Write"] {
                assert!(
                    !l.argv.iter().any(|a| a == bad),
                    "{profile}: {bad} in {:?}",
                    l.argv
                );
            }
        }
    }
}

#[test]
fn claude_scrubs_inherited_markers() {
    let mut env = BTreeMap::new();
    for k in [
        "AI_AGENT",
        "CLAUDE_CODE_CHILD_SESSION",
        "CLAUDE_CODE_ENTRYPOINT",
        "CLAUDE_CODE_EXECPATH",
        "CLAUDE_CODE_MESSAGING_SOCKET",
        "CLAUDE_CODE_MESSAGING_TOKEN",
        "CLAUDE_CODE_SESSION_ATTENDED",
        "CLAUDE_CODE_SESSION_ID",
        "CLAUDE_EFFORT",
        "CLAUDE_PID",
        "CLAUDECODE",
        // Settings a user sets on purpose stay — only the marker
        // names a parent Claude Code leaves are scrubbed.
        "CLAUDE_CODE_OAUTH_TOKEN",
        "CLAUDE_CODE_SESSION",
        "HC_SESSION",
        "UNRELATED",
    ] {
        env.insert(k.to_string(), "1".into());
    }
    let l = plan(&actor("claude", "standard"), &ctx(&env, None, None)).unwrap();
    // BTreeMap order, exact list: AI_AGENT, CLAUDECODE,
    // CLAUDE_CODE_CHILD_SESSION, CLAUDE_CODE_ENTRYPOINT,
    // CLAUDE_CODE_EXECPATH, CLAUDE_CODE_MESSAGING_SOCKET,
    // CLAUDE_CODE_MESSAGING_TOKEN, CLAUDE_CODE_SESSION_ATTENDED,
    // CLAUDE_CODE_SESSION_ID, CLAUDE_EFFORT, CLAUDE_PID.
    let prefix = [
        "env",
        "-u",
        "AI_AGENT",
        "-u",
        "CLAUDECODE",
        "-u",
        "CLAUDE_CODE_CHILD_SESSION",
        "-u",
        "CLAUDE_CODE_ENTRYPOINT",
        "-u",
        "CLAUDE_CODE_EXECPATH",
        "-u",
        "CLAUDE_CODE_MESSAGING_SOCKET",
        "-u",
        "CLAUDE_CODE_MESSAGING_TOKEN",
        "-u",
        "CLAUDE_CODE_SESSION_ATTENDED",
        "-u",
        "CLAUDE_CODE_SESSION_ID",
        "-u",
        "CLAUDE_EFFORT",
        "-u",
        "CLAUDE_PID",
        "claude",
    ];
    assert_eq!(l.argv[..24], prefix);
    // No `-u` ever names an intentional setting.
    assert!(!l.argv.iter().any(|a| a == "CLAUDE_CODE_OAUTH_TOKEN"));
    assert!(!l.argv.iter().any(|a| a == "CLAUDE_CODE_SESSION"));
    // A clean environment — or only intentional settings — leaves
    // argv unprefixed.
    let mut env = BTreeMap::new();
    env.insert("CLAUDE_CODE_OAUTH_TOKEN".to_string(), "1".into());
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
