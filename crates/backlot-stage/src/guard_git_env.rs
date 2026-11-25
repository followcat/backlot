//! Git work-area containment, `GIT_CONFIG_*` checks, `real_git` probing,
//! and plain alias word-splitting for the outward git guard.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use crate::cli::Env;
use crate::guard::find_real_binary;

/// Normalize `target` against `base` lexically and canonicalize the deepest existing ancestor.
pub(crate) fn normalize_resolve(base: &Path, target: &Path) -> PathBuf {
    let abs = if target.is_absolute() {
        target.to_path_buf()
    } else {
        base.join(target)
    };
    let mut stack: Vec<std::ffi::OsString> = Vec::new();
    let mut prefix: Option<PathBuf> = None;
    for comp in abs.components() {
        match comp {
            Component::Prefix(p) => prefix = Some(PathBuf::from(p.as_os_str())),
            Component::RootDir => {
                stack.clear();
                prefix = Some(prefix.take().unwrap_or_else(|| PathBuf::from("/")));
            }
            Component::CurDir => {}
            Component::ParentDir => {
                stack.pop();
            }
            Component::Normal(s) => stack.push(s.to_os_string()),
        }
    }
    let mut clean = prefix.unwrap_or_else(|| PathBuf::from("/"));
    for s in &stack {
        clean.push(s);
    }
    if let Ok(canon) = clean.canonicalize() {
        return canon;
    }
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let mut probe = clean.as_path();
    while let Some(parent) = probe.parent() {
        if let Some(name) = probe.file_name() {
            tail.push(name.to_os_string());
        }
        if let Ok(canon) = parent.canonicalize() {
            let mut res = canon;
            for seg in tail.iter().rev() {
                res.push(seg);
            }
            return res;
        }
        probe = parent;
    }
    clean
}

pub(crate) fn path_inside(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| path == r || path.starts_with(r))
}

/// Apply `-C` directories to `cwd`.
pub(crate) fn effective_cwd(cwd: &Path, c_dirs: &[String]) -> PathBuf {
    let mut cur = cwd.to_path_buf();
    for d in c_dirs {
        let p = Path::new(d);
        cur = if p.is_absolute() {
            p.to_path_buf()
        } else {
            cur.join(p)
        };
    }
    cur
}

/// Build an isolated env map for internal `real_git` probes from `env`.
pub(crate) fn git_probe_env(env: &Env, role_dir: Option<&Path>) -> BTreeMap<String, String> {
    let mut vars = env.vars.clone();
    let shim_bin = role_dir.map(|r| r.join("bin"));
    let path = env
        .get("PATH")
        .map(String::from)
        .or_else(|| std::env::var("PATH").ok())
        .unwrap_or_default();
    let filtered: Vec<&str> = path
        .split(':')
        .filter(|e| !e.is_empty())
        .filter(|e| {
            let p = Path::new(e);
            shim_bin.as_deref().is_none_or(|sb| p != sb)
        })
        .collect();
    vars.insert("PATH".into(), filtered.join(":"));
    vars
}

/// Run real `git` in `cwd` using `env_map` (ignoring PATH shims).
pub(crate) fn run_real_git(
    args: &[&str],
    cwd: &Path,
    env: &Env,
    role_dir: Option<&Path>,
) -> Option<(i32, String, String)> {
    let shim_bin = role_dir.map(|r| r.join("bin"));
    let proc_path = std::env::var("PATH").ok();
    let bin = find_real_binary("git", env.get("PATH"), shim_bin.as_deref())
        .or_else(|| find_real_binary("git", proc_path.as_deref(), shim_bin.as_deref()))
        .unwrap_or_else(|| PathBuf::from("git"));
    let probe_env = git_probe_env(env, role_dir);
    let run_dir = if cwd.is_dir() { cwd } else { Path::new(".") };
    let out = Command::new(bin)
        .args(args)
        .current_dir(run_dir)
        .env_clear()
        .envs(&probe_env)
        .output()
        .ok()?;
    Some((
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

pub(crate) fn is_blocked_config_key(key: &str) -> bool {
    let lower = key.trim().to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "core.sshcommand"
            | "core.fsmonitor"
            | "gpg.program"
            | "push.gpgsign"
            | "push.recursesubmodules"
            | "submodule.recurse"
    ) || lower.starts_with("alias.")
        || (lower.starts_with("remote.")
            && (lower.ends_with(".push")
                || lower.ends_with(".pushurl")
                || lower.ends_with(".receivepack")
                || lower.ends_with(".mirror")))
        || (lower.starts_with("url.")
            && (lower.ends_with(".insteadof") || lower.ends_with(".pushinsteadof")))
}

/// Check `GIT_CONFIG_*` env vars for overrides of blocked keys or tampered `core.hooksPath`.
pub(crate) fn check_git_config_env(
    env: &Env,
    eff_cwd: &Path,
    role_dir: Option<&Path>,
    allowed_work: &[PathBuf],
) -> Result<(), String> {
    if let Some(params) = env.get("GIT_CONFIG_PARAMETERS")
        && !params.trim().is_empty()
    {
        let lower = params.to_ascii_lowercase();
        for needle in [
            "core.hookspath",
            "core.sshcommand",
            "core.fsmonitor",
            "gpg.program",
            "push.gpgsign",
            "push.recursesubmodules",
            "submodule.recurse",
            "alias.",
            "pushurl",
            "insteadof",
            "pushinsteadof",
            "receivepack",
            ".mirror",
        ] {
            if lower.contains(needle) {
                return Err(format!(
                    "git config override in GIT_CONFIG_PARAMETERS ({needle})"
                ));
            }
        }
    }
    for var in ["GIT_CONFIG", "GIT_CONFIG_GLOBAL", "GIT_CONFIG_SYSTEM"] {
        if let Some(val) = env.get(var)
            && !val.trim().is_empty()
            && val != "/dev/null"
        {
            let resolved = normalize_resolve(eff_cwd, Path::new(val));
            if allowed_work.is_empty() || !path_inside(&resolved, allowed_work) {
                return Err(format!("git path outside work area ({var})"));
            }
        }
    }
    if let Some(cnt_raw) = env.get("GIT_CONFIG_COUNT") {
        let count: usize = cnt_raw
            .trim()
            .parse()
            .map_err(|_| "invalid GIT_CONFIG_COUNT".to_string())?;
        for i in 0..count {
            let Some(key) = env.get(&format!("GIT_CONFIG_KEY_{i}")) else {
                continue;
            };
            let val = env.get(&format!("GIT_CONFIG_VALUE_{i}")).unwrap_or("");
            if key.trim().eq_ignore_ascii_case("core.hooksPath") {
                if let Some(rd) = role_dir {
                    let want = normalize_resolve(eff_cwd, &rd.join("git-hooks"));
                    let got = normalize_resolve(eff_cwd, Path::new(val));
                    if got != want {
                        return Err("git hooksPath tampered".into());
                    }
                }
            } else if is_blocked_config_key(key) {
                return Err(format!("git config override in GIT_CONFIG_* ({key})"));
            }
        }
    }
    Ok(())
}

/// Verify `core.hooksPath` and local push-related config integrity before push.
pub(crate) fn verify_hook_integrity(
    eff_cwd: &Path,
    env: &Env,
    role_dir: Option<&Path>,
) -> Result<(), String> {
    if let Some(rd) = role_dir {
        let hook_dir = rd.join("git-hooks");
        let want = normalize_resolve(eff_cwd, &hook_dir);
        let mut clean = Env {
            vars: env.vars.clone(),
        };
        clean.vars.insert("GIT_CONFIG_COUNT".into(), "0".into());
        clean.vars.remove("GIT_CONFIG_PARAMETERS");
        if let Some((0, local_hp, _)) = run_real_git(
            &["config", "--get", "core.hooksPath"],
            eff_cwd,
            &clean,
            role_dir,
        ) {
            let trimmed = local_hp.trim();
            if !trimmed.is_empty() && normalize_resolve(eff_cwd, Path::new(trimmed)) != want {
                return Err("git hooksPath tampered".into());
            }
        }
        if rd.join("bin/git").is_file() || hook_dir.is_dir() {
            if !hook_dir.join("pre-push").is_file() {
                return Err("git hooksPath tampered".into());
            }
            let Some((0, out, _)) = run_real_git(
                &["config", "--get", "core.hooksPath"],
                eff_cwd,
                env,
                role_dir,
            ) else {
                return Err("git hooksPath tampered".into());
            };
            if normalize_resolve(eff_cwd, Path::new(out.trim())) != want {
                return Err("git hooksPath tampered".into());
            }
        }
    }
    for key in ["core.sshCommand", "core.fsmonitor"] {
        if let Some((0, val, _)) =
            run_real_git(&["config", "--local", "--get", key], eff_cwd, env, role_dir)
            && !val.trim().is_empty()
        {
            return Err(format!("git local config tampered ({key})"));
        }
    }
    crate::guard_git_probe::check_local_push_config(
        eff_cwd,
        env,
        role_dir,
        "git local config tampered",
    )?;
    Ok(())
}

/// Shell-word split a plain (non-`!`) git alias value.
pub(crate) fn split_plain_alias(val: &str) -> Result<Vec<String>, ()> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut has = false;
    let mut in_single = false;
    let mut in_double = false;
    let mut escape = false;

    for c in val.chars() {
        if escape {
            escape = false;
            cur.push(c);
            has = true;
            continue;
        }
        if in_single {
            if c == '\'' {
                in_single = false;
            } else {
                cur.push(c);
                has = true;
            }
            continue;
        }
        if in_double {
            match c {
                '"' => in_double = false,
                '\\' => escape = true,
                _ => {
                    cur.push(c);
                    has = true;
                }
            }
            continue;
        }
        match c {
            '\'' => {
                in_single = true;
                has = true;
            }
            '"' => {
                in_double = true;
                has = true;
            }
            '\\' => escape = true,
            c if c.is_whitespace() => {
                if has {
                    words.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            _ => {
                cur.push(c);
                has = true;
            }
        }
    }
    if escape || in_single || in_double {
        return Err(());
    }
    if has {
        words.push(cur);
    }
    Ok(words)
}
