//! The `update-check.json` cache model — shared client-wide on the
//! local machine (never per endpoint). New fields are
//! `#[serde(default)]` so a file written by an older backlot still
//! loads; writes are atomic (temp file + rename) so a crashed client
//! never leaves a torn cache.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// A cached release entry — one GitHub release newer than the running
/// binary at fetch time, notes body included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedRelease {
    /// Bare version (`0.1.13`, no `v`).
    pub version: String,
    /// ISO date (`YYYY-MM-DD`) sliced from `published_at`.
    pub date: String,
    /// Raw Markdown release notes.
    #[serde(default)]
    pub body: String,
}

/// The whole cache. `tag`/`checked_ms` are the fields the original
/// check wrote; the rest extend it backward-compatibly.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateCheckState {
    /// Latest release tag as fetched (`v0.1.13`).
    #[serde(default)]
    pub tag: Option<String>,
    /// When `tag` was fetched (ms since epoch) — the 24h TTL clock.
    #[serde(default)]
    pub checked_ms: u64,
    /// Version whose startup toast already fired once.
    #[serde(default)]
    pub seen_toast_version: Option<String>,
    /// Version the user skipped (`s skip` in the modal).
    #[serde(default)]
    pub skipped_version: Option<String>,
    /// The tag `releases` was fetched for — notes stay keyed to the
    /// release set that produced them.
    #[serde(default)]
    pub releases_for_tag: Option<String>,
    /// Release notes for `releases_for_tag`, newest first.
    #[serde(default)]
    pub releases: Vec<CachedRelease>,
    /// Version whose "what's new" view was already shown (or marked
    /// seen before an in-client restart).
    #[serde(default)]
    pub show_on_startup_version: Option<String>,
}

/// Best effort read — a missing or poisoned file is an empty cache.
/// Release fields render in the card, so a cache written before they
/// were cleaned (or hand-edited) is cleaned on the way in.
pub fn load(path: &Path) -> UpdateCheckState {
    let Ok(text) = std::fs::read_to_string(path) else {
        return UpdateCheckState::default();
    };
    serde_json::from_str(&text)
        .map(|mut state: UpdateCheckState| {
            for release in &mut state.releases {
                release.version = crate::sidebar::text::clean_ansi(&release.version);
                release.date = crate::sidebar::text::clean_ansi(&release.date);
                release.body = crate::sidebar::text::clean_lines(&release.body);
            }
            state
        })
        .unwrap_or_default()
}

/// Atomic write: temp file + rename, as `preferences::store` does — a
/// reader never sees a torn JSON. Best effort: a failed write leaves
/// the next run to re-fetch.
pub fn store(path: &Path, state: &UpdateCheckState) {
    let Some(dir) = path.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let tmp = dir.join(format!(".update-check.{}.tmp", std::process::id()));
    let Ok(mut file) = std::fs::File::create(&tmp) else {
        return;
    };
    use std::io::Write;
    let written = serde_json::to_string_pretty(state)
        .ok()
        .and_then(|text| file.write_all(text.as_bytes()).ok())
        .is_some();
    drop(file);
    if written {
        let _ = std::fs::rename(&tmp, path);
    } else {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// ms since the Unix epoch for `checked_ms`.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;

    #[test]
    fn an_old_cache_file_still_loads() {
        let dir = TestDir::new("hctui-updatestate-old");
        let path = dir.join("update-check.json");
        // Written before the extension fields existed.
        std::fs::write(&path, r#"{"tag":"v0.1.11","checked_ms":1763326800000}"#).unwrap();
        let state = load(&path);
        assert_eq!(state.tag.as_deref(), Some("v0.1.11"));
        assert_eq!(state.checked_ms, 1763326800000);
        assert_eq!(state.seen_toast_version, None);
        assert_eq!(state.skipped_version, None);
        assert_eq!(state.releases_for_tag, None);
        assert!(state.releases.is_empty());
        assert_eq!(state.show_on_startup_version, None);
    }

    #[test]
    fn a_missing_or_poisoned_file_is_empty() {
        let dir = TestDir::new("hctui-updatestate");
        let path = dir.join("absent.json");
        assert_eq!(load(&path), UpdateCheckState::default());
        let path = dir.join("bad");
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(load(&path), UpdateCheckState::default());
    }

    #[test]
    fn store_round_trips_every_field() {
        let dir = TestDir::new("hctui-updatestate-rt");
        let path = dir.join("nested/update-check.json");
        let state = UpdateCheckState {
            tag: Some("v0.1.13".into()),
            checked_ms: 1763326800000,
            seen_toast_version: Some("0.1.13".into()),
            skipped_version: Some("0.1.12".into()),
            releases_for_tag: Some("v0.1.13".into()),
            releases: vec![CachedRelease {
                version: "0.1.13".into(),
                date: "2026-11-16".into(),
                body: "### Added\n- One".into(),
            }],
            show_on_startup_version: Some("0.1.13".into()),
        };
        store(&path, &state);
        assert_eq!(load(&path), state);
        // A later write keeps only the persisted fields it changed.
        let mut next = state.clone();
        next.skipped_version = None;
        store(&path, &next);
        assert_eq!(load(&path), next);
    }
    /// A release written before bodies were cleaned — or a hand-edited
    /// cache — loads without its controls or sequences.
    #[test]
    fn a_poisoned_cached_release_loads_clean() {
        let dir = TestDir::new("hctui-updatestate-clean");
        let path = dir.join("update-check.json");
        std::fs::write(
            &path,
            r#"{"releases":[{"version":"9.9.9\u001b[2J","date":"2026-01\u000701","body":"a\u001b]52;c;AAAA\u0007b\nc\u009bd"}]}"#,
        )
        .unwrap();
        let state = load(&path);
        let release = &state.releases[0];
        for text in [&release.version, &release.date] {
            assert!(!text.chars().any(char::is_control), "{text:?}");
        }
        assert!(!release.body.chars().any(|c| c.is_control() && c != '\n'));
        assert_eq!(release.version, "9.9.9");
        assert_eq!(release.date, "2026-0101");
        assert_eq!(release.body, "ab\ncd");
    }
}
