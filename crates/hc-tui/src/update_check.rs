//! One-shot "newer release" check against GitHub — a sanctioned
//! exception to the no-network rule, opt-out by config
//! (`update.version_check = false`) or `HC_TUI_NO_UPDATE_CHECK`. It is
//! kicked once terminal setup succeeded, resolves once with an update
//! notice or nothing, and is silent on every failure: no blocking, no
//! error surface, one real fetch per 24h at most.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hc_proto::endpoint::Env;
use tokio::sync::oneshot;

/// `releases/latest` redirects to `releases/tag/<tag>` — the same
/// endpoint `scripts/install.sh` resolves with a HEAD request and
/// `%{url_effective}`, so no JSON parsing is needed and pre-releases
/// are already excluded by GitHub's "latest" semantics.
const LATEST_URL: &str = "https://github.com/followcat/hc-tui/releases/latest";
/// At most one real fetch per this window; a cached tag still notices.
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// The whole fetch — connect and response — is bounded so a hanging
/// network cannot pin the spawn.
const FETCH_TIMEOUT: Duration = Duration::from_secs(5);
/// Presence is the opt-out (`HC_TUI_NO_UPDATE_CHECK=1` documented) —
/// matching the `HERDR_DISABLE_SOUND` kill-switch convention.
const ENV_OPT_OUT: &str = "HC_TUI_NO_UPDATE_CHECK";

/// A notice for the app to show: title and body.
pub type Notice = (String, String);
/// The one-shot report. `Some(notice)` only when a release newer than
/// `CARGO_PKG_VERSION` is known — freshly fetched or remembered.
pub type Report = oneshot::Receiver<Option<Notice>>;

/// Arm the check; `None` on either opt-out — then no task, socket or
/// wake is spent at all.
pub fn kick(env: &Env, enabled: bool) -> Option<Report> {
    if !enabled || env.get(OsStr::new(ENV_OPT_OUT)).is_some() {
        return None;
    }
    let cache = cache_path(env);
    let (tx, rx) = oneshot::channel();
    tokio::spawn(async move {
        let _ = tx.send(check(cache.as_deref(), fetch_latest_tag()).await);
    });
    Some(rx)
}

/// The check itself: a fresh cache answers without a fetch; otherwise
/// fetch, remember the tag, and compare to the running version.
async fn check(
    cache: Option<&Path>,
    tag: impl std::future::Future<Output = Option<String>>,
) -> Option<Notice> {
    let tag = match cache.and_then(cached_tag) {
        Some(tag) => tag,
        None => {
            let tag = tag.await?;
            if let Some(path) = cache {
                write_cache(path, &tag);
            }
            tag
        }
    };
    newer_release(&tag).map(|version| {
        (
            "Update available".into(),
            format!("hc-tui v{version} is out — run: hc-tui --upgrade"),
        )
    })
}

/// HEAD `releases/latest` through curl (the install script's resolver):
/// `-w %{url_effective}` prints the final URL, whose last path
/// component is the release tag. `-f` makes any HTTP failure a nonzero
/// exit; every failure mode returns `None` — the check is silent.
async fn fetch_latest_tag() -> Option<String> {
    let output = tokio::time::timeout(
        FETCH_TIMEOUT,
        tokio::process::Command::new("curl")
            .args([
                "-fsSIL",
                "-o",
                "/dev/null",
                "-w",
                "%{url_effective}",
                "--connect-timeout",
                "5",
                "--max-time",
                "5",
                LATEST_URL,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout);
    let tag = url.trim().rsplit('/').next()?.trim();
    (!tag.is_empty()).then(|| tag.to_owned())
}

/// The tag when it both parses as a version and was checked inside the
/// TTL — a poisoned or stale cache misses and refetches.
fn cached_tag(path: &Path) -> Option<String> {
    let parsed: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let checked_ms = parsed.get("checked_ms")?.as_u64()?;
    let tag = parsed.get("tag")?.as_str()?.to_owned();
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis() as u64;
    let fresh = now_ms.saturating_sub(checked_ms) < CACHE_TTL.as_millis() as u64;
    (fresh && semver(&tag).is_some()).then_some(tag)
}

/// Best effort — a cache miss costs nothing but the next fetch.
fn write_cache(path: &Path, tag: &str) {
    let Some(dir) = path.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let checked_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let _ = std::fs::write(
        path,
        serde_json::json!({ "tag": tag, "checked_ms": checked_ms }).to_string(),
    );
}

/// `$XDG_STATE_HOME/hc-tui/update-check.json`, falling back to
/// `~/.local/state` — `None` when neither root is set (the check still
/// runs; it just doesn't persist).
fn cache_path(env: &Env) -> Option<PathBuf> {
    let root = env
        .get(OsStr::new("XDG_STATE_HOME"))
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env.get(OsStr::new("HOME"))
                .filter(|v| !v.is_empty())
                .map(|home| PathBuf::from(home).join(".local/state"))
        })?;
    Some(root.join("hc-tui/update-check.json"))
}

/// The newest release's bare version when it outranks the running
/// `CARGO_PKG_VERSION`; `None` on equal/older/unparseable tags.
fn newer_release(tag: &str) -> Option<String> {
    let latest = semver(tag)?;
    let current = semver(env!("CARGO_PKG_VERSION"))?;
    (latest > current).then(|| {
        tag.trim()
            .trim_start_matches('v')
            .split(['-', '+'])
            .next()
            .unwrap_or_default()
            .to_owned()
    })
}

/// `vX.Y.Z` / `X.Y.Z` (suffixes like `-rc1`/`+build` dropped, missing
/// minor/patch read as zero). `None` on anything else.
fn semver(version: &str) -> Option<(u32, u32, u32)> {
    let core = version
        .trim()
        .trim_start_matches('v')
        .split(['-', '+'])
        .next()?;
    let mut parts = core.split('.');
    Some((
        parts.next()?.parse().ok()?,
        parts.next().unwrap_or("0").parse().ok()?,
        parts.next().unwrap_or("0").parse().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::io;

    /// A scratch directory that removes itself.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            loop {
                let dir = std::env::temp_dir().join(format!(
                    "hctui-updatecheck-{}-{}-{:x}",
                    std::process::id(),
                    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                ));
                match std::fs::create_dir(&dir) {
                    Ok(()) => return Scratch(dir),
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(e) => panic!("create scratch dir {}: {e}", dir.display()),
                }
            }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn env_with(pairs: &[(&str, &str)]) -> Env {
        pairs
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect()
    }

    fn tag(tag: &str) -> impl std::future::Future<Output = Option<String>> {
        let tag = tag.to_owned();
        async move { Some(tag) }
    }

    #[test]
    fn semver_parses_tags_and_versions() {
        assert_eq!(semver("v0.1.4"), Some((0, 1, 4)));
        assert_eq!(semver("0.2.0"), Some((0, 2, 0)));
        assert_eq!(semver("v1.0"), Some((1, 0, 0)));
        assert_eq!(semver("v0.2.0-rc1"), Some((0, 2, 0)));
        assert_eq!(semver("garbage"), None);
        assert_eq!(semver(""), None);
    }

    #[test]
    fn newer_release_compares_against_the_running_version() {
        assert_eq!(
            newer_release("v999.0.0"),
            Some("999.0.0".to_owned()),
            "a far-future tag must win over any current version"
        );
        let current = env!("CARGO_PKG_VERSION");
        assert_eq!(newer_release(&format!("v{current}")), None, "equal");
        assert_eq!(newer_release("v0.0.1"), None, "older");
        assert_eq!(newer_release("not-a-version"), None, "unparseable");
    }

    /// A fetched tag is remembered: the same check answers from cache
    /// and the fetched "newer" tag still raises the notice.
    #[tokio::test]
    async fn a_fresh_cache_answer_skips_the_fetch() {
        let scratch = Scratch::new();
        let cache = scratch.0.join("update-check.json");
        let notice = check(Some(&cache), tag("v999.0.0")).await;
        assert!(notice.is_some(), "the future tag should notify");
        assert!(notice.unwrap().1.contains("999.0.0"));
        assert!(cache.exists(), "the fetch was cached");

        // A cached "newer" tag still notifies — and never refetches
        // (a `None` source would yield no notice if it were used).
        let notice = check(Some(&cache), async { None }).await;
        assert!(notice.is_some(), "the cached tag still notifies");
    }

    /// A stale cache entry is ignored and refreshed.
    #[tokio::test]
    async fn a_stale_cache_refetches() {
        let scratch = Scratch::new();
        let cache = scratch.0.join("update-check.json");
        let old_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            - CACHE_TTL.as_millis() as u64
            - 1000;
        std::fs::write(
            &cache,
            serde_json::json!({ "tag": "v0.0.1", "checked_ms": old_ms }).to_string(),
        )
        .unwrap();
        assert!(cached_tag(&cache).is_none(), "past the TTL");

        let notice = check(Some(&cache), tag("v999.0.0")).await;
        assert!(notice.is_some());
        assert_eq!(cached_tag(&cache).as_deref(), Some("v999.0.0"));
    }

    /// A garbage cache entry is treated as absent.
    #[test]
    fn a_poisoned_cache_misses() {
        let scratch = Scratch::new();
        let cache = scratch.0.join("update-check.json");
        std::fs::write(&cache, "{not json").unwrap();
        assert_eq!(cached_tag(&cache), None);
        std::fs::write(&cache, r#"{"tag":"banana","checked_ms":99999999999999}"#).unwrap();
        assert_eq!(cached_tag(&cache), None, "unparseable tag refetches");
    }

    /// Both opt-outs disarm before anything spawns; `version_check =
    /// true` without the env var arms.
    #[tokio::test]
    async fn opt_outs_disarm_the_check() {
        assert!(kick(&env_with(&[]), false).is_none(), "config off");
        let env = env_with(&[("HC_TUI_NO_UPDATE_CHECK", "1")]);
        assert!(kick(&env, true).is_none(), "env off");
        let env = env_with(&[("HOME", "/scratch/home")]);
        assert!(kick(&env, true).is_some(), "armed by default");
    }

    #[test]
    fn cache_path_follows_xdg_then_home() {
        let env = env_with(&[("XDG_STATE_HOME", "/xdg"), ("HOME", "/home")]);
        assert_eq!(
            cache_path(&env),
            Some(PathBuf::from("/xdg/hc-tui/update-check.json"))
        );
        let env = env_with(&[("HOME", "/home")]);
        assert_eq!(
            cache_path(&env),
            Some(PathBuf::from("/home/.local/state/hc-tui/update-check.json"))
        );
        assert_eq!(cache_path(&env_with(&[])), None);
    }

    /// A fetch failure resolves to nothing — never an error, never a
    /// misleading notice.
    #[tokio::test]
    async fn a_failed_fetch_is_silent() {
        let scratch = Scratch::new();
        let cache = scratch.0.join("update-check.json");
        assert_eq!(check(Some(&cache), async { None }).await, None);
        assert!(!cache.exists(), "failures are not remembered");
    }
}
