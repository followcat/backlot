//! `backlot hollywood show <seq>`: read-only lookup of one Hollywood
//! report or message from `<base>/hollywood/events.jsonl` so the Call
//! Sheet can open the full `--body` or message text in its `CardView`.

use std::path::{Component, Path, PathBuf};

use backlot_model::hollywood::HollywoodKind;

use crate::args::Common;
use crate::cli::{Env, Fail};
use crate::hwood::{base_of, store_fail};
use crate::store;

const MAX_BYTES: u64 = 1024 * 1024;

/// Look up `<seq>` in Hollywood's ledger and format its full content.
/// Reads raw events so entries older than reduced `HEADLINE_CAP` still
/// open cleanly; appends nothing.
pub fn show(common: &Common, env: &Env, seq: u64) -> Result<String, Fail> {
    let base = base_of(common, env)?;
    let hdir = store::hollywood_dir(&base);
    let (events, _) = store::load_hevents(&hdir).map_err(store_fail)?;
    let event = events
        .iter()
        .find(|e| e.seq == seq)
        .ok_or_else(|| Fail::refused(format!("no hollywood entry #{seq}")))?;
    match &event.kind {
        HollywoodKind::Reported { headline, body } => {
            let mut out = headline.clone();
            if let Some(rel) = body {
                let text = read_hollywood_file(&hdir, rel, "reports")?;
                let trimmed = text.trim_end_matches('\n');
                if !trimmed.is_empty() {
                    out.push_str("\n\n");
                    out.push_str(trimmed);
                }
            }
            Ok(out)
        }
        HollywoodKind::MessageSent { text, file, .. } => {
            if let Some(rel) = file {
                let body = read_hollywood_file(&hdir, rel, "messages")?;
                Ok(body.trim_end_matches('\n').to_string())
            } else if let Some(inline) = text {
                Ok(inline.trim_end_matches('\n').to_string())
            } else {
                Err(Fail::refused(format!(
                    "hollywood message #{seq} has no body"
                )))
            }
        }
        _ => Err(Fail::refused(format!("no hollywood entry #{seq}"))),
    }
}

/// Validate `raw` as a `<expected_root>/…` path strictly contained
/// inside `hdir` (no `..`, NUL, absolute paths, or escaping symlinks)
/// and read up to 1 MiB.
fn read_hollywood_file(hdir: &Path, raw: &str, expected_root: &str) -> Result<String, Fail> {
    let file = Path::new(raw);
    if raw.is_empty() || raw.contains('\0') || file.is_absolute() {
        return Err(Fail::refused(format!(
            "path '{raw}' must be a hollywood-relative path"
        )));
    }
    let parts: Vec<String> = file
        .components()
        .map(|c| match c {
            Component::Normal(p) => Ok(p.to_string_lossy().into_owned()),
            _ => Err(Fail::refused(format!("path '{raw}' must not contain '..'"))),
        })
        .collect::<Result<_, _>>()?;
    if parts.len() < 2 || parts[0] != expected_root {
        return Err(Fail::refused(format!(
            "path '{raw}' is not under {expected_root}/"
        )));
    }
    let mut rel = PathBuf::new();
    for part in &parts {
        rel.push(part);
    }
    let root = hdir
        .canonicalize()
        .map_err(|e| Fail::refused(e.to_string()))?;
    let real = hdir
        .join(&rel)
        .canonicalize()
        .map_err(|_| Fail::refused(format!("no file '{}'", rel.display())))?;
    if !real.starts_with(&root) || !real.is_file() {
        return Err(Fail::refused(format!(
            "'{}' is outside hollywood ledger",
            rel.display()
        )));
    }
    let bytes = real
        .metadata()
        .map_err(|e| Fail::refused(e.to_string()))?
        .len();
    if bytes > MAX_BYTES {
        return Err(Fail::refused(format!(
            "'{}' is {} bytes — the hollywood show limit is {} bytes",
            rel.display(),
            bytes,
            MAX_BYTES
        )));
    }
    let content = std::fs::read(&real).map_err(|e| Fail::refused(e.to_string()))?;
    Ok(String::from_utf8_lossy(&content).into_owned())
}
