//! Implicit, zero-secret account identity derivation (`<runner>:<label>`).
//!
//! Never reads credential files and never logs, persists, or hashes secret env
//! var values. Directory tags depend strictly on non-secret paths.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use backlot_model::hollywood::sha256_hex;

use crate::config;

/// Implicitly derived account identity and non-secret origin hint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DerivedAccount {
    pub id: String,
    pub runner: String,
    pub source: String,
    pub(crate) dir_key: Option<String>,
    pub(crate) dir_slug: Option<String>,
}

/// Derive a canonical `<runner>:<label>` account identifier for one role strictly
/// from non-secret metadata (`runner`, `model`, non-secret dir keys, or presence
/// of token env var names without touching secret values).
pub(crate) fn derive_account(
    runner: &str,
    model: &str,
    runner_env: &BTreeMap<String, String>,
    host_env: &BTreeMap<String, String>,
) -> DerivedAccount {
    let host_home = host_home_dir(host_env);
    match runner {
        "claude" => {
            let default_dir = join_lexical(&host_home, ".claude");
            if let Some(dir) = non_empty(runner_env.get("CLAUDE_CONFIG_DIR")) {
                return from_dir(runner, dir, &default_dir, &host_home);
            }
            if has_any_non_empty(
                runner_env,
                &["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"],
            ) {
                return env_token_account(runner);
            }
            default_account(runner)
        }
        "codex" => {
            let default_dir = join_lexical(&host_home, ".codex");
            if let Some(dir) = non_empty(runner_env.get("CODEX_HOME")) {
                return from_dir(runner, dir, &default_dir, &host_home);
            }
            if has_any_non_empty(runner_env, &["OPENAI_API_KEY"]) {
                return env_token_account(runner);
            }
            default_account(runner)
        }
        // Cursor adapter isolates CURSOR_CONFIG_DIR per role while auth.json sits under
        // XDG_CONFIG_HOME; ignore per-role config dirs so roles group cleanly.
        "cursor" => {
            if has_any_non_empty(runner_env, &["CURSOR_API_KEY"]) {
                return env_token_account(runner);
            }
            default_account(runner)
        }
        "agy" | "cmd" | "cline" => {
            if let Some(home_override) = non_empty(runner_env.get("HOME")) {
                return from_dir(runner, home_override, &host_home, &host_home);
            }
            default_account(runner)
        }
        "devin" => {
            let default_xdg = non_empty(host_env.get("XDG_CONFIG_HOME"))
                .map(|p| canonicalize_dir(p, &host_home))
                .unwrap_or_else(|| join_lexical(&host_home, ".config"));
            let default_devin = join_lexical(&default_xdg, "devin");
            if let Some(xdg) = non_empty(runner_env.get("XDG_CONFIG_HOME")) {
                let canon_xdg = canonicalize_dir(xdg, &host_home);
                let effective = join_lexical(&canon_xdg, "devin");
                if effective == default_devin {
                    return default_account(runner);
                }
                return from_canonical_dir(runner, &canon_xdg, &host_home);
            }
            if let Some(home_override) = non_empty(runner_env.get("HOME")) {
                let canon_home = canonicalize_dir(home_override, &host_home);
                let effective = join_lexical(&join_lexical(&canon_home, ".config"), "devin");
                if effective == default_devin {
                    return default_account(runner);
                }
                return from_canonical_dir(runner, &canon_home, &host_home);
            }
            default_account(runner)
        }
        "opencode" => derive_opencode_account(model),
        _ => default_account(runner),
    }
}

/// Derive accounts for a batch of roles and disambiguate any colliding directory tags.
pub(crate) fn resolve_accounts(
    inputs: &[(&str, &str, &BTreeMap<String, String>)],
    host_env: &BTreeMap<String, String>,
) -> Vec<DerivedAccount> {
    let mut accounts: Vec<DerivedAccount> = inputs
        .iter()
        .map(|(runner, model, env)| derive_account(runner, model, env, host_env))
        .collect();
    disambiguate_accounts(&mut accounts);
    accounts
}

/// Disambiguate colliding `(runner, dir_slug)` entries in place using a 6-hex
/// SHA-256 suffix of the non-secret canonical directory path.
pub(crate) fn disambiguate_accounts(accounts: &mut [DerivedAccount]) {
    let mut keys_by_slug: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for acct in accounts.iter() {
        if let (Some(slug), Some(key)) = (&acct.dir_slug, &acct.dir_key) {
            keys_by_slug
                .entry((acct.runner.clone(), slug.clone()))
                .or_default()
                .insert(key.clone());
        }
    }
    for acct in accounts.iter_mut() {
        if let (Some(slug), Some(key)) = (&acct.dir_slug, &acct.dir_key)
            && keys_by_slug
                .get(&(acct.runner.clone(), slug.clone()))
                .is_some_and(|keys| keys.len() > 1)
        {
            acct.id = format!("{}:{}", acct.runner, disambiguated_label(slug, key));
        }
    }
}

/// Read `<state_dir>/config.toml` and `$XDG_CONFIG_HOME/backlot/config.toml` to resolve
/// one role's implicit account, falling back cleanly if a config file is absent/malformed.
pub(crate) fn derive_for_state_dir(
    state_dir: &Path,
    host_env: &BTreeMap<String, String>,
    runner: &str,
    model: &str,
) -> DerivedAccount {
    let eff_host = effective_host_env(host_env);
    let home = Path::new(non_empty(eff_host.get("HOME")).unwrap_or("/"));
    let core_text = std::fs::read_to_string(state_dir.join("config.toml")).unwrap_or_default();
    let user_text = std::fs::read_to_string(config::config_path(&eff_host)).unwrap_or_default();
    let runner_env =
        config::runner_env_layered_home(state_dir, &core_text, &user_text, runner, home)
            .unwrap_or_default();
    derive_account(runner, model, &runner_env, &eff_host)
}

/// Build an effective host env map filling `HOME` / `XDG_CONFIG_HOME` from process
/// env only when absent from `env_vars`.
pub(crate) fn effective_host_env(env_vars: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for key in ["HOME", "XDG_CONFIG_HOME"] {
        if let Some(val) = non_empty(env_vars.get(key)) {
            out.insert(key.into(), val.to_string());
        } else if let Ok(val) = std::env::var(key)
            && !val.trim().is_empty()
        {
            out.insert(key.into(), val);
        }
    }
    out
}

fn default_account(runner: &str) -> DerivedAccount {
    DerivedAccount {
        id: format!("{runner}:default"),
        runner: runner.to_string(),
        source: "default".into(),
        dir_key: None,
        dir_slug: None,
    }
}

fn env_token_account(runner: &str) -> DerivedAccount {
    DerivedAccount {
        id: format!("{runner}:env-token"),
        runner: runner.to_string(),
        source: "env-token".into(),
        dir_key: None,
        dir_slug: None,
    }
}

fn derive_opencode_account(model: &str) -> DerivedAccount {
    if let Some((provider, _)) = model.split_once('/') {
        let slug = sanitize_label(provider);
        if !slug.is_empty() && slug != "default" && slug != "env-token" {
            return DerivedAccount {
                id: format!("opencode:{slug}"),
                runner: "opencode".into(),
                source: format!("provider ({slug})"),
                dir_key: None,
                dir_slug: None,
            };
        }
    }
    default_account("opencode")
}

fn from_dir(runner: &str, raw_dir: &str, default_dir: &str, host_home: &str) -> DerivedAccount {
    let canon = canonicalize_dir(raw_dir, host_home);
    if canon == default_dir {
        return default_account(runner);
    }
    from_canonical_dir(runner, &canon, host_home)
}

fn from_canonical_dir(runner: &str, canon: &str, host_home: &str) -> DerivedAccount {
    let slug = dir_slug_from_path(runner, canon);
    let label = if slug == "default" || slug == "env-token" {
        disambiguated_label(&slug, canon)
    } else {
        slug.clone()
    };
    let shown = abbreviate_home(canon, host_home);
    DerivedAccount {
        id: format!("{runner}:{label}"),
        runner: runner.to_string(),
        source: format!("config-dir ({shown})"),
        dir_key: Some(canon.to_string()),
        dir_slug: Some(slug),
    }
}

fn dir_slug_from_path(runner: &str, canonical: &str) -> String {
    let base = Path::new(canonical)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("custom");
    let trimmed = base.trim_start_matches('.');
    let runner_lower = runner.to_ascii_lowercase();
    let lower = trimmed.to_ascii_lowercase();
    let stripped = lower
        .strip_prefix(&format!("{runner_lower}-"))
        .or_else(|| lower.strip_prefix(&format!("{runner_lower}_")))
        .unwrap_or(&lower);
    let slug = sanitize_label(stripped);
    if slug.is_empty() {
        "custom".into()
    } else {
        slug
    }
}

fn sanitize_label(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(32));
    for ch in raw.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_lowercase() || lower.is_ascii_digit() || lower == '-' || lower == '_' {
            out.push(lower);
        } else {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches(|c| c == '-' || c == '_');
    let mut truncated: String = trimmed.chars().take(32).collect();
    while truncated.ends_with(['-', '_']) {
        truncated.pop();
    }
    if truncated.is_empty()
        || !truncated
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
    {
        return String::new();
    }
    truncated
}

fn disambiguated_label(slug: &str, canonical_dir: &str) -> String {
    let hash = &sha256_hex(canonical_dir.as_bytes())[..6];
    let mut prefix: String = slug.chars().take(25).collect();
    while prefix.ends_with(['-', '_']) {
        prefix.pop();
    }
    if prefix.is_empty() {
        format!("acct-{hash}")
    } else {
        format!("{prefix}-{hash}")
    }
}

fn host_home_dir(host_env: &BTreeMap<String, String>) -> String {
    match non_empty(host_env.get("HOME")) {
        Some(h) => canonicalize_dir(h, "/"),
        None => "/".into(),
    }
}

fn canonicalize_dir(raw: &str, host_home: &str) -> String {
    let trimmed = raw.trim();
    let expanded = if trimmed == "~" {
        host_home.to_string()
    } else if let Some(rest) = trimmed.strip_prefix("~/") {
        join_lexical(host_home, rest)
    } else {
        trimmed.to_string()
    };
    normalize_lexical(&expanded)
}

fn join_lexical(base: &str, child: &str) -> String {
    let joined = Path::new(base).join(child);
    normalize_lexical(&joined.to_string_lossy())
}

fn normalize_lexical(path: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    let abs = path.starts_with('/');
    for comp in Path::new(path).components() {
        match comp {
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                if !abs && parts.last().is_none_or(|p| p == "..") {
                    parts.push("..".into());
                } else {
                    parts.pop();
                }
            }
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
            Component::Prefix(p) => parts.push(p.as_os_str().to_string_lossy().into_owned()),
        }
    }
    if abs {
        if parts.is_empty() {
            "/".into()
        } else {
            format!("/{}", parts.join("/"))
        }
    } else if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    }
}

fn abbreviate_home(canon: &str, host_home: &str) -> String {
    if canon == host_home {
        return "~".into();
    }
    if host_home != "/" {
        let prefix = format!("{host_home}/");
        if let Some(rest) = canon.strip_prefix(&prefix) {
            return format!("~/{rest}");
        }
    }
    canon.to_string()
}

fn has_any_non_empty(env: &BTreeMap<String, String>, keys: &[&str]) -> bool {
    keys.iter().any(|k| non_empty(env.get(*k)).is_some())
}

fn non_empty(v: Option<&String>) -> Option<&str> {
    v.map(String::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
#[path = "accounts_tests.rs"]
mod tests;
