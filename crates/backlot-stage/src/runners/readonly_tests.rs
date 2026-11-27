use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use backlot_model::event::ActorIdentity;

use super::{self as runners, LaunchCtx, Prep};
use crate::test_support::TestDir;

fn runner_id(runner: &str, profile: &str) -> ActorIdentity {
    ActorIdentity {
        runner: runner.into(),
        runner_version: None,
        model: if runner == "cline" {
            "cline-pass/sonnet"
        } else {
            "m"
        }
        .into(),
        effort: (!matches!(runner, "cursor" | "cmd" | "opencode" | "devin")).then(|| "high".into()),
        permission_profile: profile.into(),
    }
}

#[test]
fn per_runner_plan_configures_readonly_native_settings() {
    let dir = TestDir::new("bl-ro-runners");
    let home = dir.join("home");
    fs::create_dir_all(&home).unwrap();
    let studio_dir = dir.join("state/studios/acme-shop");
    let cwd_str = dir.to_string_lossy().into_owned();
    let studio_str = studio_dir.to_string_lossy().into_owned();
    let env = BTreeMap::from([("HOME".into(), home.to_string_lossy().into_owned())]);
    let ctx = LaunchCtx {
        terminal: "bl-s-t1",
        cwd: &cwd_str,
        prompt: Some("audit"),
        resume: None,
        studio_dir: &studio_str,
        stage_env: &env,
        role: "actor-t1-c1",
        checks: &[],
        answer_trust: true,
    };

    // No runner emits plan flags (`plan`, `--plan`) or `read-only` / `auto` modes on `readonly`:
    // every runner stays in its normal working mode/sandbox so `backlot` commands execute
    // the same way as `standard`, paired with native file-edit deny rules where available.
    for runner in [
        "codex", "claude", "opencode", "cmd", "cursor", "agy", "devin", "cline", "fake",
    ] {
        let ro = runners::plan(&runner_id(runner, "readonly"), &ctx).unwrap();
        for forbidden in ["plan", "--plan", "read-only"] {
            assert!(
                !ro.argv.iter().any(|a| a == forbidden),
                "{runner} readonly unexpectedly emitted `{forbidden}`: {:?}",
                ro.argv
            );
        }
        assert!(
            !ro.argv
                .windows(2)
                .any(|w| w == ["--permission-mode", "auto"]),
            "{runner} readonly unexpectedly emitted `--permission-mode auto`: {:?}",
            ro.argv
        );
    }

    // codex -> keeps -s workspace-write and -a on-request (identical argv to standard)
    let codex_std = runners::plan(&runner_id("codex", "standard"), &ctx).unwrap();
    let codex = runners::plan(&runner_id("codex", "readonly"), &ctx).unwrap();
    assert_eq!(codex.argv, codex_std.argv);
    assert!(
        codex
            .argv
            .windows(2)
            .any(|w| w == ["-s", "workspace-write"]),
        "{:?}",
        codex.argv
    );

    // claude -> keeps --permission-mode manual + Bash(backlot *) + deny Edit/Write/NotebookEdit + drops git add/commit
    let claude = runners::plan(&runner_id("claude", "readonly"), &ctx).unwrap();
    assert!(
        claude
            .argv
            .windows(2)
            .any(|w| w == ["--permission-mode", "manual"]),
        "{:?}",
        claude.argv
    );
    let at = claude
        .argv
        .iter()
        .position(|a| a == "--allowedTools")
        .unwrap();
    assert!(
        claude.argv[at + 1]
            .split(',')
            .any(|r| r == "Bash(backlot *)"),
        "claude readonly must pre-allow backlot: {}",
        claude.argv[at + 1]
    );
    assert!(
        !claude.argv[at + 1].contains("git add") && !claude.argv[at + 1].contains("git commit")
    );
    let c_deny = serde_json::from_str::<serde_json::Value>(&claude.hooks.unwrap().content).unwrap();
    let c_arr: Vec<&str> = c_deny["permissions"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    for tool in ["AskUserQuestion", "Edit", "Write", "NotebookEdit"] {
        assert!(c_arr.contains(&tool), "claude missing {tool}: {c_arr:?}");
    }

    // opencode -> keeps normal argv + OPENCODE_CONFIG_CONTENT has permission.edit = deny
    let oc = runners::plan(&runner_id("opencode", "readonly"), &ctx).unwrap();
    let oc_json: serde_json::Value =
        serde_json::from_str(&oc.env["OPENCODE_CONFIG_CONTENT"]).unwrap();
    assert_eq!(oc_json["permission"]["edit"], "deny", "{oc_json}");

    // cmd -> keeps normal argv + Shell(backlot) allow + settings deny includes Edit/Write and drops git add/commit
    let cmd_l = runners::plan(&runner_id("cmd", "readonly"), &ctx).unwrap();
    let Some(Prep::Cmd(cmd_p)) = cmd_l.prep else {
        panic!("cmd prep")
    };
    assert!(
        cmd_p.allow.iter().any(|r| r == "Shell(backlot)"),
        "cmd readonly must pre-allow backlot: {:?}",
        cmd_p.allow
    );
    assert!(
        !cmd_p
            .allow
            .iter()
            .any(|r| r.contains("git add") || r.contains("git commit"))
    );
    runners::cmd::prepare(&cmd_p).unwrap();
    let cmd_v: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&cmd_p.settings).unwrap()).unwrap();
    let cmd_d: Vec<&str> = cmd_v["permissions"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        cmd_d.contains(&"Edit") && cmd_d.contains(&"Write"),
        "{cmd_d:?}"
    );

    // cursor -> keeps --trust (no --mode plan) + Shell(backlot) allow + deny Write(*)/Write(**) + drops git add/commit
    let cur_l = runners::plan(&runner_id("cursor", "readonly"), &ctx).unwrap();
    let cur_std = runners::plan(&runner_id("cursor", "standard"), &ctx).unwrap();
    assert_eq!(cur_l.argv, cur_std.argv);
    let Some(Prep::Cursor(cur_p)) = cur_l.prep else {
        panic!("cursor prep")
    };
    assert!(
        cur_p.allow.iter().any(|r| r == "Shell(backlot)"),
        "cursor readonly must pre-allow backlot: {:?}",
        cur_p.allow
    );
    assert!(
        !cur_p
            .allow
            .iter()
            .any(|r| r.contains("git add") || r.contains("git commit"))
    );
    runners::cursor::prepare(&cur_p).unwrap();
    let cur_cfg: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(cur_p.root.join("config/cli-config.json")).unwrap(),
    )
    .unwrap();
    let cur_d: Vec<&str> = cur_cfg["permissions"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        cur_d.contains(&"Write(*)") || cur_d.contains(&"Write(**)"),
        "{cur_d:?}"
    );

    // agy -> keeps --mode accept-edits + settings deny git writes (never backlot)
    let agy_l = runners::plan(&runner_id("agy", "readonly"), &ctx).unwrap();
    assert!(
        agy_l
            .argv
            .windows(2)
            .any(|w| w == ["--mode", "accept-edits"]),
        "{:?}",
        agy_l.argv
    );
    let Some(Prep::Agy(agy_p)) = agy_l.prep else {
        panic!("agy prep")
    };
    runners::agy::prepare(&agy_p).unwrap();
    let agy_cfg: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&agy_p.settings).unwrap()).unwrap();
    let agy_d: Vec<&str> = agy_cfg["permissions"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        agy_d.contains(&"command(git commit)") && agy_d.contains(&"command(git add)"),
        "{agy_d:?}"
    );
    assert!(
        !agy_d.iter().any(|r| r.contains("backlot")),
        "agy must never deny backlot: {agy_d:?}"
    );

    // devin -> keeps --permission-mode smart + same allow list as standard + config deny Write(*)
    let dev_l = runners::plan(&runner_id("devin", "readonly"), &ctx).unwrap();
    let dev_std = runners::plan(&runner_id("devin", "standard"), &ctx).unwrap();
    assert_eq!(dev_l.argv, dev_std.argv);
    assert!(
        dev_l
            .argv
            .windows(2)
            .any(|w| w == ["--permission-mode", "smart"]),
        "{:?}",
        dev_l.argv
    );
    let Some(Prep::Devin(dev_p)) = dev_l.prep else {
        panic!("devin prep")
    };
    let Some(Prep::Devin(dev_std_p)) = dev_std.prep else {
        panic!("devin std prep")
    };
    assert_eq!(dev_p.allow, dev_std_p.allow);
    runners::devin::prepare(&dev_p).unwrap();
    let dev_cfg: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&dev_p.config).unwrap()).unwrap();
    let dev_d: Vec<&str> = dev_cfg["permissions"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(dev_d.contains(&"Write(*)"), "{dev_d:?}");

    // cline -> keeps --yolo + hook keeps backlot & omits editor / git add / git commit
    let cln_l = runners::plan(&runner_id("cline", "readonly"), &ctx).unwrap();
    let cln_std = runners::plan(&runner_id("cline", "standard"), &ctx).unwrap();
    assert_eq!(cln_l.argv, cln_std.argv);
    assert!(cln_l.argv.iter().any(|a| a == "--yolo"), "{:?}", cln_l.argv);
    let Some(Prep::Cline(cln_p)) = cln_l.prep else {
        panic!("cline prep")
    };
    assert!(
        !cln_p.hook_content.contains("editor)")
            && !cln_p.hook_content.contains("[ \"$name\" = editor ]")
    );
    assert!(!cln_p.hook_content.contains("git commit") && !cln_p.hook_content.contains("git add"));
    let cln_hook = dir.join("cline-PreToolUse.sh");
    fs::write(&cln_hook, &cln_p.hook_content).unwrap();
    let run_cline_hook = |payload: &[u8]| -> String {
        let mut sh = Command::new("sh")
            .arg(&cln_hook)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        sh.stdin.take().unwrap().write_all(payload).unwrap();
        String::from_utf8(sh.wait_with_output().unwrap().stdout).unwrap()
    };
    let sh_edit =
        run_cline_hook(br#"{"tool_call":{"name":"editor","input":{"path":"src/lib.rs"}}}"#);
    assert!(
        sh_edit.contains(r#""cancel":true"#),
        "cline readonly hook allowed editor: {sh_edit}"
    );
    let deliver_call = serde_json::json!({
        "tool_call": {
            "name": "run_commands",
            "input": {
                "commands": [
                    "backlot deliver --report - <<'EOF'\n# Audit\nIt's clean.\nEOF",
                    "backlot progress \"auditing\""
                ]
            }
        }
    })
    .to_string();
    let sh_deliver = run_cline_hook(deliver_call.as_bytes());
    assert!(
        sh_deliver.contains(r#""cancel":false"#),
        "cline readonly hook must allow backlot deliver --report - and progress: {sh_deliver}"
    );

    // fake -> accepts readonly
    assert!(runners::plan(&runner_id("fake", "readonly"), &ctx).is_ok());
}
