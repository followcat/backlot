//! Command-line front end: `--session`, `--remote`, `--help`,
//! `--version`, then the current-thread application over the resolved
//! endpoint. Help and version answer before anything connects.

use std::ffi::OsString;
use std::io::Write;
use std::sync::Arc;

use hc_proto::endpoint::{self, Endpoint, Env, Platform};
use hc_proto::watch::{WatchParams, Watcher};

use crate::app::{App, OpenAttach, Probe};

/// What the command line asked for.
pub enum Action {
    /// Print help; never connects.
    Help,
    /// Print the version; never connects.
    Version,
    /// Run the client.
    Run(Options),
}

/// Parsed run options.
pub struct Options {
    /// Session name argument (`--session`, else `HC_SESSION`/default).
    pub session: Option<OsString>,
    /// SSH target (`--remote`); a local endpoint when `None`.
    pub remote: Option<String>,
}

/// Parses `args` (the program's argv tail). Errors are user-facing.
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Action, String> {
    let mut options = Options {
        session: None,
        remote: None,
    };
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Action::Help),
            "-V" | "--version" => return Ok(Action::Version),
            "--session" => {
                let Some(value) = args.next() else {
                    return Err("--session needs a value".into());
                };
                options.session = Some(value.into());
            }
            "--remote" => {
                let Some(value) = args.next() else {
                    return Err("--remote needs a value".into());
                };
                options.remote = Some(value);
            }
            other if other.starts_with("--session=") => {
                options.session = Some(other["--session=".len()..].into());
            }
            other if other.starts_with("--remote=") => {
                options.remote = Some(other["--remote=".len()..].into());
            }
            other => return Err(format!("unexpected argument '{other}'")),
        }
    }
    Ok(Action::Run(options))
}

/// Version line for `--version`.
pub fn version_text() -> String {
    format!("backlot {}", env!("CARGO_PKG_VERSION"))
}

/// Long help for `--help`.
pub fn help_text() -> String {
    format!(
        "\
backlot {}
Terminal UI for herdr-core (hc) — shared layout and terminal client.

Usage: backlot [OPTIONS]

Options:
      --session <NAME>   Use session NAME (default: $HC_SESSION or \"default\")
      --remote <TARGET>  Connect over SSH: `ssh TARGET hc connect`; one
                         shared login and one attach connection per
                         target (login kept 10 minutes)
  -h, --help             Print this help
  -V, --version          Print the version

Default keys: Ctrl+B prefix — q detach · w workspace navigation ·
b sidebar · ? keybinds · Ctrl+B Ctrl+B literal ^B.
Herdr configuration supplies the prefix and bindings. Layout keys include
c new tab · v/- split · h/j/k/l focus · z zoom · r resize · x close.
Ctrl+G toggles keyboard lock. Herdr configuration can also bind custom commands.
Click the terminal-size notice to take control when another viewer may own the grid.
Everything else types into the focused terminal.",
        env!("CARGO_PKG_VERSION"),
    )
}

/// The hc terminal this client runs inside, verified against the
/// endpoint it talks to — derived from core's `client.rs::terminal_id`
/// (Apache-2.0): `HC_TERMINAL_ID` means nothing to another session's
/// core, so it counts only when `HC_TERMINAL_SOCKET` matches the
/// resolved LOCAL socket. A remote endpoint, a foreign socket, or a
/// bare `tN` on a different session never counts as self.
pub fn self_caller(env: &Env, endpoint: &Endpoint) -> Option<String> {
    let Endpoint::Local(local) = endpoint else {
        return None; // remote attach: never a local self
    };
    let socket = env
        .get(std::ffi::OsStr::new("HC_TERMINAL_SOCKET"))
        .map(std::ffi::OsString::as_os_str)?;
    if socket != local.socket.as_os_str() {
        return None; // a foreign session's socket
    }
    env.get(std::ffi::OsStr::new("HC_TERMINAL_ID"))
        .map(|id| id.to_string_lossy().into_owned())
        .filter(|id| !id.is_empty())
}

/// The one framed attach connection a remote endpoint multiplexes its
/// attaches onto — `None` for a local endpoint, whose attaches keep
/// opening their own sockets (`Attach::connect`). Watch and RPC stay
/// on their own connections either way.
fn shared_attach(
    endpoint: &Endpoint,
    ssh: &Option<OsString>,
) -> Option<hc_proto::client::AttachConn> {
    match endpoint {
        Endpoint::Remote(_) => Some(hc_proto::client::AttachConn::new(
            endpoint.clone(),
            ssh.clone(),
        )),
        Endpoint::Local(_) => None,
    }
}

/// Builds the app over the endpoint's real connections and runs it
/// until detach. Errors are user-facing strings — a failure leaves the
/// terminal restored by the caller's guard.
pub async fn run(endpoint: &Endpoint, env: &Env) -> Result<(), String> {
    // The user's herdr config drives prefix + keybindings, read-only.
    let config_path = crate::config::config_path();
    let loaded = crate::config::load(&config_path);
    let live = loaded.config.live_keybinds();
    let shell = crate::chrome::ClientShellConfig::from_config(
        &loaded.config,
        None,
        crate::config::config_diagnostic_summary(&loaded.diagnostics),
        config_path,
    );
    let params = WatchParams {
        traversal: Some(crate::project::agent_traversal),
        ..WatchParams::default()
    };
    let watcher = Watcher::connect(endpoint, None, params);
    let ssh: Option<OsString> = None;
    let caller = self_caller(env, endpoint);
    let attach_endpoint = endpoint.clone();
    let attach_conn = shared_attach(endpoint, &ssh);
    let open: OpenAttach = Arc::new(move |params| {
        let endpoint = attach_endpoint.clone();
        let conn = attach_conn.clone();
        let caller = caller.clone();
        Box::pin(async move {
            let mut params = params;
            // The verified local caller tags every attach — the core
            // refuses a terminal attaching to itself, so no nested
            // hc-tui can echo its own output back.
            params.caller = caller;
            match conn {
                // A remote attach rides the one framed connection every
                // remote attach shares — one ssh child, one `hello`.
                Some(conn) => conn.attach(&params).await,
                None => hc_proto::client::Attach::connect(&endpoint, None, &params).await,
            }
        })
    });
    let operation_endpoint = endpoint.clone();
    let operations: hc_proto::watch::OpenRpc = Arc::new(move || {
        let endpoint = operation_endpoint.clone();
        Box::pin(async move { hc_proto::client::Client::connect(&endpoint, None).await })
    });
    let mouse_capture = shell.mouse_capture;
    let mut app = App::new(
        watcher,
        open,
        live,
        shell,
        Arc::new(Probe::default()),
        crate::term::cursor_sink(),
    );

    app.set_operations(operations);

    // Guard declared before the terminal: the terminal drops first,
    // the guard restores raw mode/modes/cursor last — on success,
    // error and panic paths alike.
    let mut guard =
        crate::term::TermGuard::enter(mouse_capture).map_err(|e| format!("terminal setup: {e}"))?;
    let backend = crate::ansi::AnsiBackend::new(std::io::stdout());
    let mut terminal = ratatui::Terminal::new(backend).map_err(|e| format!("terminal: {e}"))?;
    // The single reader owns stdin: user input and the host's query
    // replies share it — write the queries first so the reader decodes
    // replies from its armed reply windows, never as keys.
    let cell_query = crate::host::send_theme_query().map_err(|e| format!("theme query: {e}"))?;
    let mut reader = crate::reader::spawn_reader(256, mouse_capture, true, cell_query)
        .map_err(|e| format!("input reader: {e}"))?;
    app.set_host_facts(crate::host::unprobed_facts());
    app.set_host_erase_byte(guard.erase_byte());
    // Host-mode control writes (keyboard pop/push) go through the real
    // stdout; the default sink is inert for tests.
    let input_control = reader.host_control();
    app.set_host_control(Box::new(move |bytes| {
        use std::io::Write;
        let mut out = std::io::stdout();
        out.write_all(bytes).and_then(|()| out.flush())?;
        input_control.written(bytes);
        Ok(())
    }));
    let result = app
        .run(&mut terminal, &mut reader.ui, &mut reader.replies)
        .await;
    reader.stop();
    let _ = guard.writer().flush();
    result.map_err(|e| e.to_string())?;
    if let Some(error) = app.failure() {
        return Err(error.to_string());
    }
    if let Some(error) = reader.failure() {
        return Err(format!("input reader: {error}"));
    }
    Ok(())
}

/// Resolves `--remote`/`--session` to an endpoint and runs the client.
pub async fn run_options(
    options: &Options,
    env: &Env,
    platform: &dyn Platform,
) -> Result<(), String> {
    let endpoint = endpoint::resolve(
        env,
        options.session.as_deref(),
        options.remote.as_deref(),
        platform,
    )
    .map_err(|e| format!("endpoint: {e}"))?;
    run(&endpoint, env).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use hc_proto::endpoint::{Local, Remote};
    use std::path::PathBuf;

    fn env_with(pairs: &[(&str, &str)]) -> Env {
        pairs
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect()
    }

    fn local(socket: &str) -> Endpoint {
        Endpoint::Local(Local {
            session: "scratch".into(),
            socket: PathBuf::from(socket),
            holder_socket: PathBuf::from(format!("{socket}.holder.sock")),
            state: PathBuf::from("/scratch/state"),
            config: PathBuf::from("/scratch/config.toml"),
        })
    }

    /// The id counts only when socket AND id both match the local core.
    #[test]
    fn caller_counts_only_on_a_matching_local_socket() {
        let env = env_with(&[
            ("HC_TERMINAL_ID", "t7"),
            ("HC_TERMINAL_SOCKET", "/run/scratch/hc.sock"),
        ]);
        assert_eq!(
            self_caller(&env, &local("/run/scratch/hc.sock")),
            Some("t7".into())
        );
        // A foreign session's socket: the id means nothing there.
        assert_eq!(self_caller(&env, &local("/run/other/hc.sock")), None);
    }

    /// A remote endpoint never inherits a local caller — an equal `tN`
    /// on the other machine is a different terminal.
    #[test]
    fn caller_never_crosses_a_remote_endpoint() {
        let env = env_with(&[
            ("HC_TERMINAL_ID", "t7"),
            ("HC_TERMINAL_SOCKET", "/run/scratch/hc.sock"),
        ]);
        let remote = Endpoint::Remote(Remote {
            target: "host-a".into(),
            session: "scratch".into(),
        });
        assert_eq!(self_caller(&env, &remote), None);
    }

    /// Only a remote endpoint's attaches multiplex onto the shared
    /// framed connection; a local endpoint keeps `Attach::connect`.
    #[test]
    fn only_a_remote_endpoint_gets_the_shared_attach_connection() {
        let remote = Endpoint::Remote(Remote {
            target: "host-a".into(),
            session: "scratch".into(),
        });
        assert!(shared_attach(&remote, &None).is_some());
        assert!(shared_attach(&local("/run/scratch/hc.sock"), &None).is_none());
    }

    /// Missing socket proof (or missing id) is not a caller.
    #[test]
    fn caller_needs_the_verifying_socket_var() {
        let only_id = env_with(&[("HC_TERMINAL_ID", "t7")]);
        assert_eq!(self_caller(&only_id, &local("/run/scratch/hc.sock")), None);
        let only_socket = env_with(&[("HC_TERMINAL_SOCKET", "/run/scratch/hc.sock")]);
        assert_eq!(
            self_caller(&only_socket, &local("/run/scratch/hc.sock")),
            None
        );
    }
}
