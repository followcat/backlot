//! Release notes fetched on demand from the GitHub releases API —
//! the same sanctioned network exception as the update check (same
//! opt-outs and timeout, no token). Notes are cached in
//! `update-check.json` keyed by the tag they were fetched for; the
//! modal opens instantly from cache and refreshes in the background.

// Test builds default to an offline fetch (`update_flow`), so the
// network half has no caller there and only the parser runs.
#![cfg_attr(test, allow(dead_code))]

use std::process::Stdio;
use std::time::Duration;

use crate::update_state::CachedRelease;

/// The 20 most recent releases — drafts and prereleases are filtered
/// locally so the endpoint never needs a token.
pub(crate) const RELEASES_URL: &str =
    "https://api.github.com/repos/followcat/hc-tui/releases?per_page=20";
/// The whole fetch — connect and response — bounded so a hanging
/// network cannot pin the spawn.
pub(crate) const FETCH_TIMEOUT: Duration = Duration::from_secs(5);
/// Notes bodies are a few KB; a runaway response is capped.
const MAX_BYTES: usize = 512 * 1024;

pub(crate) type Releases = Vec<CachedRelease>;

/// The one GitHub object shape we read — everything else is ignored.
#[derive(serde::Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    published_at: String,
    #[serde(default)]
    body: String,
}

/// Parse the releases JSON into cached entries: non-draft,
/// non-prerelease, newer than `current`, newest first.
pub(crate) fn parse_releases(json: &str, current: &str) -> Option<Releases> {
    let api: Vec<ApiRelease> = serde_json::from_str(json).ok()?;
    let current = crate::update_check::semver(current)?;
    let mut releases: Vec<CachedRelease> = api
        .into_iter()
        .filter(|release| !release.draft && !release.prerelease)
        .filter(|release| {
            crate::update_check::semver(&release.tag_name).is_some_and(|v| v > current)
        })
        .map(|release| CachedRelease {
            version: release
                .tag_name
                .trim()
                .trim_start_matches('v')
                .split(['-', '+'])
                .next()
                .unwrap_or_default()
                .to_owned(),
            date: release.published_at.chars().take(10).collect(),
            body: release.body,
        })
        .collect();
    releases.sort_by(|a, b| {
        crate::update_check::semver(&b.version).cmp(&crate::update_check::semver(&a.version))
    });
    Some(releases)
}

/// `GET` the releases API through curl: `-fsSL` fails on HTTP errors,
/// the timeout and size bounds are enforced locally. Every failure is
/// `None` — notes are best-effort, the modal shows its offline body.
pub(crate) async fn fetch_releases_json() -> Option<String> {
    let output = tokio::time::timeout(
        FETCH_TIMEOUT,
        tokio::process::Command::new("curl")
            .args([
                "-fsSL",
                "--connect-timeout",
                "5",
                "--max-time",
                "5",
                "-H",
                "Accept: application/vnd.github+json",
                RELEASES_URL,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() || output.stdout.len() > MAX_BYTES {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

/// Fetch + parse against the running version. `current` is injected
/// so tests never pretend the binary is something it is not.
pub(crate) async fn fetch_releases(current: &str) -> Option<Releases> {
    parse_releases(&fetch_releases_json().await?, current)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two newer releases, one older, one draft and one prerelease —
    /// the fixture shape of the real API payload with invented notes.
    const FIXTURE: &str = r####"[
        {
            "tag_name": "v999.0.2",
            "draft": false,
            "prerelease": false,
            "published_at": "2026-11-16T10:20:30Z",
            "body": "### Added\n- **Waiting status:** show `waiting` (#39)\n\nInstall: hc-tui --upgrade"
        },
        {
            "tag_name": "v999.0.3",
            "draft": true,
            "prerelease": false,
            "published_at": "2026-11-20T00:00:00Z",
            "body": "draft work"
        },
        {
            "tag_name": "v999.1.0",
            "draft": false,
            "prerelease": true,
            "published_at": "2026-11-25T00:00:00Z",
            "body": "release candidate"
        },
        {
            "tag_name": "v999.0.1",
            "draft": false,
            "prerelease": false,
            "published_at": "2026-11-10T08:00:00Z",
            "body": "### Fixed\n- A fix"
        },
        {
            "tag_name": "v0.0.1",
            "draft": false,
            "prerelease": false,
            "published_at": "2026-01-01T00:00:00Z",
            "body": "ancient"
        }
    ]"####;

    #[test]
    fn only_newer_stable_releases_survive_newest_first() {
        let releases = parse_releases(FIXTURE, "0.1.11").expect("fixture parses");
        let versions: Vec<&str> = releases.iter().map(|r| r.version.as_str()).collect();
        assert_eq!(versions, ["999.0.2", "999.0.1"]);
        assert_eq!(releases[0].date, "2026-11-16");
        assert_eq!(releases[1].date, "2026-11-10");
        assert!(releases[0].body.contains("### Added"));
    }

    #[test]
    fn drafts_and_prereleases_never_win_even_when_newer() {
        let releases = parse_releases(FIXTURE, "0.1.11").unwrap();
        assert!(releases.iter().all(|r| r.version != "999.0.3"));
        assert!(releases.iter().all(|r| r.version != "999.1.0"));
    }

    #[test]
    fn an_empty_result_when_nothing_is_newer() {
        let releases = parse_releases(FIXTURE, "999.9.9").unwrap();
        assert!(releases.is_empty());
    }

    #[test]
    fn garbage_json_is_a_silent_miss() {
        assert_eq!(parse_releases("{not json", "0.1.11"), None);
        assert_eq!(parse_releases("{}", "0.1.11"), None);
        assert_eq!(parse_releases("[]", "0.1.11"), Some(vec![]));
    }
}
