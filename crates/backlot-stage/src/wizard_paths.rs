//! The `runner_env` PATH write (U5): when the accepted draft's
//! runners live only where detection could reach — the login
//! shell's PATH or a well-known install dir — the wizard adds
//! `PATH = "<bin dir>:<inherited PATH>"` to the state root's
//! `config.toml`, so the role's terminal finds the binary a plain
//! PATH could not. An existing `runner_env.<runner>` PATH is never
//! rewritten; the file is born `0600` when missing.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

use crate::cli::{Env, Fail};
use crate::commands;
use crate::runners::detect::Found;
use crate::wizard::{Io, io_fail};

/// The accepted draft's runners: each one detection reached only
/// through the login shell or a well-known dir gets its
/// `[runner_env.<runner>]` PATH — shown before it is written.
pub(crate) fn runner_paths(
    io: &mut Io,
    base: &Path,
    cast_file: &Path,
    env: &Env,
) -> Result<(), Fail> {
    let cast = commands::read_cast(cast_file)?;
    let detect = &io.detect;
    ensure_runner_paths(
        base,
        &commands::cast_runners(&cast),
        env,
        &mut |runner| detect(runner),
        &mut io.out,
    )
}

/// The `[runner_env]` PATH write without the wizard's `Io` — the
/// non-interactive `studio new` runs it too, so a cast from the
/// New Studio dialog or a script gets the launch PATH its picked
/// runners need, exactly like the interactive draft's.
pub(crate) fn ensure_runner_paths(
    base: &Path,
    named: &BTreeSet<String>,
    env: &Env,
    detect: &mut dyn FnMut(&str) -> Option<Found>,
    out: &mut dyn Write,
) -> Result<(), Fail> {
    let config = base.join("config.toml");
    for runner in named {
        let Some(found) = detect(runner) else {
            continue;
        };
        if found.needs_path() {
            write_runner_path(out, &config, runner, &found, env)?;
        }
    }
    Ok(())
}

/// One runner's PATH write: never touch an existing
/// `runner_env.<runner>` PATH — warn when it does not reach the
/// binary; otherwise add `PATH = "<bin dir>:<inherited PATH>"`,
/// inside the runner's existing table or as a new one at the end.
fn write_runner_path(
    out: &mut dyn Write,
    config: &Path,
    runner: &str,
    found: &Found,
    env: &Env,
) -> Result<(), Fail> {
    let text = std::fs::read_to_string(config).unwrap_or_default();
    let bin = found
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| runner.to_string());
    let existing = match crate::config::runner_env_in(&text, runner) {
        Ok(map) => map,
        Err(_) => {
            writeln!(
                out,
                "warning: {} does not parse — add this by hand:\n  [runner_env.{runner}]\n  {}",
                config.display(),
                path_line(found, env),
            )
            .map_err(io_fail)?;
            return Ok(());
        }
    };
    if let Some(path) = existing.get("PATH") {
        // The user's own PATH wins; it only needs a warning when it
        // still cannot reach the binary.
        if crate::runners::find_on_path(&bin, Some(path)).is_none() {
            writeln!(
                out,
                "warning: {} already sets runner_env.{runner} PATH, but '{bin}' is still not on it — the role may fail 'not found'",
                config.display(),
            )
            .map_err(io_fail)?;
        }
        return Ok(());
    }
    let line = path_line(found, env);
    let header = format!("[runner_env.{runner}]");
    match table_header(&text, runner) {
        // A table the runner already has: the line goes under it.
        Some(at) => {
            writeln!(
                out,
                "{runner} lives at {} — adding to {header} in {}:\n  {line}",
                found.bin_dir().display(),
                config.display(),
            )
            .map_err(io_fail)?;
            let mut lines: Vec<&str> = text.lines().collect();
            lines.insert(at + 1, line.as_str());
            let mut new_text = lines.join("\n");
            if text.ends_with('\n') {
                new_text.push('\n');
            }
            write_config(config, &new_text)?;
        }
        None => {
            writeln!(
                out,
                "{runner} lives at {} — adding to {}:\n  {header}\n  {line}",
                found.bin_dir().display(),
                config.display(),
            )
            .map_err(io_fail)?;
            let block = format!("{header}\n{line}\n");
            let new_text = if text.is_empty() {
                block
            } else if text.ends_with('\n') {
                format!("{text}\n{block}")
            } else {
                format!("{text}\n\n{block}")
            };
            write_config(config, &new_text)?;
        }
    }
    Ok(())
}

/// `PATH = "<bin dir>:<inherited PATH>"` — TOML-quoted; the binary's
/// own directory first, because a node-based CLI needs the `node`
/// next to it.
fn path_line(found: &Found, env: &Env) -> String {
    let value = crate::runners::resolve::launch_path(found, env);
    format!("PATH = {}", toml_string(&value))
}

/// `value` as a TOML basic string — quotes, backslashes and control
/// characters escaped.
fn toml_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The line index of `runner`'s `[runner_env.<runner>]` header —
/// whitespace inside the brackets ignored, like TOML allows.
fn table_header(text: &str, runner: &str) -> Option<usize> {
    let want = format!("runner_env.{runner}").replace(' ', "");
    text.lines().position(|line| {
        let t = line.trim();
        t.starts_with('[')
            && t.ends_with(']')
            && !t.starts_with("[[")
            && t[1..t.len() - 1].replace(' ', "") == want
    })
}

/// `config.toml`'s write: a missing file is born `0600`; an
/// existing one keeps its inode and mode.
fn write_config(config: &Path, text: &str) -> Result<(), Fail> {
    if config.exists() {
        std::fs::write(config, text).map_err(|e| Fail::refused(format!("config.toml: {e}")))
    } else {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        opts.open(config)
            .and_then(|mut f| f.write_all(text.as_bytes()))
            .map_err(|e| Fail::refused(format!("config.toml: {e}")))
    }
}
