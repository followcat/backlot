//! Exact-argv tests for every adapter: new and resume, each
//! permission profile, with and without a first prompt, plus the
//! refusal paths and the marker scrub.

use super::*;

fn actor(runner: &str, profile: &str) -> ActorIdentity {
    ActorIdentity {
        runner: runner.into(),
        runner_version: None,
        model: "m".into(),
        effort: Some("high".into()),
        permission_profile: profile.into(),
    }
}

fn ctx<'a>(
    stage_env: &'a BTreeMap<String, String>,
    prompt: Option<&'a str>,
    resume: Option<&'a str>,
) -> LaunchCtx<'a> {
    // The role key decides the pre-allow rules — the base case is a
    // fixed role with no declared checks.
    role_ctx(stage_env, "producer", prompt, resume, &[])
}

fn role_ctx<'a>(
    stage_env: &'a BTreeMap<String, String>,
    role: &'a str,
    prompt: Option<&'a str>,
    resume: Option<&'a str>,
    checks: &'a [backlot_model::event::CheckDecl],
) -> LaunchCtx<'a> {
    LaunchCtx {
        terminal: "bl-s-t1",
        cwd: "/home/user/acme-shop",
        prompt,
        resume,
        studio_dir: "/home/user/state/studios/acme-shop",
        stage_env,
        role,
        checks,
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
        normalized(&l.argv)[..11],
        [
            "claude",
            "--allowedTools",
            "Bash(backlot *),Read(//home/user/state/studios/acme-shop/**)",
            "--settings",
            "/home/user/state/studios/acme-shop/roles/producer/claude-settings.json",
            "--model",
            "m",
            "--effort",
            "high",
            "--permission-mode",
            "manual"
        ]
    );
    assert_eq!(l.argv[11], "--session-id");
    let id = l.session_id().unwrap();
    assert_eq!(l.argv[12], id);
    assert_eq!(l.argv.len(), 13);
    // With a prompt it lands last.
    let l = plan(&actor("claude", "standard"), &ctx(&env, Some("hi"), None)).unwrap();
    assert_eq!(l.argv[13], "hi");
    // Worktree edits are auto-accepted.
    let l = plan(&actor("claude", "worktree"), &ctx(&env, None, None)).unwrap();
    assert_eq!(l.argv[10], "acceptEdits");
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
            "--settings",
            "/home/user/state/studios/acme-shop/roles/producer/claude-settings.json",
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

/// S9f: every launch — fresh or resumed — carries a `HookSetup`
/// under `roles/<role>/`: the `--settings` argv names its file, the
/// file holds exactly the eight hook events each wired to the
/// binary's own `hook` command at a five second timeout — nothing
/// else, no permissions and no env, so the `--settings` layer can
/// never change what the user's own settings decide.
#[test]
fn claude_settings_file_is_hooks_only() {
    let env = BTreeMap::new();
    for c in [
        ctx(&env, None, None),
        ctx(&env, Some("go"), Some("ses-9")),
        ctx(&env, None, Some("ses-9")),
    ] {
        let l = plan(&actor("claude", "standard"), &c).unwrap();
        let h = l.hooks.as_ref().expect("a hook setup");
        assert_eq!(
            h.settings.to_str().unwrap(),
            "/home/user/state/studios/acme-shop/roles/producer/claude-settings.json"
        );
        assert_eq!(
            h.signals.to_str().unwrap(),
            "/home/user/state/studios/acme-shop/roles/producer/signals.jsonl"
        );
        let at = l.argv.iter().position(|a| a == "--settings").unwrap();
        assert_eq!(l.argv[at + 1], h.settings.to_str().unwrap());
        let file: serde_json::Value = serde_json::from_str(&h.content).unwrap();
        assert_eq!(
            file.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["hooks"],
            "{file}"
        );
        let hooks = file["hooks"].as_object().unwrap();
        assert_eq!(hooks.len(), 8);
        for event in [
            "UserPromptSubmit",
            "PreToolUse",
            "PostToolUse",
            "PostToolUseFailure",
            "PermissionRequest",
            "Notification",
            "Stop",
            "StopFailure",
        ] {
            let entry = &hooks[event][0];
            assert!(
                !entry.as_object().expect(event).contains_key("matcher"),
                "{event} carried a matcher"
            );
            let hook = &entry["hooks"][0];
            assert_eq!(hook["type"], "command", "{event}");
            assert_eq!(hook["timeout"], 5, "{event}");
            assert_eq!(hook["command"], h.command, "{event}");
        }
        assert!(h.command.contains(" hook --file "), "{}", h.command);
        assert!(h.command.ends_with("signals.jsonl'"), "{}", h.command);
    }
}

/// The hook command is shell-safe even when the studio directory
/// holds a space and a quote: `sh` must word-split it back into the
/// binary path, `hook --file`, and the literal signals path.
#[test]
#[cfg(unix)]
fn hook_command_quotes_spaces_and_quotes() {
    let env = BTreeMap::new();
    let c = LaunchCtx {
        studio_dir: "/home/user/stu dio's/acme-shop",
        ..ctx(&env, None, None)
    };
    let l = plan(&actor("claude", "standard"), &c).unwrap();
    let h = l.hooks.unwrap();
    let exe = std::env::current_exe().unwrap().display().to_string();
    assert_eq!(
        h.command,
        format!(
            "'{exe}' hook --file '/home/user/stu dio'\\''s/acme-shop/roles/producer/signals.jsonl'"
        ),
        "{}",
        h.command
    );
    // `echo` unquotes the command: its output must be the two bare
    // paths with the words between.
    let out = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("echo {}", h.command))
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().trim(),
        format!("{exe} hook --file /home/user/stu dio's/acme-shop/roles/producer/signals.jsonl")
    );
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

/// S9c: a Claude actor's launch pre-allows the git commands its
/// loop needs plus every declared check's `run` — each bare and
/// with arguments. The director gets `git merge` and the same
/// checks; producer and writer stay on the base pair, and a `run`
/// the rule syntax cannot hold — a parenthesis — is simply never
/// pre-allowed though the cast accepted it.
#[test]
fn claude_pre_allows_per_role() {
    use backlot_model::event::CheckDecl;
    let env = BTreeMap::new();
    let base = "Bash(backlot *),Read(//home/user/state/studios/acme-shop/**)";
    let checks = [
        CheckDecl {
            name: "unit".into(),
            run: "cargo test --lib".into(),
            timeout_s: 600,
        },
        CheckDecl {
            name: "lint".into(),
            run: "cargo clippy".into(),
            timeout_s: 600,
        },
    ];
    // An actor: the six git commands bare and starred, then the
    // checks verbatim.
    let l = plan(
        &actor("claude", "standard"),
        &role_ctx(&env, "actor-t1-c1", None, None, &checks),
    )
    .unwrap();
    assert_eq!(
        l.argv[2],
        format!(
            "{base},Bash(git add),Bash(git add *),Bash(git commit),Bash(git commit *),\
             Bash(git status),Bash(git status *),Bash(git diff),Bash(git diff *),\
             Bash(git log),Bash(git log *),Bash(git show),Bash(git show *),\
             Bash(cargo test --lib),Bash(cargo clippy)"
        )
    );
    // The director: the read-only git set it reviews with, `git
    // merge` and the same checks — never `git branch`, which would
    // also allow `git branch -D`.
    let l = plan(
        &actor("claude", "standard"),
        &role_ctx(&env, "director", None, None, &checks),
    )
    .unwrap();
    assert_eq!(
        l.argv[2],
        format!(
            "{base},Bash(git status),Bash(git status *),Bash(git diff),Bash(git diff *),\
             Bash(git log),Bash(git log *),Bash(git show),Bash(git show *),\
             Bash(git merge),Bash(git merge *),Bash(cargo test --lib),Bash(cargo clippy)"
        )
    );
    // Producer and writer keep the base pair only.
    for role in ["producer", "writer"] {
        let l = plan(
            &actor("claude", "standard"),
            &role_ctx(&env, role, None, None, &checks),
        )
        .unwrap();
        assert_eq!(l.argv[2], base, "{role}");
    }
    // With no declared checks the actor's git set still lands,
    // resume or fresh alike.
    for resume in [None, Some("ses-9")] {
        let l = plan(
            &actor("claude", "standard"),
            &role_ctx(&env, "actor-t1-c1", None, resume, &[]),
        )
        .unwrap();
        let at = l.argv.iter().position(|a| a == "--allowedTools").unwrap();
        assert_eq!(l.argv[at + 1].split(',').count(), 14, "{}", l.argv[at + 1]);
    }
    // A `run` with a parenthesis is not expressible in the rule
    // syntax — the cast accepted it, no rule carries it.
    let paren = [CheckDecl {
        name: "odd".into(),
        run: "make (all)".into(),
        timeout_s: 600,
    }];
    let l = plan(
        &actor("claude", "standard"),
        &role_ctx(&env, "actor-t1-c1", None, None, &paren),
    )
    .unwrap();
    assert!(!l.argv[2].contains("make"), "{}", l.argv[2]);
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

/// cursor's fresh and resume argv — `--trust` (its marker lands in
/// the role's own data dir), `--model`, the prompt positional and
/// last — plus the per-role env trio under the studio state dir.
/// The session id is found after launch; effort rides in the model
/// id, so no flag carries it.
#[test]
fn cursor_argv_and_env() {
    let env = BTreeMap::new();
    for profile in ["standard", "worktree"] {
        let l = plan(&actor("cursor", profile), &ctx(&env, None, None)).unwrap();
        assert_eq!(l.argv, ["cursor-agent", "--trust", "--model", "m"]);
        assert_eq!(l.session, Session::PostLaunch);
        // A first prompt is positional and last.
        let c = ctx(&env, Some("first"), None);
        let l = plan(&actor("cursor", profile), &c).unwrap();
        assert_eq!(l.argv, ["cursor-agent", "--trust", "--model", "m", "first"]);
        // Resume: `--resume <id>` ahead of `--trust`, prompt last.
        let c = ctx(&env, Some("more"), Some("chat-9"));
        let l = plan(&actor("cursor", profile), &c).unwrap();
        assert_eq!(
            l.argv,
            [
                "cursor-agent",
                "--resume",
                "chat-9",
                "--trust",
                "--model",
                "m",
                "more"
            ]
        );
        // The forbidden flags never appear, fresh or resume.
        for flag in ["--force", "--yolo", "--approve-mcps", "-p", "--print"] {
            assert!(!l.argv.iter().any(|a| a == flag), "{flag}");
        }
        // The env trio roots in the role's own cursor tree inside
        // the studio state directory — never the worktree, and
        // `XDG_CONFIG_HOME` is never set.
        let root = "/home/user/state/studios/acme-shop/roles/producer/cursor";
        assert_eq!(l.env["CURSOR_CONFIG_DIR"], format!("{root}/config"));
        assert_eq!(l.env["CURSOR_DATA_DIR"], format!("{root}/data"));
        assert_eq!(
            l.env["CURSOR_SANDBOX_POLICY_DIR"],
            format!("{root}/sandbox")
        );
        assert!(!l.env.contains_key("XDG_CONFIG_HOME"));
    }
    // An `XDG_CONFIG_HOME` in the stage's own env refuses the
    // launch: cursor's auth.json moves with it, so the role would
    // start logged out — the attention names that cause.
    let mut env = BTreeMap::new();
    env.insert("XDG_CONFIG_HOME".into(), "/home/user/.config".into());
    let e = plan(&actor("cursor", "standard"), &ctx(&env, None, None)).unwrap_err();
    assert_eq!(e.reason, cursor::XDG_CONFIG_HOME_SET);
    assert!(e.detail.contains("XDG_CONFIG_HOME"), "{e:?}");
    // An empty value is the XDG convention for unset — no refusal.
    env.insert("XDG_CONFIG_HOME".into(), String::new());
    assert!(plan(&actor("cursor", "standard"), &ctx(&env, None, None)).is_ok());
}

/// cmd's fresh and resume argv — `-n <terminal>` names a fresh
/// session (resumable by name), `-r <id>` resumes, `-m` carries the
/// model, and the measured flag trio always rides. The prompt is
/// positional on a fresh launch only — a resume ignores it, so the
/// stage sends it after the composer opens (like devin).
/// `NODE_USE_ENV_PROXY=1` is always in the env — measured: without
/// it the model traffic bypasses the company proxy (S12a).
#[test]
fn cmd_argv_and_env() {
    let env = BTreeMap::new();
    for profile in ["standard", "worktree"] {
        let l = plan(&actor("cmd", profile), &ctx(&env, None, None)).unwrap();
        assert_eq!(
            l.argv,
            [
                "cmd",
                "-n",
                "bl-s-t1",
                "-m",
                "m",
                "--trust",
                "--skip-onboarding",
                "--no-auto-update"
            ]
        );
        assert_eq!(l.session, Session::PostLaunch);
        assert_eq!(l.env["NODE_USE_ENV_PROXY"], "1");
        // A first prompt is positional and last.
        let c = ctx(&env, Some("first"), None);
        let l = plan(&actor("cmd", profile), &c).unwrap();
        assert_eq!(l.argv.last().unwrap(), "first");
        // Resume: `-r <id>` ahead of `-m`, no `-n`, no prompt —
        // the positional is ignored on a resumed TUI.
        let c = ctx(&env, Some("more"), Some("sess-9"));
        let l = plan(&actor("cmd", profile), &c).unwrap();
        assert_eq!(
            l.argv,
            [
                "cmd",
                "-r",
                "sess-9",
                "-m",
                "m",
                "--trust",
                "--skip-onboarding",
                "--no-auto-update"
            ]
        );
        assert!(!l.argv.iter().any(|a| a == "more"), "{:?}", l.argv);
        // The never-flags, fresh or resume — `--effort` writes the
        // GLOBAL config (measured); the rest are non-interactive
        // modes, global writes or bypasses the stage must not
        // produce.
        for flag in [
            "--effort",
            "--permission-mode",
            "--tools-all",
            "--tools-enable",
            "--yolo",
            "--dangerously-skip-permissions",
            "--config",
            "-p",
            "--print",
            "--output-format",
            "-w",
            "--cloud",
            "--no-session",
        ] {
            assert!(!l.argv.iter().any(|a| a == flag), "{flag}");
        }
    }
}

/// A profile no runner knows is a refusal, whatever the runner.
#[test]
fn an_unknown_profile_refuses_every_runner() {
    let env = BTreeMap::new();
    for runner in ["claude", "codex", "devin", "opencode", "cursor", "cmd"] {
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

/// A binary replaced under the running process reports
/// `<path> (deleted)` — the marker comes off when the path without
/// it exists, and stays when nothing is there either.
#[test]
fn a_replaced_binary_names_the_live_path() {
    use std::path::{Path, PathBuf};
    let exists = |p: &Path| p == Path::new("/usr/bin/backlot");
    assert_eq!(
        claude::live_exe(PathBuf::from("/usr/bin/backlot (deleted)"), exists),
        PathBuf::from("/usr/bin/backlot")
    );
    assert_eq!(
        claude::live_exe(PathBuf::from("/gone/backlot (deleted)"), exists),
        PathBuf::from("/gone/backlot (deleted)")
    );
    assert_eq!(
        claude::live_exe(PathBuf::from("/usr/bin/backlot"), exists),
        PathBuf::from("/usr/bin/backlot")
    );
}
