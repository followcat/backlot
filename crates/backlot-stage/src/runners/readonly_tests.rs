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

    // codex -> -s read-only
    let codex = runners::plan(&runner_id("codex", "readonly"), &ctx).unwrap();
    assert!(
        codex.argv.windows(2).any(|w| w == ["-s", "read-only"]),
        "{:?}",
        codex.argv
    );

    // claude -> --permission-mode plan + deny Edit/Write/NotebookEdit + no git add/commit allow
    let claude = runners::plan(&runner_id("claude", "readonly"), &ctx).unwrap();
    assert!(
        claude
            .argv
            .windows(2)
            .any(|w| w == ["--permission-mode", "plan"]),
        "{:?}",
        claude.argv
    );
    let at = claude
        .argv
        .iter()
        .position(|a| a == "--allowedTools")
        .unwrap();
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

    // opencode -> OPENCODE_CONFIG_CONTENT has permission.edit = deny
    let oc = runners::plan(&runner_id("opencode", "readonly"), &ctx).unwrap();
    let oc_json: serde_json::Value =
        serde_json::from_str(&oc.env["OPENCODE_CONFIG_CONTENT"]).unwrap();
    assert_eq!(oc_json["permission"]["edit"], "deny", "{oc_json}");

    // cmd -> settings deny includes Edit/Write and allow drops git add/commit
    let cmd_l = runners::plan(&runner_id("cmd", "readonly"), &ctx).unwrap();
    let Some(Prep::Cmd(cmd_p)) = cmd_l.prep else {
        panic!("cmd prep")
    };
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

    // cursor -> --mode plan + deny Write(*)/Write(**) + allow drops git add/commit
    let cur_l = runners::plan(&runner_id("cursor", "readonly"), &ctx).unwrap();
    assert!(
        cur_l.argv.windows(2).any(|w| w == ["--mode", "plan"]),
        "{:?}",
        cur_l.argv
    );
    let Some(Prep::Cursor(cur_p)) = cur_l.prep else {
        panic!("cursor prep")
    };
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

    // agy -> --mode plan + settings deny git writes
    let agy_l = runners::plan(&runner_id("agy", "readonly"), &ctx).unwrap();
    assert!(
        agy_l.argv.windows(2).any(|w| w == ["--mode", "plan"]),
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

    // devin -> --permission-mode auto + config deny Write(*)
    let dev_l = runners::plan(&runner_id("devin", "readonly"), &ctx).unwrap();
    assert!(
        dev_l
            .argv
            .windows(2)
            .any(|w| w == ["--permission-mode", "auto"]),
        "{:?}",
        dev_l.argv
    );
    let Some(Prep::Devin(dev_p)) = dev_l.prep else {
        panic!("devin prep")
    };
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

    // cline -> --plan + hook omits editor and drops git add/commit (and cancels editor at runtime)
    let cln_l = runners::plan(&runner_id("cline", "readonly"), &ctx).unwrap();
    assert!(
        cln_l.argv.iter().any(|a| a == "--plan" || a == "-p"),
        "{:?}",
        cln_l.argv
    );
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
    let mut sh = Command::new("sh")
        .arg(&cln_hook)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    sh.stdin
        .take()
        .unwrap()
        .write_all(br#"{"tool_call":{"name":"editor","input":{"path":"src/lib.rs"}}}"#)
        .unwrap();
    let sh_out = String::from_utf8(sh.wait_with_output().unwrap().stdout).unwrap();
    assert!(
        sh_out.contains(r#""cancel":true"#),
        "cline readonly hook allowed editor: {sh_out}"
    );

    // fake -> accepts readonly
    assert!(runners::plan(&runner_id("fake", "readonly"), &ctx).is_ok());
}
