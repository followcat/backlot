// Derived from herdr-core crates/herdr-core/src/{paths,config,machines}.rs and
// platform/unix.rs at ec8adcdf (Apache-2.0).
//
//! Where a core's endpoint comes from. Pure resolution from explicitly given
//! environment values, a session override and a [`Platform`] view of the
//! filesystem: nothing here connects, creates a directory or starts a core.

use crate::jsonrpc::RpcError;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};

/// The environment discovery sees: variable name to value. Production code
/// fills it with [`env_snapshot`]; tests build a map by hand so nothing in a
/// test mutates or depends on the process environment.
pub type Env = BTreeMap<OsString, OsString>;

/// This process's environment as an [`Env`].
pub fn env_snapshot() -> Env {
    std::env::vars_os().collect()
}

fn get<'e>(env: &'e Env, name: &str) -> Option<&'e OsStr> {
    env.get(OsStr::new(name)).map(OsString::as_os_str)
}

/// What of the outside world resolution is allowed to look at. Implementations
/// answer questions only; [`Sys`] is the real one, tests use a fake.
pub trait Platform {
    /// The per-user runtime directory — `/run/user/<uid>` — when it is a
    /// directory. `None` on systems without one (and always off Windows).
    /// The uid comes from the system call, never from a shell variable.
    fn run_user_dir(&self) -> Option<PathBuf>;
    /// Reads a file to text; `NotFound` when it does not exist.
    fn read_file(&self, path: &Path) -> io::Result<String>;
}

/// The real filesystem and uid.
pub struct Sys;

impl Platform for Sys {
    fn run_user_dir(&self) -> Option<PathBuf> {
        #[cfg(unix)]
        {
            // Logins without XDG_RUNTIME_DIR (some ssh setups) must still
            // find the core — the same fallback core uses.
            let path = PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() }));
            path.is_dir().then_some(path)
        }
        #[cfg(not(unix))]
        None
    }

    fn read_file(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }
}

/// The longest Unix socket path: `sockaddr_un.sun_path` minus the NUL —
/// 107 on Linux, 103 on macOS. Other systems have no Unix socket bound.
#[cfg(unix)]
pub const MAX_SOCKET_PATH: usize = {
    let addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    addr.sun_path.len() - 1
};
#[cfg(not(unix))]
pub const MAX_SOCKET_PATH: usize = usize::MAX;

/// Hex digits in a holder's socket name; the longest path the holder needs.
pub const HOLDER_ID: usize = 8;

/// The session name goes into folder and socket names. A `--session`
/// argument wins over `HC_SESSION`; spaces around the name are dropped
/// (cmd's `set X=a & ...` keeps the one before `&`); empty means `default`.
pub fn session(env: &Env, argument: Option<&OsStr>) -> Result<String, RpcError> {
    let raw = argument
        .or_else(|| get(env, "HC_SESSION"))
        .and_then(|v| v.to_str());
    let name = raw
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("default");
    if name == "."
        || name == ".."
        || name
            .chars()
            .any(|c| c.is_control() || r#"/\:*?"<>|"#.contains(c))
    {
        return Err(RpcError::new(
            -32000,
            "usage",
            format!("session name {name:?} can't be a folder name"),
            "use letters, digits, - _ .",
        ));
    }
    Ok(name.into())
}

/// `config.toml` as core validates it: every field it knows, at the type it
/// expects. A known field with the wrong type fails the whole parse — so the
/// `socket` override is ignored exactly when core's `Config::load` would
/// fail — while fields core does not know are tolerated, as core does.
/// A missing, unreadable or unparsable file means no socket.
#[derive(Deserialize)]
struct ConfigFile {
    /// Executable and arguments for new shells.
    #[allow(dead_code)]
    shell: Option<Vec<String>>,
    /// The `hc attach` detach chord.
    #[allow(dead_code)]
    detach_key: Option<String>,
    /// Seconds a `wait` runs before it checks in.
    #[allow(dead_code)]
    wait_timeout: Option<f64>,
    /// `<hc>` notes and parent-agent notes, on or off.
    #[allow(dead_code)]
    notes: Option<bool>,
    /// Quiet seconds before a note goes in.
    #[allow(dead_code)]
    notes_quiet: Option<f64>,
    /// Size of a terminal nobody is typing in, `COLSxROWS`.
    #[allow(dead_code)]
    headless: Option<String>,
    /// Scrollback kept per live terminal, in MB.
    #[allow(dead_code)]
    history_mb: Option<f64>,
    /// Where the core listens; `HC_SOCKET` wins over this.
    socket: Option<String>,
    /// Seconds for machine answer/redial timeouts.
    #[allow(dead_code)]
    machine_timeout: Option<f64>,
}

/// A local core's endpoint: the socket to dial and the paths that chose it.
#[derive(Debug, Clone, PartialEq)]
pub struct Local {
    /// The session this core serves.
    pub session: String,
    /// The client socket, from `HC_SOCKET`, the `socket` config key or the
    /// runtime directory.
    pub socket: PathBuf,
    /// The holder socket beside it; the length check covers its longest name.
    pub holder_socket: PathBuf,
    /// The session state directory.
    pub state: PathBuf,
    /// The config file discovery read.
    pub config: PathBuf,
}

/// A remote core reached over ssh: a validated target and the fixed remote
/// command `hc connect [--session <name>]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Remote {
    /// The ssh target as the user wrote it (`host`, `user@host`, `ssh://…`,
    /// `[v6]`); passed to ssh as a single argument.
    pub target: String,
    /// The session to reach on that machine.
    pub session: String,
}

/// Where this client can reach a core.
#[derive(Debug, Clone, PartialEq)]
pub enum Endpoint {
    /// A socket on this machine.
    Local(Local),
    /// `ssh <target> hc connect …`.
    Remote(Remote),
}

/// The endpoint for a `--session`/`--remote` choice over `env`. A remote
/// target ignores every local socket rule (the session name still applies).
pub fn resolve(
    env: &Env,
    argument: Option<&OsStr>,
    remote_target: Option<&str>,
    platform: &dyn Platform,
) -> Result<Endpoint, RpcError> {
    let session = session(env, argument)?;
    match remote_target {
        Some(target) => remote(target, &session).map(Endpoint::Remote),
        None => local(env, &session, platform).map(Endpoint::Local),
    }
}

/// The local endpoint for `session`, following core's precedence:
/// nonempty `HC_SOCKET`, else `socket` in `config.toml`, else the runtime
/// dir, else the session state dir.
pub fn local(env: &Env, session: &str, platform: &dyn Platform) -> Result<Local, RpcError> {
    let xdg = |var: &str, fallback: PathBuf| {
        get(env, var)
            .filter(|v| !v.is_empty())
            .map_or(fallback, PathBuf::from)
    };
    let home = get(env, "HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
    let state = xdg("XDG_STATE_HOME", home.join(".local/state"))
        .join("herdr-core")
        .join(session);
    let config = xdg("XDG_CONFIG_HOME", home.join(".config"))
        .join("herdr-core")
        .join("config.toml");

    let chosen = get(env, "HC_SOCKET")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| config_socket(&config, platform));
    let (socket, holder_socket) = match chosen {
        Some(socket) => (socket.clone(), socket.with_extension("holder.sock")),
        None => {
            let run = get(env, "XDG_RUNTIME_DIR")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .or_else(|| platform.run_user_dir())
                .map_or_else(|| state.clone(), |run| run.join("herdr-core"));
            (
                run.join(format!("{session}.sock")),
                run.join(format!("{session}.holder.sock")),
            )
        }
    };
    // Holders listen on `<holder minus .sock>.<id>.sock`: the longest name.
    let longest = holder_socket.as_os_str().len() + 1 + HOLDER_ID;
    if socket.as_os_str().len().max(longest) > MAX_SOCKET_PATH {
        return Err(RpcError::new(
            -32000,
            "usage",
            format!(
                "the socket path for session {session:?} is too long here ({longest} bytes; this system allows {MAX_SOCKET_PATH})"
            ),
            "use a shorter session name (or socket in the config)",
        ));
    }
    Ok(Local {
        session: session.into(),
        socket,
        holder_socket,
        state,
        config,
    })
}

/// The `socket` key from `config.toml`, or `None` on any problem at all —
/// core resolves endpoints the same way (`load().ok()?`).
fn config_socket(config: &Path, platform: &dyn Platform) -> Option<PathBuf> {
    let text = platform.read_file(config).ok()?;
    toml::from_str::<ConfigFile>(&text)
        .ok()?
        .socket
        .map(PathBuf::from)
}

/// An ssh target is passed to ssh as one argument: never an option, never
/// two words. `user@host`, `ssh://…`, `[v6]` and alias names are fine; a
/// leading `-` or any whitespace or control character is not.
fn check_target(target: &str) -> Result<(), RpcError> {
    if target.is_empty()
        || target.starts_with('-')
        || target.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(RpcError::new(
            -32000,
            "usage",
            format!("bad ssh target {target:?}"),
            "a host, user@host or ssh alias",
        ));
    }
    Ok(())
}

/// One remote argument as a POSIX shell word: single-quoted, with every `'`
/// closed and reopened (`o'clock` → `'o'\''clock'`). Applied to the session
/// name, the only part of the remote command that varies.
fn shell_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// A remote endpoint for `target`/`session`, or a usage error.
pub fn remote(target: &str, session: &str) -> Result<Remote, RpcError> {
    check_target(target)?;
    Ok(Remote {
        target: target.into(),
        session: session.into(),
    })
}

/// The longest ControlPath this layer allows: ssh must fit the socket
/// path in `sun_path`. `%C` expands to a 40-digit hash, so the control
/// directory plus `/%C` must stay within this bound or the control
/// options are skipped entirely.
#[cfg(unix)]
const CONTROL_PATH_MAX: usize = 100;

/// `ControlMaster=auto` + `ControlPath=<dir>/%C` + `ControlPersist=600`:
/// the first stream starts a master that every stream to the target
/// rides, and it stays up 10 minutes after the last one ends — without
/// the persist its exit would take the multiplexed streams down with
/// it. Empty off Unix, without a usable directory, or when `dir/%C`
/// would exceed [`CONTROL_PATH_MAX`].
#[cfg(unix)]
fn control_options(dir: Option<&Path>) -> Vec<OsString> {
    /// `%C` expands to this many hash digits in the socket name.
    const HASH: usize = 40;
    let Some(dir) = dir else { return Vec::new() };
    if dir.as_os_str().len() + 1 + HASH > CONTROL_PATH_MAX {
        return Vec::new();
    }
    let mut path = OsString::from("ControlPath=");
    path.push(dir.as_os_str());
    path.push("/%C");
    vec![
        OsString::from("-o"),
        OsString::from("ControlMaster=auto"),
        OsString::from("-o"),
        path,
        OsString::from("-o"),
        OsString::from("ControlPersist=600"),
    ]
}

#[cfg(not(unix))]
fn control_options(_: Option<&Path>) -> Vec<OsString> {
    Vec::new()
}

impl Remote {
    /// The remote command line ssh executes: `hc connect --session '<name>'`.
    /// The normalized session always goes on the command — including
    /// `default` — so a remote `HC_SESSION` (sent through `SendEnv` or the
    /// login environment) can never silently pick a different core than the
    /// one resolved here.
    pub fn line(&self) -> String {
        format!("hc connect --session {}", shell_quote(&self.session))
    }

    /// The ssh argument vector after the program name. `-T` because the
    /// stream is a pipe, never a terminal; batch mode so ssh fails fast
    /// instead of asking our UI for a password; keepalives so a dead link
    /// closes instead of hanging forever. On Unix a usable `control_dir`
    /// adds the options that share one login per target for 10 minutes
    /// past the last stream (see [`control_options`]). `None` — always
    /// off Unix, for a refused directory or an over-long socket path —
    /// leaves each open its own login; the user's ssh config may still
    /// share it.
    pub fn argv(&self, control_dir: Option<&Path>) -> Vec<OsString> {
        let mut argv = vec![OsString::from("-T")];
        for option in [
            "BatchMode=yes",
            "ConnectTimeout=10",
            "ServerAliveInterval=15",
            "ServerAliveCountMax=4",
        ] {
            argv.push("-o".into());
            argv.push(option.into());
        }
        argv.extend(control_options(control_dir));
        argv.push(self.target.clone().into());
        argv.push(self.line().into());
        argv
    }
}
