//! Running a studio's `backlot` command on the machine that owns it.
//!
//! A remote action is one `terminal.run on: <machine>` through the
//! client's shared RPC connection — never ssh, never a local spawn.
//! `backlot` is not always on a remote core's PATH (a core started
//! over ssh has no `~/.local/bin`), so the runner locates it once per
//! machine with a short locate script and caches the absolute path.
//! A `spawn_failed` RPC error or a 126/127 exit drops the cache
//! entry, locates once more and retries once; nothing found reports
//! `backlot is not installed on <machine>` — U6c will offer to
//! install it there.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hc_proto::client::{CallError, Client};
use hc_proto::jsonrpc::error_kind;
use hc_proto::terminal::{RunParams, RunResult, TerminalParams};
use hc_proto::watch::OpenRpc;

use crate::app::BoxFut;

/// The runner the panel and Hollywood share for a studio on an added
/// machine. Clone-cheap: the client opener and the locate cache are
/// `Arc`s, so every `run` joins the same connection and cache.
#[derive(Clone)]
pub(crate) struct RemoteRunner {
    open: OpenRpc,
    /// `machine → absolute backlot path` — located once per machine
    /// per client run.
    located: Arc<Mutex<BTreeMap<String, String>>>,
}

/// The wait deadline one action's `terminal.run` gets; a `timeout`
/// result still removes the terminal.
const ACTION_WAIT: f64 = 30.0;
/// The locate run's deadline — the script's own login-shell lookup
/// carries a 3 s cap inside it.
const LOCATE_WAIT: f64 = 15.0;
/// A client-side guard past the core's wait report, so a wedged
/// machine cannot park an action forever.
const CALL_GUARD: Duration = Duration::from_secs(40);

/// The locate script: print one absolute `backlot` path or nothing —
/// `command -v`, then the login shell's lookup (3 s cap, last line
/// only), then `~/.local/bin/backlot` when executable.
pub(crate) const LOCATE_SCRIPT: &str = "p=$(command -v backlot 2>/dev/null); \
[ -n \"$p\" ] && { printf '%s\\n' \"$p\"; exit 0; }; \
s=${SHELL:-/bin/sh}; \
p=$(if command -v timeout >/dev/null 2>&1; then timeout 3 \"$s\" -lc 'command -v backlot' </dev/null 2>/dev/null; else \"$s\" -lc 'command -v backlot' </dev/null 2>/dev/null; fi | tail -n 1); \
[ -n \"$p\" ] && { printf '%s\\n' \"$p\"; exit 0; }; \
[ -x \"$HOME/.local/bin/backlot\" ] && printf '%s\\n' \"$HOME/.local/bin/backlot\"; \
exit 0";

impl RemoteRunner {
    pub(crate) fn new(open: OpenRpc) -> Self {
        Self {
            open,
            located: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    /// Run `backlot <argv>` on `machine`: locate, run waited, take
    /// the first output line as the failure text and always remove
    /// the terminal — a timeout included. The result is the same
    /// `(Ok, Err(line))` shape the local runner reports on `panel_rx`.
    pub(crate) fn run(&self, machine: String, argv: Vec<String>) -> BoxFut<Result<(), String>> {
        let open = self.open.clone();
        let located = self.located.clone();
        Box::pin(async move {
            let client = open().await.map_err(|e| format!("{e}"))?;
            run_on_machine(&client, &located, &machine, &argv).await
        })
    }
}

/// One action attempt, with one re-locate/retry for a stale path.
async fn run_on_machine(
    client: &Client,
    located: &Mutex<BTreeMap<String, String>>,
    machine: &str,
    argv: &[String],
) -> Result<(), String> {
    let Some(path) = locate(client, located, machine).await else {
        return Err(format!("backlot is not installed on {machine}"));
    };
    let out = run_waited(client, machine, vec![path], argv, ACTION_WAIT).await;
    match outcome(&out) {
        Outcome::Ok => {
            rm(client, &out).await;
            Ok(())
        }
        Outcome::NotFound => {
            // A stale cache entry: drop it, locate once, retry once.
            located.lock().expect("locate cache").remove(machine);
            let path = locate(client, located, machine).await;
            rm(client, &out).await;
            let Some(path) = path else {
                return Err(format!("backlot is not installed on {machine}"));
            };
            let retried = run_waited(client, machine, vec![path], argv, ACTION_WAIT).await;
            finish(client, machine, &retried).await
        }
        _ => finish(client, machine, &out).await,
    }
}

/// A finished run's verdict — `Ok` on exit 0, `NotFound` when the
/// binary was missing so the caller can re-locate, `Failed` with the
/// first non-empty output line otherwise.
enum Outcome {
    Ok,
    NotFound,
    Failed(String),
    TimedOut,
    Rpc(String),
}

/// A `terminal.run` call's shape: either the core answered (and the
/// terminal address is known) or the RPC itself failed — kept as the
/// structured `CallError` so a `spawn_failed` answer is known by
/// kind, not by message text.
enum RunOutcome {
    Ran(Box<RunResult>),
    Rpc(CallError),
    /// The client-side guard elapsed before any reply arrived.
    Guard,
}

/// Run `argv` on `machine` with `wait`; the core reports within
/// `wait_secs`, the client-side guard bounds a wedged call.
async fn run_waited(
    client: &Client,
    machine: &str,
    head: Vec<String>,
    tail: &[String],
    wait_secs: f64,
) -> RunOutcome {
    let argv: Vec<String> = head.into_iter().chain(tail.iter().cloned()).collect();
    let call = client.terminal_run(RunParams {
        argv,
        on: Some(machine.to_owned()),
        wait: true,
        timeout: Some(wait_secs),
        ..Default::default()
    });
    match tokio::time::timeout(CALL_GUARD, call).await {
        Ok(Ok(result)) => RunOutcome::Ran(Box::new(result)),
        Ok(Err(e)) => RunOutcome::Rpc(e),
        Err(_) => RunOutcome::Guard,
    }
}

/// Read the run's verdict. `NotFound` is only the binary being
/// missing — a `spawn_failed` RPC answer, or the shell's 126/127 —
/// never a plain backlot error that merely says "not found": those
/// keep their first output line as the failure text and never retry.
fn outcome(out: &RunOutcome) -> Outcome {
    match out {
        RunOutcome::Guard => Outcome::Rpc("terminal.run timed out".into()),
        RunOutcome::Rpc(e) => {
            if e.is_kind(error_kind::SPAWN_FAILED) {
                Outcome::NotFound
            } else {
                Outcome::Rpc(format!("{e}"))
            }
        }
        RunOutcome::Ran(result) => {
            let ended = result.wait.as_ref().is_some_and(|w| w.ended);
            let code = result
                .wait
                .as_ref()
                .and_then(|w| w.exit_code)
                .or(result.terminal.exit_code);
            if !ended {
                return Outcome::TimedOut;
            }
            match code {
                Some(0) => Outcome::Ok,
                Some(126) | Some(127) => Outcome::NotFound,
                _ => Outcome::Failed(first_line(result.output.as_deref().unwrap_or(""))),
            }
        }
    }
}

/// Report a finished run: clean removal on every path — a timed-out
/// run's terminal is killed first.
async fn finish(client: &Client, machine: &str, out: &RunOutcome) -> Result<(), String> {
    match outcome(out) {
        Outcome::Ok => {
            rm(client, out).await;
            Ok(())
        }
        Outcome::NotFound => {
            rm(client, out).await;
            Err(format!("backlot is not installed on {machine}"))
        }
        Outcome::Failed(line) => {
            rm(client, out).await;
            Err(line)
        }
        Outcome::TimedOut => {
            if let RunOutcome::Ran(result) = out {
                let _ = client
                    .terminal_kill(TerminalParams {
                        terminal: result.terminal.address.clone(),
                        caller: None,
                    })
                    .await;
            }
            rm(client, out).await;
            Err(format!("timed out after {} s", ACTION_WAIT as u64))
        }
        Outcome::Rpc(e) => Err(e),
    }
}

/// The first non-empty output line, or a fallback when the failure
/// printed nothing.
fn first_line(output: &str) -> String {
    output
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("command failed")
        .to_owned()
}

/// Remove the run's terminal — `not_found` after a kill is quiet.
async fn rm(client: &Client, out: &RunOutcome) {
    if let RunOutcome::Ran(result) = out {
        let _ = client
            .terminal_rm(TerminalParams {
                terminal: result.terminal.address.clone(),
                caller: None,
            })
            .await;
    }
}

/// The machine's `backlot` path: the cache, or a locate run that
/// returns the script's first printed absolute path.
async fn locate(
    client: &Client,
    located: &Mutex<BTreeMap<String, String>>,
    machine: &str,
) -> Option<String> {
    if let Some(path) = located.lock().expect("locate cache").get(machine) {
        return Some(path.clone());
    }
    let out = run_waited(
        client,
        machine,
        vec!["sh".into(), "-c".into(), LOCATE_SCRIPT.into()],
        &[],
        LOCATE_WAIT,
    )
    .await;
    let path = match &out {
        RunOutcome::Ran(result) => result
            .output
            .as_deref()
            .unwrap_or("")
            .lines()
            .map(str::trim)
            .find(|l| l.starts_with('/'))
            .map(str::to_owned),
        RunOutcome::Rpc(_) | RunOutcome::Guard => None,
    };
    rm(client, &out).await;
    if let Some(path) = &path {
        located
            .lock()
            .expect("locate cache")
            .insert(machine.to_owned(), path.clone());
    }
    path
}
