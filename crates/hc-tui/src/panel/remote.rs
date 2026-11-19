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
    /// — the remote tests' shape; actions use `run_output`.
    #[cfg(test)]
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

    /// Run `backlot <argv>` on `machine` and return its full output —
    /// the New Studio dialog's `repos --json`, `runners --json`,
    /// `--version` and `studio new` calls. `stdin`, when set, is
    /// base64'd into the run's shell line and piped to the program —
    /// how a generated cast.toml reaches `--cast -` on a remote
    /// machine without a stray file.
    pub(crate) fn run_output(
        &self,
        machine: String,
        argv: Vec<String>,
        stdin: Option<String>,
    ) -> BoxFut<Result<String, String>> {
        let open = self.open.clone();
        let located = self.located.clone();
        Box::pin(async move {
            let client = open().await.map_err(|e| format!("{e}"))?;
            output_on_machine(&client, &located, &machine, &argv, stdin.as_deref()).await
        })
    }
}

/// One action attempt, with one re-locate/retry for a stale path.
#[cfg(test)]
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

/// One output run, with the same one re-locate/retry a stale path
/// gets. `stdin` is piped through `base64 -d` inside the remote
/// shell line so no file is left on the target machine.
async fn output_on_machine(
    client: &Client,
    located: &Mutex<BTreeMap<String, String>>,
    machine: &str,
    argv: &[String],
    stdin: Option<&str>,
) -> Result<String, String> {
    let Some(path) = locate(client, located, machine).await else {
        return Err(format!("backlot is not installed on {machine}"));
    };
    let head = run_head(&path, stdin);
    let out = run_waited(client, machine, head, argv, ACTION_WAIT).await;
    match outcome(&out) {
        Outcome::Ok => {
            let text = ran_output(&out);
            rm(client, &out).await;
            Ok(text)
        }
        Outcome::NotFound => {
            located.lock().expect("locate cache").remove(machine);
            let path = locate(client, located, machine).await;
            rm(client, &out).await;
            let Some(path) = path else {
                return Err(format!("backlot is not installed on {machine}"));
            };
            let retried =
                run_waited(client, machine, run_head(&path, stdin), argv, ACTION_WAIT).await;
            finish_output(client, machine, &retried).await
        }
        _ => finish_output(client, machine, &out).await,
    }
}

/// The argv head for an output run: the bare backlot path, or a
/// `sh -c` wrapper piping the base64'd stdin into it.
fn run_head(path: &str, stdin: Option<&str>) -> Vec<String> {
    match stdin {
        None => vec![path.to_owned()],
        Some(data) => {
            let b64 = base64_encode(data.as_bytes());
            vec![
                "sh".into(),
                "-c".into(),
                format!("printf %s {b64} | base64 -d | \"$@\""),
                "sh".into(),
                path.to_owned(),
            ]
        }
    }
}

/// Minimal RFC 4648 base64 — the remote side decodes with the
/// ubiquitous `base64` tool, so no crate is needed here.
fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b = [
            c.first().copied().unwrap_or(0),
            c.get(1).copied().unwrap_or(0),
            c.get(2).copied().unwrap_or(0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// The captured output of a run that finished, empty on an RPC or
/// guard path.
fn ran_output(out: &RunOutcome) -> String {
    match out {
        RunOutcome::Ran(result) => result.output.clone().unwrap_or_default(),
        _ => String::new(),
    }
}

/// An output run's verdict: `Ok(output)` on exit 0 — the dialog
/// wants stdout, not a `()` — and the whole captured output as the
/// failure text otherwise, so a dry run's error is shown verbatim.
async fn finish_output(client: &Client, machine: &str, out: &RunOutcome) -> Result<String, String> {
    match outcome(out) {
        Outcome::Ok => {
            let text = ran_output(out);
            rm(client, out).await;
            Ok(text)
        }
        Outcome::NotFound => {
            rm(client, out).await;
            Err(format!("backlot is not installed on {machine}"))
        }
        Outcome::Failed => {
            let text = ran_output(out);
            rm(client, out).await;
            Err(if text.trim().is_empty() {
                "command failed".into()
            } else {
                text
            })
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

/// A finished run's verdict — `Ok` on exit 0, `NotFound` when the
/// binary was missing so the caller can re-locate, `Failed`
/// otherwise; callers re-read `out` for the failure line.
enum Outcome {
    Ok,
    NotFound,
    Failed,
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
                _ => Outcome::Failed,
            }
        }
    }
}

/// Report a finished run: clean removal on every path — a timed-out
/// run's terminal is killed first. Test-only: `run` is the tests'
/// shape; actions use `finish_output` for the output run.
#[cfg(test)]
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
        Outcome::Failed => {
            let line = first_line(&ran_output(out));
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
#[cfg(test)]
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
