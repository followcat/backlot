//! `backlot hollywood setup` (R22, D14): writes the `[hollywood]`
//! table into the user's `backlot/config.toml` — the file the client
//! reads at start and the stage re-reads every tick — and keeps
//! everything else in it byte for byte. Every bad input is refused
//! before the file is touched: an unsupported or uninstalled runner
//! is the dry run's own message, a bad effort or `--key` is refused
//! the way the config's own rules refuse it.

use std::fmt::Write as _;
use std::path::Path;

use crate::args::Common;
use crate::cli::{Env, Fail};
use crate::config::{self, HollywoodCfg};

/// The TOML literal for a config value: quotes and control
/// characters must not break the table's own shape.
fn quoted(v: &str) -> String {
    let mut out = String::from("\"");
    for c in v.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The `[hollywood]` table's own text, newline-ended.
fn table(cfg: &HollywoodCfg) -> String {
    let mut out = format!(
        "[hollywood]\nrunner = {}\nmodel = {}\n",
        quoted(&cfg.runner),
        quoted(&cfg.model)
    );
    if let Some(e) = cfg.effort.as_deref() {
        let _ = writeln!(out, "effort = {}", quoted(e));
    }
    if let Some(k) = cfg.key.as_deref() {
        let _ = writeln!(out, "key = {}", quoted(k));
    }
    if let Some(d) = cfg.default_cast.as_deref() {
        let _ = writeln!(out, "default_cast = {}", quoted(d));
    }
    out
}

/// A line's first dotted table name when it opens a `[table]` or
/// `[[array]]` header — `None` for entries, values and comments.
/// Names cannot hold `#` or `"`, so a trailing comment strips
/// safely.
fn header_name(line: &str) -> Option<String> {
    let bare = line.split('#').next()?.trim();
    let mut inner = bare.strip_prefix('[')?;
    inner = inner.strip_prefix('[').unwrap_or(inner);
    let inner = inner.strip_suffix(']').unwrap_or(inner).trim_end();
    let inner = inner.strip_suffix(']').unwrap_or(inner).trim();
    inner.split('.').next().map(|s| s.trim().to_string())
}

/// The byte range of the `[hollywood]` table family — its header
/// line through the line before the next table with another name —
/// or `None` when no such header exists.
fn hollywood_span(text: &str) -> Option<(usize, usize)> {
    let mut start = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        match header_name(line).as_deref() {
            Some("hollywood") => {
                if start.is_none() {
                    start = Some(offset);
                }
            }
            Some(_) if start.is_some() => return Some((start.unwrap(), offset)),
            _ => {}
        }
        offset += line.len();
    }
    start.map(|s| (s, text.len()))
}

/// A `--key` sanity check — the client does the real binding parse.
/// Here a direct binding needs a modifier (`alt+y`, not `y`) and a
/// `prefix+` chord needs a tail.
fn check_key(key: &str) -> Result<(), Fail> {
    let k = key.trim();
    if k.is_empty() {
        return Err(Fail::usage("--key is empty"));
    }
    match k.strip_prefix("prefix+") {
        Some(tail) if tail.trim().is_empty() => {
            Err(Fail::usage("--key 'prefix+' wants a key after it"))
        }
        Some(_) => Ok(()),
        None if k.contains('+') => Ok(()),
        None => Err(Fail::usage(format!(
            "--key '{k}' is a bare key — want a binding like alt+y or prefix+<key>"
        ))),
    }
}

/// `hollywood setup --runner <r> --model <m> [--effort <e>] [--key
/// <k>]` — the user's, never a role terminal's: the table steers the
/// overseer itself (D17a's gate's shape).
pub(crate) fn hsetup(_common: &Common, env: &Env, cfg: &HollywoodCfg) -> Result<String, Fail> {
    if env.get("BACKLOT_ROLE").is_some() {
        return Err(Fail::refused(
            "hollywood setup is the user's — BACKLOT_ROLE is set",
        ));
    }
    if cfg.runner.trim().is_empty() {
        return Err(Fail::usage("--runner is empty"));
    }
    if !crate::runners::supported(&cfg.runner) {
        return Err(Fail::refused(format!(
            "runner '{}' is not supported (supported: fake, claude, codex, devin, opencode, cursor, cmd, cline)",
            cfg.runner
        )));
    }
    if !crate::runners::installed(&cfg.runner, env.get("PATH")) {
        let what = if cfg.runner == "fake" {
            "tools/fake-runner.sh"
        } else {
            cfg.runner.as_str()
        };
        return Err(Fail::refused(format!(
            "runner '{}' is not installed: {what} not found on PATH",
            cfg.runner
        )));
    }
    if cfg.model.trim().is_empty() {
        return Err(Fail::usage("--model is empty"));
    }
    config::effort_rule(&cfg.runner, cfg.effort.as_deref()).map_err(Fail::refused)?;
    if let Some(k) = cfg.key.as_deref() {
        check_key(k)?;
    }
    let path = config::config_path(&env.vars);
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    // A malformed file is left exactly as it is — the refusal names
    // the parse position only, never the file's text.
    let _: toml::Value = toml::from_str(&old).map_err(|e| match e.span() {
        Some(span) => Fail::refused(format!(
            "config.toml: malformed at {}",
            config::line_col(&old, span.start)
        )),
        None => Fail::refused("config.toml: malformed".to_string()),
    })?;
    let section = table(cfg);
    let new = match hollywood_span(&old) {
        Some((a, b)) => format!("{}{section}{}", &old[..a], &old[b..]),
        None => {
            let mut new = old.clone();
            if !new.is_empty() && !new.ends_with('\n') {
                new.push('\n');
            }
            if !new.is_empty() {
                new.push('\n');
            }
            new.push_str(&section);
            new
        }
    };
    if new != old {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| Fail::refused(format!("{}: {e}", dir.display())))?;
        }
        // Through a sibling temp file: a torn write leaves the old
        // config, never a half table.
        write_atomic(&path, &new)?;
    }
    Ok(format!(
        "[hollywood] written to {} — a running stage sees it on its next tick; restart the TUI for the layer",
        path.display()
    ))
}

/// Write `text` over `path` through a sibling temp file + rename,
/// keeping the old file's mode when it has one.
fn write_atomic(path: &Path, text: &str) -> Result<(), Fail> {
    let tmp = path.with_extension("toml.setup-tmp");
    std::fs::write(&tmp, text).map_err(|e| Fail::refused(format!("{}: {e}", tmp.display())))?;
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Fail::refused(format!("{}: {e}", path.display()))
    })
}
