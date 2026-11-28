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
        answer_trust: true,
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

/// Every role's `--allowedTools` carries the base pair plus the shared
/// read-only set, one bare and one starred rule per command.
fn base_rules() -> String {
    let mut rules = vec![
        "Bash(backlot *)".to_string(),
        "Read(//home/user/state/studios/acme-shop/**)".to_string(),
    ];
    for cmd in crate::runners::native_read_only() {
        rules.push(format!("Bash({cmd})"));
        rules.push(format!("Bash({cmd} *)"));
    }
    rules.join(",")
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
            base_rules().as_str(),
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
fn claude_settings_deny_ask_user_question_and_no_runner_uses_plan_mode() {
    let env = BTreeMap::new();
    let launch = plan(&actor("claude", "standard"), &ctx(&env, None, None)).unwrap();
    let setup = launch.hooks.expect("claude hook setup");
    let json: serde_json::Value = serde_json::from_str(&setup.content).expect("valid json");
    let deny = json["permissions"]["deny"]
        .as_array()
        .expect("permissions.deny array");
    assert!(
        deny.iter().any(|v| v.as_str() == Some("AskUserQuestion")),
        "{json}"
    );
    assert!(json["hooks"].is_object(), "{json}");

    // None of the adapters start a role in an interactive plan mode.
    for caps in backlot_model::caps::ALL {
        let model = match caps.name {
            "cline" => "cline-pass/sonnet",
            _ => "m",
        };
        let effort = match caps.effort {
            backlot_model::caps::Effort::None => None,
            _ => Some("high".into()),
        };
        let id = ActorIdentity {
            runner: caps.name.into(),
            runner_version: None,
            model: model.into(),
            effort,
            permission_profile: "standard".into(),
        };
        let launch = plan(&id, &ctx(&env, Some("hi"), None)).unwrap();
        assert!(
            !launch.argv.iter().any(|a| a == "plan"),
            "{} launches in plan mode: {:?}",
            caps.name,
            launch.argv
        );
    }
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
            base_rules().as_str(),
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
            ["hooks", "permissions"],
            "{file}"
        );
        assert_eq!(
            file["permissions"],
            serde_json::json!({"deny": ["AskUserQuestion"]}),
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
/// that opens with the base pair and carries every shared read-only
/// rule, on fresh launches and resumes alike — and the value is
/// always followed by another `--flag`, since the variadic list would
/// otherwise swallow the positional prompt.
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
        assert_eq!(rules[0], "Bash(backlot *)");
        assert_eq!(rules[1], "Read(//home/user/state/studios/acme-shop/**)");
        for cmd in crate::runners::native_read_only() {
            assert!(
                rules.contains(&format!("Bash({cmd})").as_str()),
                "missing `{cmd}`: {:?}",
                l.argv[at + 1]
            );
            assert!(
                rules.contains(&format!("Bash({cmd} *)").as_str()),
                "missing `{cmd} *`: {:?}",
                l.argv[at + 1]
            );
        }
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

/// S9c: every role's launch pre-allows the shared read-only set —
/// the producer's and writer's included, so no role's `git log` asks
/// a person — plus the actor's `git add`/`git commit`, the director's
/// `git merge`, and every declared check's `run`, each bare and with
/// arguments. `git branch` stays its two listing forms only, and a
/// `run` the rule syntax cannot hold — a parenthesis — is simply
/// never pre-allowed though the cast accepted it.
#[test]
fn claude_pre_allows_per_role() {
    use backlot_model::event::CheckDecl;
    let env = BTreeMap::new();
    // The base pair plus the shared read-only set — what every role
    // gets before its own additions.
    let base = base_rules();
    let checks = [
        CheckDecl::new("unit", "cargo test --lib", 600),
        CheckDecl::new("lint", "cargo clippy", 600),
    ];
    // Producer and writer: base + read-only, nothing a job adds.
    for role in ["producer", "writer"] {
        let l = plan(
            &actor("claude", "standard"),
            &role_ctx(&env, role, None, None, &checks),
        )
        .unwrap();
        assert_eq!(l.argv[2], base, "{role}");
    }
    // An actor: read-only, its two writes, then the checks.
    let l = plan(
        &actor("claude", "standard"),
        &role_ctx(&env, "actor-t1-c1", None, None, &checks),
    )
    .unwrap();
    assert_eq!(
        l.argv[2],
        format!(
            "{base},Bash(git add),Bash(git add *),\
             Bash(git commit),Bash(git commit *),\
             Bash(cargo test --lib),Bash(cargo clippy)"
        )
    );
    // The director: read-only, `git merge`, the same checks — never
    // bare `git branch`, which would also allow `git branch -D`.
    let l = plan(
        &actor("claude", "standard"),
        &role_ctx(&env, "director", None, None, &checks),
    )
    .unwrap();
    assert_eq!(
        l.argv[2],
        format!(
            "{base},Bash(git merge),Bash(git merge *),\
             Bash(cargo test --lib),Bash(cargo clippy)"
        )
    );
    // With no declared checks the actor's read-only set and writes
    // still land, resume or fresh alike.
    for resume in [None, Some("ses-9")] {
        let l = plan(
            &actor("claude", "standard"),
            &role_ctx(&env, "actor-t1-c1", None, resume, &[]),
        )
        .unwrap();
        let at = l.argv.iter().position(|a| a == "--allowedTools").unwrap();
        // 2 base + 2 per read-only command + 4 writes.
        let want = 2 + 2 * crate::runners::native_read_only().count() + 4;
        assert_eq!(
            l.argv[at + 1].split(',').count(),
            want,
            "{}",
            l.argv[at + 1]
        );
    }
    // A `run` with a parenthesis is not expressible in the rule
    // syntax — the cast accepted it, no rule carries it.
    let paren = [CheckDecl::new("odd", "make (all)", 600)];
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
/// model, `--add-dir` puts the studio's state directory inside the
/// workspace (a role's cwd is a worktree), and the measured flag
/// trio always rides. The prompt is positional on a fresh launch
/// only — a resume ignores it, so the stage sends it after the
/// composer opens (like devin). `NODE_USE_ENV_PROXY=1` is always in
/// the env — measured: without it the model traffic bypasses the
/// company proxy (S12a).
#[test]
fn cmd_argv_and_env() {
    let env = BTreeMap::new();
    #[cfg(unix)]
    let scratch_dir = format!("/tmp/commandcode-{}/-home-user-acme-shop", unsafe {
        libc::getuid()
    });
    #[cfg(not(unix))]
    let scratch_dir = "/tmp/commandcode/-home-user-acme-shop".to_string();

    for profile in ["standard", "worktree"] {
        let mut expected_fresh = vec![
            "cmd",
            "-n",
            "bl-s-t1",
            "-m",
            "m",
            "--trust",
            "--skip-onboarding",
            "--no-auto-update",
            "--add-dir",
            "/home/user/state/studios/acme-shop",
        ];
        let mut expected_resume = vec![
            "cmd",
            "-r",
            "sess-9",
            "-m",
            "m",
            "--trust",
            "--skip-onboarding",
            "--no-auto-update",
            "--add-dir",
            "/home/user/state/studios/acme-shop",
        ];
        if profile == "worktree" {
            expected_fresh.extend(["--add-dir", &scratch_dir]);
            expected_resume.extend(["--add-dir", &scratch_dir]);
        }

        let l = plan(&actor("cmd", profile), &ctx(&env, None, None)).unwrap();
        assert_eq!(l.argv, expected_fresh);
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
        assert_eq!(l.argv, expected_resume);
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

/// cmd's `--add-dir` reaches the studio state directory so the
/// stage-sent message files (and prompts) are inside the workspace
/// — a role's cwd is its worktree, so without it every read prompts.
/// It rides fresh and resumed alike.
#[test]
fn cmd_add_dir_is_the_studio_dir() {
    let env = BTreeMap::new();
    for resume in [None, Some("sess-9")] {
        for role in ["producer", "director", "writer", "actor-t1-c1"] {
            let l = plan(
                &actor("cmd", "standard"),
                &role_ctx(&env, role, None, resume, &[]),
            )
            .unwrap();
            let at: Vec<usize> = l
                .argv
                .iter()
                .enumerate()
                .filter_map(|(i, a)| (a == "--add-dir").then_some(i))
                .collect();
            assert_eq!(at.len(), 1, "{role}: {:?}", l.argv);
            assert_eq!(
                l.argv[at[0] + 1],
                "/home/user/state/studios/acme-shop",
                "{role}"
            );
        }
    }
}

/// Hollywood runs in `<base>/hollywood` and reads every studio's
/// state under `<base>/studios`, its cwd's sibling — so its
/// `--add-dir` is the `studios` directory, not its own.
#[test]
fn cmd_hollywood_add_dir_is_the_studios_dir() {
    let env = BTreeMap::new();
    for resume in [None, Some("sess-9")] {
        let c = LaunchCtx {
            cwd: "/home/user/state/hollywood",
            studio_dir: "/home/user/state/hollywood",
            ..role_ctx(&env, "hollywood", None, resume, &[])
        };
        let l = plan(&actor("cmd", "standard"), &c).unwrap();
        let at = l.argv.iter().position(|a| a == "--add-dir").unwrap();
        assert_eq!(l.argv[at + 1], "/home/user/state/studios", "{:?}", l.argv);
    }
}

/// A profile no runner knows is a refusal, whatever the runner.
#[test]
fn an_unknown_profile_refuses_every_runner() {
    let env = BTreeMap::new();
    for runner in [
        "claude", "codex", "devin", "opencode", "cursor", "cmd", "cline",
    ] {
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

/// The fixed `rg` read remains covered by backlot's whole-argv
/// auto-answer, but no runner gets a native prefix that would also
/// cover `--pre` or another later exec-capable flag.
#[test]
fn exec_capable_fixed_reads_stay_out_of_native_allow_lists() {
    assert!(crate::permit_cmd::fixed_covers("rg"));
    let checks = [];
    let claude_rules: Vec<String> =
        claude::allowed_tools("/home/user/state/studios/acme-shop", "producer", &checks)
            .split(',')
            .map(str::to_string)
            .collect();
    for (runner, rules, forbidden) in [
        ("devin", devin::allow_rules(), vec!["Exec(rg)"]),
        (
            "cmd",
            cmd::allow_rules("producer", &checks),
            vec!["Shell(rg:*)"],
        ),
        (
            "cursor",
            cursor::allow_rules("producer", &checks),
            vec!["Shell(rg)"],
        ),
        (
            "cline",
            cline::allow_commands("producer", &checks),
            vec!["rg"],
        ),
        ("claude", claude_rules, vec!["Bash(rg)", "Bash(rg *)"]),
    ] {
        for rule in forbidden {
            assert!(
                !rules.iter().any(|r| r == rule),
                "{runner} natively allows `{rule}`: {rules:?}"
            );
        }
    }
}
