//! Fail-first tests for PR-6: the `"worktree"` `permission_profile` on
//! `cmd`, `cursor` and `claude`, plus identical-profile pinning for the
//! remaining runners.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use backlot_model::event::{ActorIdentity, CheckDecl};

use crate::runners::{
    IDENTICAL_WORKTREE_RUNNERS, LaunchCtx, Prep, agy, claude, cline, cmd, codex, cursor, devin,
    native_read_only, opencode, plan,
};
use crate::test_support::TestDir;

fn actor(runner: &str, profile: &str) -> ActorIdentity {
    ActorIdentity {
        runner: runner.into(),
        runner_version: None,
        model: match runner {
            "cline" => "cline-pass/sonnet".into(),
            _ => "m".into(),
        },
        effort: match runner {
            "cursor" | "cmd" | "devin" | "opencode" => None,
            _ => Some("high".into()),
        },
        permission_profile: profile.into(),
    }
}

fn ctx<'a>(
    stage_env: &'a BTreeMap<String, String>,
    cwd: &'a str,
    studio_dir: &'a str,
    role: &'a str,
    checks: &'a [CheckDecl],
) -> LaunchCtx<'a> {
    LaunchCtx {
        terminal: "bl-acme-shop-t1",
        cwd,
        prompt: None,
        resume: None,
        studio_dir,
        stage_env,
        role,
        checks,
        answer_trust: true,
    }
}

fn uid_suffix() -> String {
    #[cfg(unix)]
    {
        format!("commandcode-{}", unsafe { libc::getuid() })
    }
    #[cfg(not(unix))]
    {
        "commandcode".to_string()
    }
}

const EXPECTED_DIAGNOSTICS: &[&str] = &["which", "nproc", "df", "free", "test", "ps", "tail"];
const EXPECTED_GIT_READS: &[&str] = &[
    "status",
    "log",
    "show",
    "diff",
    "branch --list",
    "branch --show-current",
];

#[test]
#[cfg(unix)]
fn cmd_worktree_writes_diagnostics_scratch_dir_and_worktree_path_rules() {
    let dir = TestDir::new("cmd-wt-profile");
    let home = dir.join("home");
    let studio_dir = dir.join("state/studios/acme-shop");
    let worktree = studio_dir.join("work/t1");
    fs::create_dir_all(home.join(".commandcode")).unwrap();
    fs::create_dir_all(&worktree).unwrap();

    let mut env = BTreeMap::new();
    env.insert("HOME".into(), home.to_string_lossy().into_owned());
    let wt_str = worktree.to_string_lossy().into_owned();
    let studio_str = studio_dir.to_string_lossy().into_owned();

    let l = plan(
        &actor("cmd", "worktree"),
        &ctx(&env, &wt_str, &studio_str, "actor-t1-c1", &[]),
    )
    .unwrap();
    let Some(Prep::Cmd(prep)) = l.prep else {
        panic!("expected CmdPrep");
    };
    assert_eq!(cmd::prepare(&prep).unwrap(), None);

    let encoded: String = wt_str
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let container = format!("/tmp/{}", uid_suffix());
    let expected_scratch = format!("{container}/{encoded}");

    // Both settings.json `additionalDirectories` and `--add-dir` receive
    // the studio dir and the exact per-worktree scratch dir, never the
    // bare `/tmp/commandcode-<uid>` container.
    assert_eq!(
        prep.additional_dirs,
        vec![studio_str.clone(), expected_scratch.clone()]
    );
    assert!(
        l.argv
            .windows(2)
            .any(|w| w == ["--add-dir", studio_str.as_str()]),
        "missing studio --add-dir in {:?}",
        l.argv
    );
    assert!(
        l.argv
            .windows(2)
            .any(|w| w == ["--add-dir", expected_scratch.as_str()]),
        "missing scratch --add-dir `{expected_scratch}` in {:?}",
        l.argv
    );

    let raw = fs::read_to_string(worktree.join(".commandcode/settings.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let allow: Vec<String> = parsed["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let add_dirs: Vec<String> = parsed["permissions"]["additionalDirectories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();

    assert_eq!(add_dirs, vec![studio_str, expected_scratch]);
    assert!(
        !add_dirs
            .iter()
            .any(|d| d == &container || d == &format!("{container}/")),
        "container dir must never be pre-added: {add_dirs:?}"
    );
    assert!(
        !add_dirs
            .iter()
            .any(|d| d == &home.to_string_lossy().into_owned())
    );

    for diag in EXPECTED_DIAGNOSTICS {
        assert!(
            allow.contains(&format!("Shell({diag}:*)")),
            "missing diagnostic `{diag}` in {allow:?}"
        );
    }
    for ro in native_read_only() {
        assert!(
            allow.contains(&format!("Shell({ro}:*)")),
            "missing READ_ONLY `{ro}` in {allow:?}"
        );
    }
    assert!(
        allow.contains(&format!("Shell(cd {wt_str}:*)")),
        "{allow:?}"
    );
    for sub in EXPECTED_GIT_READS {
        assert!(
            allow.contains(&format!("Shell(git -C {wt_str} {sub}:*)")),
            "missing `git -C <wt> {sub}` in {allow:?}"
        );
    }

    for forbidden in [
        "Shell(git:*)",
        "Shell(git branch:*)",
        "Shell(cargo:*)",
        "Shell(backlot:*)",
        "Shell(curl:*)",
        "Shell(wget:*)",
        "Shell(pip:*)",
        "Shell(uv:*)",
        "Shell(colab:*)",
        "Shell(python:*)",
        "Shell(python3:*)",
        "Shell(bash -c:*)",
        "Shell(sh -c:*)",
    ] {
        assert!(
            !allow.contains(&forbidden.to_string()),
            "forbidden rule `{forbidden}` leaked into {allow:?}"
        );
    }
}

#[test]
#[cfg(unix)]
fn cmd_scratch_dir_matches_command_code_encoding() {
    let container = format!("/tmp/{}", uid_suffix());

    // Preserves case, does not split camel boundaries, keeps leading `-`.
    let env = BTreeMap::new();
    let camel = "/home/user/AControl/work/t1";
    let l = cmd::plan(
        &actor("cmd", "worktree"),
        &ctx(
            &env,
            camel,
            "/home/user/state/studios/acme-shop",
            "actor-t1-c1",
            &[],
        ),
    )
    .unwrap();
    let Some(Prep::Cmd(prep)) = l.prep else {
        panic!("expected CmdPrep");
    };
    assert_eq!(
        prep.additional_dirs,
        vec![
            "/home/user/state/studios/acme-shop".to_string(),
            format!("{container}/-home-user-AControl-work-t1"),
        ]
    );

    // Over-200-char paths truncate to 200 sanitized chars + `-` + 8 base64url chars of sha256(cwd).
    let long_path = format!("/home/user/{}work/t1", "aControlWorktree/".repeat(14));
    assert_eq!(long_path.len(), 256);
    let expected_long = format!(
        "{container}/-home-user-aControlWorktree-aControlWorktree-aControlWorktree-aControlWorktree-aControlWorktree-aControlWorktree-aControlWorktree-aControlWorktree-aControlWorktree-aControlWorktree-aControlWorktree-aC-cQf1kPxv"
    );
    let l = cmd::plan(
        &actor("cmd", "worktree"),
        &ctx(
            &env,
            &long_path,
            "/home/user/state/studios/acme-shop",
            "actor-t1-c1",
            &[],
        ),
    )
    .unwrap();
    let Some(Prep::Cmd(prep)) = l.prep else {
        panic!("expected CmdPrep");
    };
    assert_eq!(prep.additional_dirs[1], expected_long);

    // Respects COMMANDCODE_SCRATCHPAD_BASE when set to an absolute path.
    let mut custom_env = BTreeMap::new();
    custom_env.insert("COMMANDCODE_SCRATCHPAD_BASE".into(), "/var/scratch".into());
    let l = cmd::plan(
        &actor("cmd", "worktree"),
        &ctx(
            &custom_env,
            camel,
            "/home/user/state/studios/acme-shop",
            "actor-t1-c1",
            &[],
        ),
    )
    .unwrap();
    let Some(Prep::Cmd(prep)) = l.prep else {
        panic!("expected CmdPrep");
    };
    assert_eq!(
        prep.additional_dirs[1],
        format!("/var/scratch/{}/-home-user-AControl-work-t1", uid_suffix())
    );
}

#[test]
fn cmd_standard_and_readonly_are_unchanged_by_worktree_profile() {
    let dir = TestDir::new("cmd-std-ro");
    let home = dir.join("home");
    let studio_dir = dir.join("state/studios/acme-shop");
    let worktree = studio_dir.join("work/t1");
    fs::create_dir_all(home.join(".commandcode")).unwrap();
    fs::create_dir_all(&worktree).unwrap();

    let mut env = BTreeMap::new();
    env.insert("HOME".into(), home.to_string_lossy().into_owned());
    let wt_str = worktree.to_string_lossy().into_owned();
    let studio_str = studio_dir.to_string_lossy().into_owned();

    for (profile, is_ro) in [("standard", false), ("readonly", true)] {
        let l = plan(
            &actor("cmd", profile),
            &ctx(&env, &wt_str, &studio_str, "actor-t1-c1", &[]),
        )
        .unwrap();
        let add_dirs: Vec<&str> = l
            .argv
            .windows(2)
            .filter_map(|w| (w[0] == "--add-dir").then_some(w[1].as_str()))
            .collect();
        assert_eq!(
            add_dirs,
            vec![studio_str.as_str()],
            "{profile}: {add_dirs:?}"
        );

        let Some(Prep::Cmd(prep)) = l.prep else {
            panic!("expected CmdPrep");
        };
        assert_eq!(cmd::prepare(&prep).unwrap(), None);
        let raw = fs::read_to_string(worktree.join(".commandcode/settings.json")).unwrap();

        let mut expected_allow = vec!["Shell(backlot)".to_string()];
        for ro in native_read_only() {
            expected_allow.push(format!("Shell({ro}:*)"));
        }
        if !is_ro {
            expected_allow.push("Shell(git add:*)".into());
            expected_allow.push("Shell(git commit:*)".into());
        }
        let expected_deny: Vec<&str> = if is_ro { vec!["Edit", "Write"] } else { vec![] };
        let expected_json = serde_json::to_string(&serde_json::json!({
            "allow": expected_allow,
            "deny": expected_deny,
            "additionalDirectories": [studio_str],
        }))
        .unwrap();
        let expected_prefix = format!("{{\"permissions\":{expected_json},\n  \"hooks\": ");
        assert!(
            raw.starts_with(&expected_prefix),
            "{profile} permissions bytes drifted:\nactual:   {raw}\nexpected: {expected_prefix}..."
        );
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.as_object().unwrap().len(), 2, "{raw}");
        assert!(parsed["hooks"].is_object(), "{raw}");
    }
}

/// Pins the safety invariant checked in `command-code 1.93.0` (`cli.mjs` line 2:
/// `splitShellSubcommands`, `buildShellRuleTarget`, `shellAllowedByPatterns`,
/// `matchesShellPattern`): `cmd` splits compound commands on `&&`/`||`/`;`/`|`
/// and verifies EVERY subcommand segment individually against `matchesShellPattern`
/// (`^prefix( .*)?$`), blocking `cd <wt> && curl ...` and `cd <wt>-other`.
#[test]
fn cmd_worktree_pins_per_segment_matching_assumption() {
    let env = BTreeMap::new();
    let studio_str = "/home/user/state/studios/acme-shop";

    // Hollywood overseer stays strictly on its own allowlist even under worktree.
    let h = cmd::plan(
        &actor("cmd", "worktree"),
        &ctx(&env, "/home/user/work/h", studio_str, "hollywood", &[]),
    )
    .unwrap();
    let Some(Prep::Cmd(h_prep)) = h.prep else {
        panic!("expected CmdPrep");
    };
    assert!(!h_prep.allow.iter().any(|r| r.starts_with("Shell(cd ")));
    assert!(!h_prep.allow.iter().any(|r| r.starts_with("Shell(which")));

    // Relative paths or paths carrying glob/operator syntax never turn into
    // `cd` / `git -C` rules or scratch dirs.
    for unsafe_cwd in [
        "relative/work/t1",
        "/tmp/work*bad",
        "/tmp/work;evil",
        "/tmp/work dir",
    ] {
        let l = cmd::plan(
            &actor("cmd", "worktree"),
            &ctx(&env, unsafe_cwd, studio_str, "actor-t1-c1", &[]),
        )
        .unwrap();
        let Some(Prep::Cmd(prep)) = l.prep else {
            panic!("expected CmdPrep");
        };
        assert!(
            !prep.allow.iter().any(|r| r.contains(unsafe_cwd)),
            "unsafe cwd `{unsafe_cwd}` leaked into {:?}",
            prep.allow
        );
        if !Path::new(unsafe_cwd).is_absolute() {
            assert_eq!(prep.additional_dirs, vec![studio_str.to_string()]);
        }
    }
}

#[test]
fn cursor_worktree_adds_diagnostics_and_worktree_path_rules_without_broadening_standard() {
    let dir = TestDir::new("cursor-wt-profile");
    let home = dir.join("home");
    let studio_dir = dir.join("state/studios/acme-shop");
    let worktree = studio_dir.join("work/t1");
    fs::create_dir_all(home.join(".cursor")).unwrap();
    fs::create_dir_all(&worktree).unwrap();

    let mut env = BTreeMap::new();
    env.insert("HOME".into(), home.to_string_lossy().into_owned());
    let wt_str = worktree.to_string_lossy().into_owned();
    let studio_str = studio_dir.to_string_lossy().into_owned();

    // standard and readonly stay byte-identical to pre-PR-6 baselines.
    for (profile, is_ro, role_name) in [
        ("standard", false, "actor-std-c1"),
        ("readonly", true, "actor-ro-c1"),
    ] {
        let l = plan(
            &actor("cursor", profile),
            &ctx(&env, &wt_str, &studio_str, role_name, &[]),
        )
        .unwrap();
        let Some(Prep::Cursor(prep)) = l.prep else {
            panic!("expected CursorPrep");
        };
        cursor::prepare(&prep).unwrap();
        let bytes = fs::read(prep.root.join("config/cli-config.json")).unwrap();

        let mut expected_allow = vec!["Shell(backlot)".to_string()];
        for ro in native_read_only() {
            expected_allow.push(format!("Shell({ro})"));
        }
        if !is_ro {
            expected_allow.push("Shell(git add)".into());
            expected_allow.push("Shell(git commit)".into());
        }
        let expected_deny: Vec<&str> = if is_ro {
            vec!["Write(*)", "Write(**)"]
        } else {
            vec![]
        };
        let expected = serde_json::to_string_pretty(&serde_json::json!({
            "version": 1,
            "editor": { "vimMode": false },
            "permissions": {
                "allow": expected_allow,
                "deny": expected_deny,
            },
        }))
        .unwrap()
            + "\n";
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            expected,
            "cursor {profile} cli-config drifted"
        );
    }

    // worktree adds exact diagnostics + worktree-scoped cd / git -C reads.
    let l = plan(
        &actor("cursor", "worktree"),
        &ctx(&env, &wt_str, &studio_str, "actor-t1-c1", &[]),
    )
    .unwrap();
    let Some(Prep::Cursor(prep)) = l.prep else {
        panic!("expected CursorPrep");
    };
    cursor::prepare(&prep).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(prep.root.join("config/cli-config.json")).unwrap(),
    )
    .unwrap();
    let allow: Vec<String> = parsed["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();

    for diag in EXPECTED_DIAGNOSTICS {
        assert!(allow.contains(&format!("Shell({diag})")), "{allow:?}");
    }
    assert!(allow.contains(&format!("Shell(cd {wt_str})")), "{allow:?}");
    for sub in EXPECTED_GIT_READS {
        assert!(
            allow.contains(&format!("Shell(git -C {wt_str} {sub})")),
            "missing `git -C <wt> {sub}` in {allow:?}"
        );
    }
    for forbidden in [
        "Shell(git)",
        "Shell(curl)",
        "Shell(wget)",
        "Shell(pip)",
        "Shell(uv)",
        "Shell(colab)",
        "Shell(python)",
        "Shell(python3)",
        "Shell(bash)",
    ] {
        assert!(!allow.contains(&forbidden.to_string()), "{allow:?}");
    }
}

#[test]
fn claude_worktree_adds_diagnostics_and_worktree_path_rules_without_add_dir() {
    let env = BTreeMap::new();
    let studio_str = "/home/user/state/studios/acme-shop";
    let wt_str = "/home/user/state/studios/acme-shop/work/t1";

    // standard and readonly keep identical --allowedTools.
    let std_l = claude::plan(
        &actor("claude", "standard"),
        &ctx(&env, wt_str, studio_str, "actor-t1-c1", &[]),
    )
    .unwrap();
    let std_tools_idx = std_l
        .argv
        .iter()
        .position(|a| a == "--allowedTools")
        .unwrap();
    let std_tools = &std_l.argv[std_tools_idx + 1];
    for diag in EXPECTED_DIAGNOSTICS {
        assert!(!std_tools.contains(&format!("Bash({diag})")), "{std_tools}");
    }
    assert!(
        !std_tools.contains(&format!("Bash(cd {wt_str})")),
        "{std_tools}"
    );

    let ro_l = claude::plan(
        &actor("claude", "readonly"),
        &ctx(&env, wt_str, studio_str, "actor-t1-c1", &[]),
    )
    .unwrap();
    let ro_tools_idx = ro_l
        .argv
        .iter()
        .position(|a| a == "--allowedTools")
        .unwrap();
    let ro_tools = &ro_l.argv[ro_tools_idx + 1];
    for diag in EXPECTED_DIAGNOSTICS {
        assert!(!ro_tools.contains(&format!("Bash({diag})")), "{ro_tools}");
    }
    assert!(!ro_tools.contains("Bash(git add)"), "{ro_tools}");

    // worktree keeps acceptEdits, no --add-dir, and adds diagnostics + worktree path rules.
    let wt_l = claude::plan(
        &actor("claude", "worktree"),
        &ctx(&env, wt_str, studio_str, "actor-t1-c1", &[]),
    )
    .unwrap();
    assert!(
        !wt_l.argv.iter().any(|a| a == "--add-dir"),
        "{:?}",
        wt_l.argv
    );
    let mode_idx = wt_l
        .argv
        .iter()
        .position(|a| a == "--permission-mode")
        .unwrap();
    assert_eq!(wt_l.argv[mode_idx + 1], "acceptEdits");

    let wt_tools_idx = wt_l
        .argv
        .iter()
        .position(|a| a == "--allowedTools")
        .unwrap();
    let rules: Vec<&str> = wt_l.argv[wt_tools_idx + 1].split(',').collect();
    for diag in EXPECTED_DIAGNOSTICS {
        assert!(
            rules.contains(&format!("Bash({diag})").as_str()),
            "{rules:?}"
        );
        assert!(
            rules.contains(&format!("Bash({diag} *)").as_str()),
            "{rules:?}"
        );
    }
    assert!(
        rules.contains(&format!("Bash(cd {wt_str})").as_str()),
        "{rules:?}"
    );
    for sub in EXPECTED_GIT_READS {
        assert!(
            rules.contains(&format!("Bash(git -C {wt_str} {sub})").as_str()),
            "{rules:?}"
        );
        assert!(
            rules.contains(&format!("Bash(git -C {wt_str} {sub} *)").as_str()),
            "{rules:?}"
        );
    }
    for forbidden in [
        "Bash(curl)",
        "Bash(wget)",
        "Bash(pip)",
        "Bash(uv)",
        "Bash(colab)",
        "Bash(python)",
        "Bash(python3)",
    ] {
        assert!(!rules.contains(&forbidden), "{rules:?}");
    }

    // Hollywood stays minimal on worktree.
    let h_l = claude::plan(
        &actor("claude", "worktree"),
        &ctx(&env, wt_str, studio_str, "hollywood", &[]),
    )
    .unwrap();
    let h_idx = h_l.argv.iter().position(|a| a == "--allowedTools").unwrap();
    assert!(
        !h_l.argv[h_idx + 1].contains("Bash(which)"),
        "{}",
        h_l.argv[h_idx + 1]
    );
}

#[test]
fn identical_worktree_runners_produce_same_plan_for_standard_and_worktree() {
    let mut env = BTreeMap::new();
    env.insert("HOME".into(), "/home/user".into());
    let studio_str = "/home/user/state/studios/acme-shop";
    let wt_str = "/home/user/state/studios/acme-shop/work/t1";

    assert_eq!(
        IDENTICAL_WORKTREE_RUNNERS,
        ["devin", "agy", "cline", "codex", "opencode"]
    );
    for runner in IDENTICAL_WORKTREE_RUNNERS {
        let c = LaunchCtx {
            resume: Some("ses-fixed"),
            ..ctx(&env, wt_str, studio_str, "actor-t1-c1", &[])
        };
        let std_plan = plan(&actor(runner, "standard"), &c).unwrap();
        let wt_plan = plan(&actor(runner, "worktree"), &c).unwrap();
        assert_eq!(
            std_plan, wt_plan,
            "expected {runner} to treat standard and worktree identically"
        );
    }

    // Keep unused module imports honest if codex/devin/agy/cline/opencode were routed via runners::plan.
    let _ = (
        agy::MODELS,
        cline::MODELS,
        codex::MODELS,
        devin::MODELS,
        opencode::MODELS,
    );
}
