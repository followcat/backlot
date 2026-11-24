//! Command-line front end: `--session`, `--remote`, `--help`,
//! `--version`, then the current-thread application over the resolved
//! endpoint. Help and version answer before anything connects.

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use hc_proto::endpoint::{self, Endpoint, Env, Platform};
use hc_proto::watch::{WatchParams, Watcher};

use crate::app::{App, HostOutput, OpenAttach, Probe};

/// What the command line asked for.
pub enum Action {
    /// Print help; never connects.
    Help,
    /// Print the version; never connects.
    Version,
    /// Run the official installer to upgrade; never connects.
    Upgrade,
    /// Run the client.
    Run(Options),
}

/// Parsed run options.
pub struct Options {
    /// Session name argument (`--session`, else `HC_SESSION`/default).
    pub session: Option<OsString>,
    /// SSH target (`--remote`); a local endpoint when `None`.
    pub remote: Option<String>,
    /// `--plain`: the plain multiplexer from `main` — no studio mode,
    /// no opening page. For the parity harness and troubleshooting;
    /// general users wanting this shape should run `hc-tui`.
    pub plain: bool,
}

/// Parses `args` (the program's argv tail). Errors are user-facing.
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Action, String> {
    let mut options = Options {
        session: None,
        remote: None,
        plain: false,
    };
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Action::Help),
            "-V" | "--version" => return Ok(Action::Version),
            "--upgrade" => return Ok(Action::Upgrade),
            "--plain" => options.plain = true,
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

/// A `--version` line: `backlot 0.3.0`, with the build in parentheses
/// when the build provided one — `backlot 0.3.0 (abc123def456)`.
pub fn version_line(version: &str, build: Option<&str>) -> String {
    match build.map(str::trim).filter(|build| !build.is_empty()) {
        Some(build) => format!("backlot {version} ({build})"),
        None => format!("backlot {version}"),
    }
}

/// Version line for `--version`.
pub fn version_text() -> String {
    version_line(env!("CARGO_PKG_VERSION"), option_env!("BACKLOT_BUILD_ID"))
}

pub const INSTALL_URL: &str =
    "https://github.com/followcat/backlot/releases/latest/download/install.sh";

/// The documented installer one-liner — the same URL `docs/install.md`
/// publishes and [`INSTALL_URL`] names. Kept together so `--upgrade`,
/// the update card and the notice text can't drift apart.
pub const INSTALL_COMMAND: &str =
    "curl -fsSL https://github.com/followcat/backlot/releases/latest/download/install.sh | sh";

/// `backlot --upgrade`: a user-invoked network action, not a background
/// call — the official installer runs with stdio inherited so its
/// progress shows, and the exit code propagates. A package-managed
/// install or dev build is refused before anything runs (issue #44);
/// an explicit `HC_TUI_INSTALL_DIR` is the user's own choice.
pub fn upgrade() -> std::process::ExitCode {
    let configured = std::env::var_os("HC_TUI_INSTALL_DIR");
    let source = crate::install_source::current(|key| std::env::var_os(key));
    let dir = crate::update_install::upgrade_install_dir(configured.as_deref(), None);
    let code = upgrade_with(&source, configured.as_deref(), dir, &run_installer);
    if code == 0 {
        println!("restart backlot to use the new version");
    }
    std::process::ExitCode::from(code)
}

/// The `--upgrade` decision, with the installer injected so tests never
/// spawn it. An explicit non-empty `BACKLOT_INSTALL_DIR` runs as asked; a
/// managed or dev-build binary refuses with its own command; a direct
/// install upgrades `direct_dir`, the running binary's directory.
pub fn upgrade_with(
    source: &crate::install_source::InstallSource,
    configured_dir: Option<&std::ffi::OsStr>,
    direct_dir: Option<PathBuf>,
    run: &dyn Fn(Option<PathBuf>) -> u8,
) -> u8 {
    if configured_dir.is_some_and(|dir| !dir.is_empty()) {
        return run(None);
    }
    if let Some(refusal) = source.refusal() {
        eprintln!("{refusal}");
        return 1;
    }
    run(direct_dir)
}

/// Run the official installer with stdio inherited, optionally into
/// `dir` (`BACKLOT_INSTALL_DIR`); its exit code propagates.
fn run_installer(dir: Option<PathBuf>) -> u8 {
    let mut command = std::process::Command::new("sh");
    command
        .args(["-c", INSTALL_COMMAND])
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());
    if let Some(dir) = dir {
        command.env("BACKLOT_INSTALL_DIR", dir);
    }
    match command.status() {
        Ok(status) => status
            .code()
            .map_or(1, |code| u8::try_from(code).unwrap_or(1)),
        Err(e) => {
            eprintln!("error: upgrade: {e}");
            1
        }
    }
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
      --plain            Plain multiplexer — no studio mode, no opening
                         page (hc-tui is the general client)
  -h, --help             Print this help
  -V, --version          Print the version
      --upgrade          Run the official installer into this binary's
                         directory (BACKLOT_INSTALL_DIR overrides) to upgrade
                         to the latest release, then restart backlot; a
                         package-managed or dev build is refused instead

Default keys: Ctrl+B prefix — q detach · w workspace navigation ·
b sidebar · ? keybinds · Ctrl+B Ctrl+B literal ^B.
Herdr configuration supplies the prefix and bindings. Layout keys include
c new tab · v/- split · h/j/k/l focus · z zoom · r resize · x close.
Ctrl+G toggles keyboard lock. Herdr configuration can also bind custom commands.
Click the terminal-size notice to take control when another viewer may own the grid.
Everything else types into the focused terminal.
Studio commands (`backlot studio`, `req`, `task`, `deliver`, `review`, `status`, `records`, …)
run before this UI; 'backlot <command> --help' shows each one's usage.
docs/studio/guide.md walks a studio end to end.",
        env!("CARGO_PKG_VERSION"),
    )
}

/// The watcher's parameters: the reference's traversal order, plus
/// backlot's own `backlot` document. `documents` is the one place the
/// studio names it.
///
/// It stays out of `doc_patterns`: on 0.1.21 and later cores, naming
/// documents in `events.subscribe` narrows the subscription to those
/// documents alone and silences terminal events, and the studio needs
/// every terminal event. `doc_patterns` therefore stays empty while
/// backlot wants every terminal (docs/fork-features.md, "Named document
/// subscriptions").
pub(crate) fn watch_params() -> WatchParams {
    let mut params = WatchParams {
        traversal: Some(crate::project::agent_traversal),
        ..WatchParams::default()
    };
    // Every attached machine serves its own stage's summary under
    // `DocKey { machine, name }`.
    params.documents.push("backlot".into());
    params
}

/// The one framed RPC connection every consumer on `endpoint` shares —
/// the watcher's refreshes, layout actions and background seen writes
/// all multiplex onto it (one transport, one `hello`): lazy like
/// `AttachConn`, single-flight, rebuilt once when the wire dies.
fn shared_rpc(
    endpoint: &Endpoint,
    ssh: &Option<OsString>,
    stats: Arc<hc_proto::stats::ConnStats>,
) -> hc_proto::watch::OpenRpc {
    hc_proto::client::shared_rpc_counted(endpoint.clone(), ssh.clone(), stats)
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

/// The one framed attach connection every attach on `endpoint`
/// multiplexes onto: a remote's attaches share one ssh child, a
/// local's share one Unix socket — either way a burst of attaches
/// costs one transport and one `hello`. Watch and RPC stay on their
/// own connections either way.
fn shared_attach(endpoint: &Endpoint, ssh: &Option<OsString>) -> hc_proto::client::AttachConn {
    hc_proto::client::AttachConn::new(endpoint.clone(), ssh.clone())
}

/// Builds the app over the endpoint's real connections and runs it
/// until detach. `plain` keeps the client exactly as `main` outside
/// studio mode: Herdr's agents list, keys and layout — no studio
/// cast, no opening page. Errors are user-facing strings — a failure
/// leaves the terminal restored by the caller's guard. `Some` is the
/// restart path an in-client update asked for.
pub async fn run(endpoint: &Endpoint, env: &Env, plain: bool) -> Result<Option<PathBuf>, String> {
    // The user's herdr config drives prefix + keybindings, read-only.
    let config_path = crate::config::config_path();
    let mut loaded = crate::config::load(&config_path);
    // backlot's own `[theme]` goes over herdr's (`own_theme.rs`).
    let own_theme =
        crate::own_theme::Shell::at(crate::panel::config_path(env)).overlay(&mut loaded);
    let live = loaded.config.live_keybinds();
    let shell = crate::chrome::ClientShellConfig::from_config(
        &loaded.config,
        None,
        crate::config::config_diagnostic_summary(&loaded.diagnostics),
        config_path,
    );
    // Chrome preferences key on the endpoint's own identity — the
    // local core's socket, or a remote's target + session — so two
    // endpoints never share a file.
    let mut shell = match endpoint {
        Endpoint::Local(local) => shell.with_local_endpoint(env, &local.socket),
        Endpoint::Remote(remote) => {
            shell.with_remote_endpoint(env, &remote.target, &remote.session)
        }
    };
    shell.own_theme = own_theme;
    let params = watch_params();
    // One framed RPC wire: the watcher's refreshes and the operations
    // + seen workers' calls all ride clones of this single opener.
    let ssh: Option<OsString> = None;
    let rpc_stats = Arc::new(hc_proto::stats::ConnStats::default());
    let sub_stats = Arc::new(hc_proto::stats::ConnStats::default());
    let rpc = shared_rpc(endpoint, &ssh, Arc::clone(&rpc_stats));
    let sub =
        hc_proto::watch::subscription_opener(endpoint, ssh.as_deref(), Arc::clone(&sub_stats));
    let watcher = Watcher::with_openers(rpc.clone(), sub, params);
    let caller = self_caller(env, endpoint);
    // The verified host pane id also goes to the app: every "which
    // pane next" fallback skips it — attaching to it is the refused
    // self-attach the core answers with an error card.
    let own = caller.clone();
    let attach_conn = shared_attach(endpoint, &ssh);
    let attach_for_diag = attach_conn.clone();
    let open: OpenAttach = Arc::new(move |params| {
        let conn = attach_conn.clone();
        let caller = caller.clone();
        Box::pin(async move {
            let mut params = params;
            // The verified local caller tags every attach — the core
            // refuses a terminal attaching to itself, so no nested
            // backlot can echo its own output back.
            params.caller = caller;
            // Every attach rides the one framed connection — remote or
            // local, a burst of attaches shares one transport + `hello`.
            conn.attach(&params).await
        })
    });
    let mouse_capture = shell.mouse_capture;
    let probe = Arc::new(Probe {
        publish_panes: crate::diag::requested(env),
        ..Probe::default()
    });
    let mut app = App::new(watcher, open, live, shell, Arc::clone(&probe));
    app.set_plain(plain);
    app.set_own_pane_id(own);

    app.set_operations(rpc.clone());
    // A studio on an added machine runs its `backlot` there — one
    // `terminal.run on:` through this same shared RPC connection.
    app.set_remote_runner(crate::panel::RemoteRunner::new(rpc));
    // `endpoint_accepts_local_images` client arms: a `--remote`
    // endpoint, or this client itself running inside an SSH session.
    app.set_image_paste_context(
        matches!(endpoint, Endpoint::Remote(_)),
        env.get(std::ffi::OsStr::new("SSH_CONNECTION")).is_some()
            || env.get(std::ffi::OsStr::new("SSH_TTY")).is_some(),
    );

    // The studio panel's own config — herdr's `config.toml` load above
    // is untouched. A malformed file is defaults plus the position.
    let (panel_config, panel_notice) = crate::panel::load(&crate::panel::config_path(env));
    app.set_panel_config(panel_config, panel_notice, &crate::panel::config_path(env));
    app.set_panel_endpoint(endpoint);

    // The `[hollywood]` table (H4) from the same config file — the
    // layer starts collapsed with it and stays hidden without it, so
    // Alt+Y keeps reaching the program exactly as before (D15a).
    let env_map: std::collections::BTreeMap<String, String> = env
        .iter()
        .filter_map(|(k, v)| Some((k.to_str()?.to_string(), v.to_str()?.to_string())))
        .collect();
    let hwood_path = backlot_stage::config::config_path(&env_map);
    app.set_hollywood(backlot_stage::config::hollywood(&env_map), &hwood_path);
    // The settings tab's `<r>` — a PATH scan only, never a probe.
    app.set_setup_runner_hint(backlot_stage::detect::first(&backlot_stage::Env {
        vars: env_map.clone(),
    }));

    // Guard declared before the terminal: the terminal drops first,
    // the guard restores raw mode/modes/cursor last — on success,
    // error and panic paths alike.
    let mut guard =
        crate::term::TermGuard::enter(mouse_capture).map_err(|e| format!("terminal setup: {e}"))?;
    let backend =
        crate::ansi::AnsiBackend::new(HostOutput::new(std::io::stdout(), Arc::clone(&probe)));
    let mut terminal = ratatui::Terminal::new(backend).map_err(|e| format!("terminal: {e}"))?;
    // The single reader owns stdin: user input and the host's query
    // replies share it — write the queries first so the reader decodes
    // replies from its armed reply windows, never as keys.
    let cell_query = crate::host::send_theme_query().map_err(|e| format!("theme query: {e}"))?;
    let mut reader =
        crate::reader::spawn_reader(256, mouse_capture, true, cell_query, Arc::clone(&probe))
            .map_err(|e| format!("input reader: {e}"))?;
    app.set_host_facts(crate::host::unprobed_facts());
    app.set_host_erase_byte(guard.erase_byte());
    // Host-mode control writes (keyboard pop/push) go through the real
    // stdout; the default sink is inert for tests.
    let input_control = reader.host_control();
    let control_probe = Arc::clone(&probe);
    app.set_host_control(Box::new(move |bytes| {
        use std::io::Write;
        let mut out = HostOutput::new(std::io::stdout(), Arc::clone(&control_probe));
        out.write_all(bytes).and_then(|()| out.flush())?;
        input_control.written(bytes);
        Ok(())
    }));
    // The install channel gates in-place upgrades: a package-managed
    // or dev-build binary never gets `↵ update` (issue #44).
    app.set_install_source(crate::install_source::current(|key| {
        env.get(std::ffi::OsStr::new(key)).map(OsString::from)
    }));
    // Terminal setup has succeeded — a run with no terminal already
    // failed above and starts nothing. Kick the one-shot local core
    // start; its report lands as a notice only when it fails.
    app.set_core_start(crate::core_start::kick(endpoint, env));
    // The sanctioned one-off release check: one curl HEAD, bounded and
    // silent — `update.version_check = false` or HC_TUI_NO_UPDATE_CHECK
    // disarm it before anything spawns.
    app.set_update_check(crate::update_check::kick(
        env,
        loaded.config.update.version_check,
    ));
    let diag = crate::diag::Diag::start(
        env,
        crate::diag::Sources::new(
            Arc::clone(&probe),
            endpoint,
            sub_stats,
            rpc_stats,
            attach_for_diag,
        ),
    );
    let result = app
        .run(&mut terminal, &mut reader.ui, &mut reader.replies)
        .await;
    let restart = app.take_restart();
    if let Some(diag) = diag {
        diag.finish();
    }
    reader.stop();
    let _ = guard.writer().flush();
    result.map_err(|e| e.to_string())?;
    if let Some(error) = app.failure() {
        return Err(error.to_string());
    }
    if let Some(error) = reader.failure() {
        return Err(format!("input reader: {error}"));
    }
    Ok(restart)
}

/// Resolves `--remote`/`--session` to an endpoint and runs the client.
pub async fn run_options(
    options: &Options,
    env: &Env,
    platform: &dyn Platform,
) -> Result<Option<PathBuf>, String> {
    let endpoint = endpoint::resolve(
        env,
        options.session.as_deref(),
        options.remote.as_deref(),
        platform,
    )
    .map_err(|e| format!("endpoint: {e}"))?;
    run(&endpoint, env, options.plain).await
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
