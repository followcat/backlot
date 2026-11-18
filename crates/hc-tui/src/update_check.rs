//! One-shot "newer release" check against GitHub — a sanctioned
//! exception to the no-network rule, opt-out by config
//! (`update.version_check = false`) or `BACKLOT_NO_UPDATE_CHECK`. It is
//! kicked once terminal setup succeeded, resolves once with an update
//! notice or nothing, and is silent on every failure: no blocking, no
//! error surface, one real fetch per 24h at most.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use hc_proto::endpoint::Env;
use tokio::sync::oneshot;

/// `releases/latest` redirects to `releases/tag/<tag>` — the same
/// endpoint `scripts/install.sh` resolves with a HEAD request and
/// `%{url_effective}`, so no JSON parsing is needed and pre-releases
/// are already excluded by GitHub's "latest" semantics.
const LATEST_URL: &str = "https://github.com/followcat/backlot/releases/latest";
/// At most one real fetch per this window; a cached tag still notices.
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// The whole fetch — connect and response — is bounded so a hanging
/// network cannot pin the spawn.
const FETCH_TIMEOUT: Duration = Duration::from_secs(5);
/// Presence is the opt-out (`BACKLOT_NO_UPDATE_CHECK=1` documented) —
/// matching the `HERDR_DISABLE_SOUND` kill-switch convention.
const ENV_OPT_OUT: &str = "BACKLOT_NO_UPDATE_CHECK";

/// The shared toast wording — the report path and the badge's
/// re-raise click both use it. `prefix` is the live primary prefix
/// label (`ctrl+b`); the mobile body drops the "click" verb.
pub(crate) fn toast(version: &str, prefix: &str, mobile: bool) -> (String, String) {
    let body = if mobile {
        format!("backlot v{version} — {prefix} u or ↑v{version}")
    } else {
        format!("backlot v{version} — {prefix} u or click ↑v{version}")
    };
    ("Update available".into(), body)
}

/// The one-shot report: the bare latest version when it outranks the
/// running binary, plus the whole cache state so the app owns
/// skip/seen/notes persistence from one place.
pub struct CheckReport {
    /// Newer release's bare version (`0.1.13`), or `None`.
    pub latest: Option<String>,
    /// The cache as loaded (and possibly refreshed) — never `None` so
    /// the app always has somewhere to persist seen/skip flags.
    pub state: crate::update_state::UpdateCheckState,
    /// Where the state persists — `None` when no XDG/HOME root exists.
    pub path: Option<PathBuf>,
}

/// The one-shot report channel.
pub type Report = oneshot::Receiver<CheckReport>;

/// Arm the check; `None` on either opt-out — then no task, socket or
/// wake is spent at all.
pub fn kick(env: &Env, enabled: bool) -> Option<Report> {
    if !enabled || env.get(OsStr::new(ENV_OPT_OUT)).is_some() {
        return None;
    }
    let cache = cache_path(env);
    let (tx, rx) = oneshot::channel();
    tokio::spawn(async move {
        let _ = tx.send(check(cache, fetch_latest_tag()).await);
    });
    Some(rx)
}

/// The check itself: a fresh cache answers without a fetch; otherwise
/// fetch, remember the tag, and compare to the running version.
async fn check(
    cache: Option<PathBuf>,
    tag: impl std::future::Future<Output = Option<String>>,
) -> CheckReport {
    let mut state = cache
        .as_deref()
        .map(crate::update_state::load)
        .unwrap_or_default();
    let tag = match fresh_cached_tag(&state) {
        Some(tag) => tag,
        None => match tag.await {
            Some(tag) => {
                state.tag = Some(tag.clone());
                state.checked_ms = crate::update_state::now_ms();
                if let Some(path) = &cache {
                    crate::update_state::store(path, &state);
                }
                tag
            }
            None => {
                return CheckReport {
                    latest: None,
                    state,
                    path: cache,
                };
            }
        },
    };
    CheckReport {
        latest: newer_release(&tag),
        state,
        path: cache,
    }
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

/// The cached tag when it both parses as a version and was checked
/// inside the TTL — a stale or unparseable entry refetches.
fn fresh_cached_tag(state: &crate::update_state::UpdateCheckState) -> Option<String> {
    let tag = state.tag.clone()?;
    let fresh = crate::update_state::now_ms().saturating_sub(state.checked_ms)
        < CACHE_TTL.as_millis() as u64;
    (fresh && semver(&tag).is_some()).then_some(tag)
}

/// `$XDG_STATE_HOME/backlot/update-check.json`, falling back to
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
    Some(root.join("backlot/update-check.json"))
}

/// The newest release's bare version when it outranks the running
/// `CARGO_PKG_VERSION`; `None` on equal/older/unparseable tags.
fn newer_release(tag: &str) -> Option<String> {
    let (major, minor, patch) = semver(tag)?;
    let current = semver(env!("CARGO_PKG_VERSION"))?;
    ((major, minor, patch) > current).then(|| format!("{major}.{minor}.{patch}"))
}

/// `vX.Y.Z` / `X.Y.Z` — digits and dots only in the numeric core
/// (missing minor/patch read as zero), then an optional `-…`/`+…`
/// semver suffix of `0-9 A-Z a-z . + -`. Anything else — a fourth
/// field, an empty field, or another character — is `None`, so a
/// hostile tag can never parse its way into the installer's env.
pub(crate) fn semver(version: &str) -> Option<(u32, u32, u32)> {
    let trimmed = version.trim();
    let rest = trimmed.strip_prefix('v').unwrap_or(trimmed);
    let split = rest.find(['-', '+']).unwrap_or(rest.len());
    let (core, suffix) = rest.split_at(split);
    if !suffix.is_empty()
        && (suffix.len() == 1
            || !suffix[1..]
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".+-".contains(&b)))
    {
        return None;
    }
    let mut numbers = [0u32; 3];
    for (count, field) in core.split('.').enumerate() {
        if count == 3 || field.is_empty() || !field.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        numbers[count] = field.parse().ok()?;
    }
    Some((numbers[0], numbers[1], numbers[2]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::io;
    use std::time::{SystemTime, UNIX_EPOCH};

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
    /// and the fetched "newer" tag still reports latest.
    #[tokio::test]
    async fn a_fresh_cache_answer_skips_the_fetch() {
        let scratch = Scratch::new();
        let cache = scratch.0.join("update-check.json");
        let report = check(Some(cache.clone()), tag("v999.0.0")).await;
        assert_eq!(report.latest.as_deref(), Some("999.0.0"));
        assert!(cache.exists(), "the fetch was cached");

        // A cached "newer" tag still reports — and never refetches
        // (a `None` source would yield no latest if it were used).
        let report = check(Some(cache.clone()), async { None }).await;
        assert_eq!(report.latest.as_deref(), Some("999.0.0"));
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

        let report = check(Some(cache.clone()), tag("v999.0.0")).await;
        assert_eq!(report.latest.as_deref(), Some("999.0.0"));
        let state = crate::update_state::load(&cache);
        assert_eq!(state.tag.as_deref(), Some("v999.0.0"));
    }

    /// A garbage cache entry is treated as absent.
    #[test]
    fn a_poisoned_cache_misses() {
        let scratch = Scratch::new();
        let cache = scratch.0.join("update-check.json");
        std::fs::write(&cache, "{not json").unwrap();
        assert_eq!(crate::update_state::load(&cache).tag, None);
        std::fs::write(&cache, r#"{"tag":"banana","checked_ms":99999999999999}"#).unwrap();
        let state = crate::update_state::load(&cache);
        assert!(
            fresh_cached_tag(&state).is_none(),
            "unparseable tag refetches"
        );
    }

    /// Both opt-outs disarm before anything spawns; `version_check =
    /// true` without the env var arms.
    #[tokio::test]
    async fn opt_outs_disarm_the_check() {
        assert!(kick(&env_with(&[]), false).is_none(), "config off");
        let env = env_with(&[("BACKLOT_NO_UPDATE_CHECK", "1")]);
        assert!(kick(&env, true).is_none(), "env off");
        let env = env_with(&[("HOME", "/scratch/home")]);
        assert!(kick(&env, true).is_some(), "armed by default");
    }

    #[test]
    fn cache_path_follows_xdg_then_home() {
        let env = env_with(&[("XDG_STATE_HOME", "/xdg"), ("HOME", "/home")]);
        assert_eq!(
            cache_path(&env),
            Some(PathBuf::from("/xdg/backlot/update-check.json"))
        );
        let env = env_with(&[("HOME", "/home")]);
        assert_eq!(
            cache_path(&env),
            Some(PathBuf::from(
                "/home/.local/state/backlot/update-check.json"
            ))
        );
        assert_eq!(cache_path(&env_with(&[])), None);
    }

    /// A fetch failure resolves to nothing — never an error, never a
    /// misleading notice.
    #[tokio::test]
    async fn a_failed_fetch_is_silent() {
        let scratch = Scratch::new();
        let cache = scratch.0.join("update-check.json");
        let report = check(Some(cache.clone()), async { None }).await;
        assert_eq!(report.latest, None);
        assert!(!cache.exists(), "failures are not remembered");
    }
    /// Review round 1: a tag's numeric core is digits and dots only —
    /// a fourth field, an empty field or any other character rejects
    /// the tag, so a hostile ref can never reach the installer's env.
    #[test]
    fn semver_rejects_a_hostile_numeric_core() {
        assert_eq!(semver("v9.9.9.;id;"), None);
        assert_eq!(semver("v1.2.3.4"), None, "a fourth field");
        assert_eq!(semver("v1..2"), None, "an empty field");
        assert_eq!(semver("v1.2.3$(x)"), None);
        assert_eq!(semver("v1.2.3-;id;"), None, "the suffix stays semver-safe");
        assert_eq!(semver("v1.2.3-"), None, "an empty suffix");
        assert_eq!(
            semver(" v1.2.3"),
            Some((1, 2, 3)),
            "a padded tag normalizes"
        );
        assert_eq!(semver("v1.2.3-rc.1+build.2"), Some((1, 2, 3)));
    }

    /// The version the client keeps is rebuilt from the parsed numbers,
    /// never sliced out of the tag text.
    #[test]
    fn newer_release_rebuilds_the_version_from_numbers() {
        assert_eq!(newer_release("v999"), Some("999.0.0".to_owned()));
        assert_eq!(newer_release("v999.0.0-rc1"), Some("999.0.0".to_owned()));
        assert_eq!(newer_release("v9.9.9.;id;"), None);
        assert_eq!(newer_release("v999.0.0$(x)"), None);
    }

    /// A cached tag with shell metacharacters is not a version — the
    /// check refetches, and a hostile fetched tag is dropped too. It
    /// can never reach `update_run::plan`.
    #[tokio::test]
    async fn a_hostile_tag_never_reaches_the_kept_version() {
        let scratch = Scratch::new();
        let cache = scratch.0.join("update-check.json");
        std::fs::write(
            &cache,
            serde_json::json!({
                "tag": "v9.9.9.;id;",
                "checked_ms": crate::update_state::now_ms()
            })
            .to_string(),
        )
        .unwrap();
        let report = check(Some(cache), async { None }).await;
        assert_eq!(report.latest, None, "the hostile cached tag refetches");

        let scratch = Scratch::new();
        let cache = scratch.0.join("update-check.json");
        let report = check(Some(cache), tag("v9.9.9$(x)")).await;
        assert_eq!(report.latest, None, "a hostile fetched tag is dropped");
    }
}
