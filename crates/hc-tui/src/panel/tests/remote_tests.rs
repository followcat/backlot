//! A remote studio action's wire: `terminal.run on: <machine>`, the
//! locate script's fall-through and the cleanup — driven through a
//! real client against a fake core that records every call.
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::panel::remote::RemoteRunner;
use crate::testcore::{FakeCore, minimal_terminal, openers, spawn_fake};

/// The methods the action's wire needs — `Client` refuses anything
/// `hello` did not advertise before the call leaves the process.
const REMOTE_METHODS: &[&str] = &[
    "hello",
    "terminal.run",
    "terminal.rm",
    "terminal.kill",
    "machine.list",
];

/// What the fake core saw: every `terminal.run`'s params and the
/// addresses `terminal.kill`/`terminal.rm` touched.
#[derive(Default)]
struct Calls {
    runs: Vec<Value>,
    kills: Vec<String>,
    rms: Vec<String>,
}

fn run_result(address: &str, exit: i32, output: &str) -> Value {
    // `RunResult` flattens `Terminal` — the fields sit inline beside
    // `output`/`wait`, not under a `terminal` key.
    let mut result = minimal_terminal();
    result["address"] = address.into();
    result["output"] = output.into();
    result["wait"] = json!({
        "terminal": address, "ended": true, "reason": "exit",
        "turn": 1, "exit_code": exit,
    });
    result
}

/// The locate run's answer — `output` is what the script printed.
fn locate_result(output: &str) -> Value {
    run_result("host-b/r1", 0, output)
}

/// Serve `terminal.run`/`rm`/`kill`: a run whose argv[0] is `sh` is
/// the locate script and gets `locate`, any other run gets the next
/// of `actions` (the last one repeating).
fn serve(
    calls: Arc<Mutex<Calls>>,
    locate: &'static str,
    actions: Vec<Value>,
) -> impl FnMut(FakeCore, bool) -> hc_proto::watch::BoxFuture<()> + Send + Sync + 'static {
    let actions = Arc::new(actions);
    move |mut core, framed| {
        let (calls, actions) = (calls.clone(), actions.clone());
        Box::pin(async move {
            if !framed {
                std::future::pending::<()>().await;
            }
            core.answer_hello(true, REMOTE_METHODS, &[]).await;
            let mut n = 0usize;
            while let Some(frame) = core.try_read_frame().await {
                if frame.channel != 0 || frame.kind != hc_proto::frame::Kind::Data {
                    continue;
                }
                let req: Value = serde_json::from_slice(&frame.payload).unwrap();
                let result = match req["method"].as_str().unwrap_or("") {
                    "terminal.run" => {
                        let params = req["params"].clone();
                        calls.lock().unwrap().runs.push(params.clone());
                        if params["argv"][0] == "sh" {
                            locate_result(locate)
                        } else {
                            let reply = actions[n.min(actions.len() - 1)].clone();
                            n += 1;
                            reply
                        }
                    }
                    "terminal.kill" => {
                        calls
                            .lock()
                            .unwrap()
                            .kills
                            .push(req["params"]["terminal"].as_str().unwrap_or("").into());
                        minimal_terminal()
                    }
                    "terminal.rm" => {
                        calls
                            .lock()
                            .unwrap()
                            .rms
                            .push(req["params"]["terminal"].as_str().unwrap_or("").into());
                        json!({"removed": true})
                    }
                    _ => Value::Null,
                };
                core.reply(&req["id"], result).await;
            }
        })
    }
}

/// A runner over the fake wire plus its recorded calls.
fn rig(
    locate: &'static str,
    actions: Vec<Value>,
) -> (RemoteRunner, Arc<Mutex<Calls>>, tokio::task::JoinHandle<()>) {
    let (accepted, rx) = mpsc::channel(4);
    let (rpc, _sub) = openers(accepted);
    let calls = Arc::new(Mutex::new(Calls::default()));
    let fake = spawn_fake(rx, serve(calls.clone(), locate, actions));
    (RemoteRunner::new(rpc), calls, fake)
}

#[tokio::test]
async fn a_remote_action_rides_terminal_run_on_the_machine() {
    let (runner, calls, fake) = rig(
        "/home/user/.local/bin/backlot\n",
        vec![run_result("host-b/r2", 0, "")],
    );
    runner
        .run(
            "host-b".into(),
            vec!["priority".into(), "--task".into(), "t1".into()],
        )
        .await
        .expect("the action succeeds");
    let calls = calls.lock().unwrap();
    // Locate, then the action — two runs, no other call but the rms.
    assert_eq!(calls.runs.len(), 2, "{:?}", calls.runs);
    let locate = &calls.runs[0];
    assert_eq!(locate["argv"][0], "sh");
    let action = &calls.runs[1];
    assert_eq!(action["on"], "host-b");
    assert_eq!(action["wait"], true);
    assert_eq!(
        action["argv"],
        json!(["/home/user/.local/bin/backlot", "priority", "--task", "t1"])
    );
    // No endpoint flags — the remote terminal reaches its own core.
    let argv: Vec<&str> = action["argv"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| a.as_str())
        .collect();
    assert!(!argv.contains(&"--session"), "{argv:?}");
    assert!(!argv.contains(&"--remote"), "{argv:?}");
    // The locate and the action terminals are both gone.
    assert_eq!(calls.rms.len(), 2, "{:?}", calls.rms);
    fake.abort();
}

#[tokio::test]
async fn the_located_path_is_cached_per_machine() {
    let (runner, calls, fake) = rig(
        "/home/user/.local/bin/backlot\n",
        vec![run_result("host-b/r2", 0, "")],
    );
    for _ in 0..2 {
        runner
            .run("host-b".into(), vec!["priority".into()])
            .await
            .expect("the action succeeds");
    }
    let calls = calls.lock().unwrap();
    // One locate for the first action only.
    assert_eq!(calls.runs.len(), 3, "{:?}", calls.runs);
    assert_eq!(calls.runs[1]["argv"][0], "/home/user/.local/bin/backlot");
    assert_eq!(calls.runs[2]["argv"][0], "/home/user/.local/bin/backlot");
    fake.abort();
}

#[tokio::test]
async fn a_127_relocates_once_and_retries() {
    let (runner, calls, fake) = rig(
        "/home/user/.local/bin/backlot\n",
        vec![
            run_result("host-b/r2", 127, "backlot: command not found"),
            run_result("host-b/r4", 0, ""),
        ],
    );
    runner
        .run("host-b".into(), vec!["reopen".into()])
        .await
        .expect("the retry succeeds");
    let calls = calls.lock().unwrap();
    // locate, action(127), locate again, action(ok).
    assert_eq!(calls.runs.len(), 4, "{:?}", calls.runs);
    assert_eq!(calls.runs[2]["argv"][0], "sh", "the retry re-locates");
    assert_eq!(calls.runs[3]["argv"][0], "/home/user/.local/bin/backlot");
    fake.abort();
}

#[tokio::test]
async fn no_backlot_on_the_machine_is_an_install_notice() {
    // The locate script prints nothing it found.
    let (runner, calls, fake) = rig("", vec![]);
    let err = runner
        .run("host-b".into(), vec!["priority".into()])
        .await
        .expect_err("nothing to run it with");
    assert_eq!(err, "backlot is not installed on host-b");
    let calls = calls.lock().unwrap();
    assert_eq!(calls.runs.len(), 1, "only the locate ran");
    assert_eq!(calls.rms.len(), 1, "the locate terminal is gone");
    fake.abort();
}

#[tokio::test]
async fn a_failed_action_toasts_its_first_output_line() {
    let (runner, calls, fake) = rig(
        "/home/user/.local/bin/backlot\n",
        vec![run_result("host-b/r2", 2, "\nno task t9\nmore detail\n")],
    );
    let err = runner
        .run("host-b".into(), vec!["priority".into()])
        .await
        .expect_err("the failure surfaces");
    assert_eq!(err, "no task t9");
    let calls = calls.lock().unwrap();
    assert_eq!(calls.rms.len(), 2, "locate and action both removed");
    fake.abort();
}

#[tokio::test]
async fn a_timed_out_run_is_killed_then_removed() {
    let mut hung = minimal_terminal();
    hung["address"] = "host-b/r2".into();
    hung["output"] = "".into();
    hung["wait"] = json!({
        "terminal": "host-b/r2", "ended": false,
        "reason": "timeout", "turn": 1,
    });
    let (runner, calls, fake) = rig("/home/user/.local/bin/backlot\n", vec![hung]);
    let err = runner
        .run("host-b".into(), vec!["reopen".into()])
        .await
        .expect_err("the wait timed out");
    assert!(err.contains("timed out"), "{err}");
    let calls = calls.lock().unwrap();
    assert_eq!(calls.kills, vec!["host-b/r2"], "{:?}", calls.kills);
    assert!(calls.rms.contains(&"host-b/r2".to_string()));
    fake.abort();
}

#[test]
fn the_locate_script_falls_through_path_login_shell_then_home() {
    // The order is the contract: `command -v`, the login shell's own
    // lookup capped at 3 s, then `~/.local/bin/backlot`.
    let script = crate::panel::remote::LOCATE_SCRIPT;
    let path = script.find("command -v backlot").unwrap();
    let login = script.find("-lc 'command -v backlot'").unwrap();
    let home = script.find("$HOME/.local/bin/backlot").unwrap();
    assert!(path < login && login < home, "{script}");
    assert!(script.contains("timeout 3"), "{script}");
}
