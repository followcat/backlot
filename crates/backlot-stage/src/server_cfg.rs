//! Client resolution of `--server <host>[/<session>]`, `BACKLOT_SERVER`,
//! `[client]` in `config.toml`, and local-hub host detection.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::io::ErrorKind;

use hc_proto::endpoint::{Endpoint, Env as ProtoEnv, Platform};
use hc_proto::jsonrpc::RpcError;
use serde::Deserialize;

use crate::cli::Fail;
use crate::config;

/// Default `hc` session name when `--server <host>` omits `/<session>`.
pub const DEFAULT_SERVER_SESSION: &str = "backlot";

/// Environment variable naming the remote or hub server target.
pub const SERVER_ENV: &str = "BACKLOT_SERVER";

/// Resolved server target `(host, session)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerTarget {
    pub host: String,
    pub session: String,
}

#[derive(Debug, Default, Deserialize)]
struct ClientConfigFile {
    client: Option<ClientSection>,
}

#[derive(Debug, Default, Deserialize)]
struct ClientSection {
    server: Option<String>,
    session: Option<String>,
}

fn valid_token(s: &str) -> bool {
    !s.is_empty() && !s.chars().any(|c| c.is_whitespace() || c.is_control())
}

fn valid_host(host: &str) -> bool {
    if !valid_token(host) || host.starts_with('-') {
        return false;
    }
    if let Some((user, name)) = host.split_once('@') {
        !user.is_empty() && !name.is_empty() && !name.starts_with('-')
    } else {
        true
    }
}

/// Parse `<host>[/<session>]`, defaulting the session to `default_session`
/// or [`DEFAULT_SERVER_SESSION`].
pub fn parse_target(raw: &str, default_session: Option<&str>) -> Result<ServerTarget, Fail> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Fail::usage("--server requires <host>[/<session>]"));
    }
    let (host, session) = match trimmed.split_once('/') {
        Some((h, s)) => {
            if !valid_token(s) || s.contains('/') {
                return Err(Fail::usage(format!("invalid server target '{trimmed}'")));
            }
            if let Some(ds) = default_session
                && ds != s
            {
                return Err(Fail::usage(format!(
                    "conflicting session '{s}' in server and '{ds}' in [client].session"
                )));
            }
            (h, s)
        }
        None => (trimmed, default_session.unwrap_or(DEFAULT_SERVER_SESSION)),
    };
    if !valid_host(host) {
        return Err(Fail::usage(format!("invalid server host '{host}'")));
    }
    Ok(ServerTarget {
        host: host.to_owned(),
        session: session.to_owned(),
    })
}

fn from_config(env: &BTreeMap<String, String>) -> Result<Option<ServerTarget>, Fail> {
    let path = config::config_path(env);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Fail::usage(format!("config.toml: {e}"))),
    };
    let parsed: ClientConfigFile = toml::from_str(&text).map_err(|e| match e.span() {
        Some(span) => Fail::usage(format!("config.toml: malformed at offset {}", span.start)),
        None => Fail::usage("config.toml: malformed".to_owned()),
    })?;
    let Some(section) = parsed.client else {
        return Ok(None);
    };
    let default_session = match section.session.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(s) => {
            if !valid_token(s) || s.contains('/') {
                return Err(Fail::usage(format!(
                    "config.toml: invalid [client].session '{s}'"
                )));
            }
            Some(s)
        }
    };
    let Some(server) = section
        .server
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Ok(None);
    };
    parse_target(server, default_session).map(Some)
}

/// Resolve a named server target in precedence order:
/// `--server` > `BACKLOT_SERVER` > `[client]` in `config.toml`.
pub fn resolve(
    cli_server: Option<&str>,
    env: &BTreeMap<String, String>,
) -> Result<Option<ServerTarget>, Fail> {
    if let Some(cli) = cli_server {
        return parse_target(cli, None).map(Some);
    }
    if let Some(env_val) = env
        .get(SERVER_ENV)
        .map(String::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return parse_target(env_val, None).map(Some);
    }
    from_config(env)
}

/// Convenience wrapper for `hc-proto`'s OS-typed `Env`.
pub fn resolve_os(
    cli_server: Option<&str>,
    env: &ProtoEnv,
) -> Result<Option<ServerTarget>, String> {
    let mut vars = BTreeMap::new();
    for key in [SERVER_ENV, "XDG_CONFIG_HOME", "HOME"] {
        if let Some(val) = env.get(OsStr::new(key)).and_then(|v| v.to_str()) {
            vars.insert(key.to_owned(), val.to_owned());
        }
    }
    resolve(cli_server, &vars).map_err(|e| e.message().to_owned())
}

/// Best-effort OS hostname on Unix (`libc::gethostname`).
#[cfg(unix)]
pub fn system_hostname() -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: `buf` is valid for writes of `buf.len()` bytes.
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
    if rc != 0 {
        return None;
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let name = std::str::from_utf8(&buf[..end]).ok()?.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// Best-effort OS hostname on non-Unix targets.
#[cfg(not(unix))]
pub fn system_hostname() -> Option<String> {
    std::env::var("HOSTNAME")
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

fn short_label(host: &str) -> &str {
    host.split_once('.').map_or(host, |(head, _)| head)
}

/// `true` when `target_host` refers to the local hub machine given
/// `local_hostname` and `current_user`.
pub fn is_local_host_with(
    target_host: &str,
    local_hostname: Option<&str>,
    current_user: Option<&str>,
) -> bool {
    let (user, host) = match target_host.trim().split_once('@') {
        Some((u, h)) => (Some(u), h),
        None => (None, target_host.trim()),
    };
    if let Some(u) = user
        && current_user != Some(u)
    {
        return false;
    }
    let clean = host
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(host);
    if clean.eq_ignore_ascii_case("localhost")
        || clean.eq_ignore_ascii_case("localhost.localdomain")
        || clean == "127.0.0.1"
        || clean == "::1"
    {
        return true;
    }
    let Some(local) = local_hostname.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    if clean.eq_ignore_ascii_case(local) {
        return true;
    }
    let bare_pair = !clean.contains('.') || !local.contains('.');
    bare_pair && short_label(clean).eq_ignore_ascii_case(short_label(local))
}

/// `true` when `target_host` refers to this machine.
pub fn is_local_host(target_host: &str) -> bool {
    let user = std::env::var("USER").ok();
    is_local_host_with(target_host, system_hostname().as_deref(), user.as_deref())
}

/// Build `Endpoint::Local` (on the hub) or `Endpoint::Remote` (off-hub)
/// from `target`.
pub fn resolve_endpoint_with(
    target: &ServerTarget,
    env: &ProtoEnv,
    platform: &dyn Platform,
    local_hostname: Option<&str>,
    current_user: Option<&str>,
) -> Result<Endpoint, RpcError> {
    let remote = (!is_local_host_with(&target.host, local_hostname, current_user))
        .then_some(target.host.as_str());
    hc_proto::endpoint::resolve(env, Some(OsStr::new(&target.session)), remote, platform)
}

/// Build `Endpoint::Local` or `Endpoint::Remote` using the live OS identity.
pub fn resolve_endpoint(
    target: &ServerTarget,
    env: &ProtoEnv,
    platform: &dyn Platform,
) -> Result<Endpoint, RpcError> {
    let user = std::env::var("USER").ok();
    resolve_endpoint_with(
        target,
        env,
        platform,
        system_hostname().as_deref(),
        user.as_deref(),
    )
}
