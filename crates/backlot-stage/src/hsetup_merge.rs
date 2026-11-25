//! Line-preserving `[hollywood]` TOML section writer shared by
//! `hsetup`, `propose setup`, and `Setup` card execution.

use std::fmt::Write as _;

use crate::config::HollywoodCfg;

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

/// A line's first dotted table name when it opens a `[table]` or
/// `[[array]]` header — `None` for entries, values and comments.
pub(super) fn header_name(line: &str) -> Option<String> {
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
pub(super) fn hollywood_span(text: &str) -> Option<(usize, usize)> {
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

fn exact_header(line: &str) -> Option<String> {
    let bare = line.split('#').next()?.trim();
    let inner = bare.strip_prefix('[')?.strip_suffix(']')?.trim();
    if inner.starts_with('[') || inner.ends_with(']') {
        return None;
    }
    Some(inner.to_string())
}

fn toml_kv_key(line: &str) -> Option<String> {
    let s = line.trim_start();
    if s.is_empty() || s.starts_with('#') || s.starts_with('[') {
        return None;
    }
    let (raw, _) = s.split_once('=')?;
    let k = raw.trim();
    if k.is_empty() || k.contains('.') {
        return None;
    }
    let unquoted = k
        .strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .or_else(|| k.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')))
        .unwrap_or(k);
    Some(unquoted.to_string())
}

/// The `[hollywood]` table's own text when creating from scratch.
pub(super) fn table(cfg: &HollywoodCfg) -> String {
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

/// Replace/update `[hollywood]` in an existing `[hollywood]` table slice
/// while preserving `start`, `key`/`default_cast` (when omitted in `cfg`),
/// comments, subtables, and every unknown key.
pub(super) fn update_section(existing: &str, cfg: &HollywoodCfg) -> String {
    let mut out = String::new();
    let mut trailing_blanks = String::new();
    let mut in_root = false;
    let mut saw_root = false;
    let mut wrote_runner = false;
    let mut wrote_model = false;
    let mut wrote_effort = false;
    let mut wrote_key = false;
    let mut wrote_cast = false;

    let flush_missing = |out: &mut String,
                         wrote_runner: &mut bool,
                         wrote_model: &mut bool,
                         wrote_effort: &mut bool,
                         wrote_key: &mut bool,
                         wrote_cast: &mut bool| {
        let mut need_nl = !out.is_empty() && !out.ends_with('\n');
        let mut push_line = |line: String| {
            if need_nl {
                out.push('\n');
                need_nl = false;
            }
            out.push_str(&line);
        };
        if !*wrote_runner {
            push_line(format!("runner = {}\n", quoted(&cfg.runner)));
            *wrote_runner = true;
        }
        if !*wrote_model {
            push_line(format!("model = {}\n", quoted(&cfg.model)));
            *wrote_model = true;
        }
        if !*wrote_effort {
            if let Some(e) = cfg.effort.as_deref() {
                push_line(format!("effort = {}\n", quoted(e)));
            }
            *wrote_effort = true;
        }
        if !*wrote_key {
            if let Some(k) = cfg.key.as_deref() {
                push_line(format!("key = {}\n", quoted(k)));
            }
            *wrote_key = true;
        }
        if !*wrote_cast {
            if let Some(d) = cfg.default_cast.as_deref() {
                push_line(format!("default_cast = {}\n", quoted(d)));
            }
            *wrote_cast = true;
        }
    };

    for line in existing.split_inclusive('\n') {
        if exact_header(line).as_deref() == Some("hollywood") {
            out.push_str(&trailing_blanks);
            trailing_blanks.clear();
            out.push_str(line);
            saw_root = true;
            in_root = true;
            continue;
        }
        if header_name(line).as_deref() == Some("hollywood") {
            if in_root {
                flush_missing(
                    &mut out,
                    &mut wrote_runner,
                    &mut wrote_model,
                    &mut wrote_effort,
                    &mut wrote_key,
                    &mut wrote_cast,
                );
                in_root = false;
            }
            out.push_str(&trailing_blanks);
            trailing_blanks.clear();
            out.push_str(line);
            continue;
        }
        if !in_root {
            out.push_str(line);
            continue;
        }
        if line.trim().is_empty() {
            trailing_blanks.push_str(line);
            continue;
        }
        out.push_str(&trailing_blanks);
        trailing_blanks.clear();
        match toml_kv_key(line).as_deref() {
            Some("runner") => {
                if !wrote_runner {
                    let _ = writeln!(out, "runner = {}", quoted(&cfg.runner));
                    wrote_runner = true;
                }
            }
            Some("model") => {
                if !wrote_model {
                    let _ = writeln!(out, "model = {}", quoted(&cfg.model));
                    wrote_model = true;
                }
            }
            Some("effort") => {
                if !wrote_effort {
                    if let Some(e) = cfg.effort.as_deref() {
                        let _ = writeln!(out, "effort = {}", quoted(e));
                    }
                    wrote_effort = true;
                }
            }
            Some("key") => {
                if !wrote_key {
                    match cfg.key.as_deref() {
                        Some(k) => {
                            let _ = writeln!(out, "key = {}", quoted(k));
                        }
                        None => out.push_str(line),
                    }
                    wrote_key = true;
                }
            }
            Some("default_cast") => {
                if !wrote_cast {
                    match cfg.default_cast.as_deref() {
                        Some(d) => {
                            let _ = writeln!(out, "default_cast = {}", quoted(d));
                        }
                        None => out.push_str(line),
                    }
                    wrote_cast = true;
                }
            }
            _ => out.push_str(line),
        }
    }

    if !saw_root {
        out.push_str(&trailing_blanks);
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&table(cfg));
    } else if in_root {
        flush_missing(
            &mut out,
            &mut wrote_runner,
            &mut wrote_model,
            &mut wrote_effort,
            &mut wrote_key,
            &mut wrote_cast,
        );
        out.push_str(&trailing_blanks);
    } else {
        out.push_str(&trailing_blanks);
    }
    out
}
