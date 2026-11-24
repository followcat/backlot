//! Shared helpers for per-work-area hook files (`agy`, `cmd`, `devin`)
//! and git exclude / tracked-file guards.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use super::claude::{live_exe, sh_quote};
use super::splice::splice_member;

/// Build `<quoted backlot> hook --runner <runner> --file <quoted signals>`.
pub(crate) fn hook_command(runner: &str, signals: &Path) -> String {
    debug_assert!(
        !runner.is_empty() && runner.bytes().all(|b| b.is_ascii_alphanumeric()),
        "runner name must be alphanumeric: {runner}"
    );
    let raw = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("backlot"));
    let exe = live_exe(raw, |p| p.exists());
    format!(
        "{} hook --runner {runner} --file {}",
        sh_quote(&exe.to_string_lossy()),
        sh_quote(&signals.to_string_lossy()),
    )
}

/// Derive `<cwd>` when `file` lives two levels beneath it
/// (`.agents/hooks.json`, `.commandcode/settings.json`, `.devin/config.local.json`).
pub(crate) fn work_dir_of(file: &Path) -> &Path {
    file.parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("."))
}

/// Return `true` when `rel` (relative to `cwd`) is tracked in `git`.
pub(crate) fn is_tracked(cwd: &Path, rel: &str) -> bool {
    crate::workareas::real_git(
        &[
            "-C",
            &cwd.to_string_lossy(),
            "ls-files",
            "--error-unmatch",
            "--",
            rel,
        ],
        ".",
    )
    .code
        == 0
}

/// Locate the repository's `info/exclude` file for `cwd`, following a
/// worktree `.git` file and its `commondir` (falling back to `gitdir`
/// when `commondir` is absent).
pub(crate) fn exclude_path(cwd: &Path) -> Option<PathBuf> {
    let dot = cwd.join(".git");
    let md = fs::metadata(&dot).ok()?;
    if md.is_dir() {
        return Some(dot.join("info").join("exclude"));
    }
    let text = fs::read_to_string(&dot).ok()?;
    let line = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("gitdir:"))?
        .trim();
    let git = resolve_rel(cwd, line);
    if let Ok(common) = fs::read_to_string(git.join("commondir"))
        && let Some(rel) = common.lines().find(|l| !l.trim().is_empty())
    {
        let root = resolve_rel(&git, rel.trim());
        return Some(root.join("info").join("exclude"));
    }
    Some(git.join("info").join("exclude"))
}

fn resolve_rel(base: &Path, rel: &str) -> PathBuf {
    let p = Path::new(rel);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

/// Append `entry` to `<git>/info/exclude` if not already present.
pub(crate) fn exclude_add(path: &Path, entry: &str) -> Result<(), String> {
    let existing = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let needle = entry.trim_start_matches('/');
    if existing
        .lines()
        .any(|l| l.trim().trim_start_matches('/') == needle)
    {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if !existing.is_empty() && !existing.ends_with('\n') {
        writeln!(f).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    writeln!(f, "{entry}").map_err(|e| format!("{}: {e}", path.display()))
}

/// If `was_new` is true and `cwd` is in a git repo, add `rel` to `info/exclude`.
pub(crate) fn maybe_exclude_new(cwd: &Path, was_new: bool, rel: &str) -> Result<(), String> {
    if !was_new {
        return Ok(());
    }
    let Some(ex) = exclude_path(cwd) else {
        return Ok(());
    };
    exclude_add(&ex, rel)
}

/// Build or byte-preservingly splice `<cwd>/.agents/hooks.json`.
pub(crate) fn agy_hooks_content(existing: Option<&[u8]>, signals: &Path) -> Vec<u8> {
    let cmd = hook_command("agy", signals);
    let hook = serde_json::json!({
        "type": "command",
        "command": cmd,
        "timeout": 5,
    });
    let flat = serde_json::json!([hook]);
    let group = serde_json::json!([{
        "matcher": "",
        "hooks": [hook],
    }]);
    let entry = serde_json::json!({
        "SessionStart": flat,
        "PreInvocation": flat,
        "PreToolUse": group,
        "PostToolUse": group,
        "PostInvocation": flat,
        "Stop": flat,
    });
    let serialized = serde_json::to_string(&entry).unwrap_or_default();
    let fresh = || format!("{{\"backlot\":{serialized}}}\n").into_bytes();
    let Some(text) = existing.and_then(|b| std::str::from_utf8(b).ok()) else {
        return fresh();
    };
    match splice_member(text.trim(), "backlot", &serialized) {
        Some(spliced) => format!("{spliced}\n").into_bytes(),
        None => fresh(),
    }
}

fn is_backlot_hook_group(item: &serde_json::Value) -> bool {
    item.get("hooks")
        .and_then(|v| v.as_array())
        .is_some_and(|arr| {
            arr.iter().any(|h| {
                h.get("command")
                    .and_then(|v| v.as_str())
                    .is_some_and(|c| c.contains(" hook --runner ") || c.contains(" hook --file "))
            })
        })
}

/// Build the serialized `"hooks"` object value preserving non-backlot
/// event matchers and stripping stale backlot hook groups.
pub(crate) fn hooks_member_json(
    existing_text: Option<&str>,
    events: &[&str],
    with_empty_matcher: bool,
    runner: &str,
    signals: &Path,
) -> String {
    let mut map = existing_text
        .and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok())
        .and_then(|v| v.get("hooks").and_then(|h| h.as_object()).cloned())
        .unwrap_or_default();

    let cmd = hook_command(runner, signals);
    let hook = serde_json::json!({
        "type": "command",
        "command": cmd,
        "timeout": 5,
    });
    let new_group = if with_empty_matcher {
        serde_json::json!({
            "matcher": "",
            "hooks": [hook],
        })
    } else {
        serde_json::json!({
            "hooks": [hook],
        })
    };

    for &event in events {
        let mut kept: Vec<serde_json::Value> = map
            .get(event)
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter(|item| !is_backlot_hook_group(item))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        kept.push(new_group.clone());
        map.insert(event.to_string(), serde_json::Value::Array(kept));
    }

    serde_json::to_string(&serde_json::Value::Object(map)).unwrap_or_default()
}
