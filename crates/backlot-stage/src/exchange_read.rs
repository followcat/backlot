//! Stage-side snippet reader for `StudioSummary.exchange`: reads at
//! most the first 4 KiB of the ≤ `MAX_STUDIO_EXCHANGE` user-facing
//! `messages/…` and `reports/…` files per studio, strictly validated
//! inside `<base>/studios/<studio>/` via `report_relpath` +
//! `canonicalize()`, and caches immutable `(studio, seq)` snippets.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::Path;

use backlot_model::state::StudioState;
use backlot_model::summary::{HEADLINE_CHARS, studio_exchange_files};

use crate::store;

const SNIPPET_READ_CAP: u64 = 4096;

/// Extract a single clean preview line from a studio-relative
/// `messages/…` or `reports/…` file, or `None` if missing, empty, or
/// outside `dir`.
pub(crate) fn read_snippet(dir: &Path, raw: &str) -> Option<String> {
    let rel = crate::hwood_read::report_relpath(Path::new(raw)).ok()?;
    let root = dir.canonicalize().ok()?;
    let real = dir.join(&rel).canonicalize().ok()?;
    if !real.starts_with(&root) || !real.is_file() {
        return None;
    }
    let file = std::fs::File::open(&real).ok()?;
    let mut bytes = Vec::new();
    file.take(SNIPPET_READ_CAP).read_to_end(&mut bytes).ok()?;
    let content = String::from_utf8_lossy(&bytes);
    first_meaningful_line(&content)
}

/// First non-empty line, stripping a leading Markdown ATX heading
/// prefix (`# `, `## `, …) while keeping bare `#7` issue/seq tokens.
fn first_meaningful_line(content: &str) -> Option<String> {
    for raw_line in content.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let without_hashes = trimmed.trim_start_matches('#');
        let line = if without_hashes.len() < trimmed.len()
            && without_hashes.starts_with(char::is_whitespace)
        {
            without_hashes.trim_start()
        } else {
            trimmed
        };
        let cleaned: String = line
            .chars()
            .filter(|c| !c.is_control())
            .take(HEADLINE_CHARS)
            .collect();
        if !cleaned.is_empty() {
            return Some(cleaned);
        }
    }
    None
}

/// Build `(studio, seq) -> snippet` for all active exchange entries,
/// populating and pruning `cache` so each immutable ledger copy is
/// read from disk at most once.
pub(crate) fn studio_snippets(
    base: &Path,
    studios: &[(String, StudioState)],
    cache: &mut BTreeMap<(String, u64), String>,
) -> BTreeMap<(String, u64), String> {
    let mut needed = BTreeSet::new();
    let mut out = BTreeMap::new();
    for (name, state) in studios {
        if crate::cli::valid_studio_name(name) {
            let dir = store::studio_dir(base, name);
            for (seq, rel) in studio_exchange_files(state) {
                let key = (name.clone(), seq);
                needed.insert(key.clone());
                if let Some(hit) = cache.get(&key) {
                    out.insert(key, hit.clone());
                } else if let Some(snippet) = read_snippet(&dir, &rel) {
                    cache.insert(key.clone(), snippet.clone());
                    out.insert(key, snippet);
                }
            }
        }
    }
    cache.retain(|k, _| needed.contains(k));
    out
}
