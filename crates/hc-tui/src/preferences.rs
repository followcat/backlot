// Derived from herdr src/client/shell/preferences.rs (Apache-2.0).
//! Per-endpoint client chrome preferences: choices the user changed by
//! hand — sidebar width, the spaces/agents split, collapse state and the
//! agent-panel sort — remembered across restarts. One small JSON file
//! per endpoint under the client state root (`hc-tui/client-shell`); a
//! missing or malformed file is ignored and a write failure is a
//! notice, never a panic.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use hc_proto::endpoint::Env;
use serde::{Deserialize, Serialize};

use crate::config::AgentPanelSortConfig;

static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(1);

/// Collapsed workspace groups on one remote endpoint, in the file.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ClientRemoteCollapsedGroups {
    /// The remote machine's name (`host-b` in `host-b/t7`).
    pub profile_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collapsed_groups: Vec<String>,
}

/// One endpoint's remembered chrome choices. The `Option` fields are
/// `Some` only after the user changed them by hand — the persisted
/// `*_manual` flags — so a file written before a choice carries only
/// what was actually chosen.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ClientChromePreferences {
    /// The sidebar width the user last dragged to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar_width: Option<u16>,
    /// The spaces/agents split the user last dragged to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar_section_split: Option<f32>,
    /// The sidebar collapse state the user last toggled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar_collapsed: Option<bool>,
    /// The agent-panel sort the user last clicked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_panel_sort: Option<AgentPanelSortConfig>,
    /// Workspace groups collapsed on the local endpoint.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collapsed_groups: Vec<String>,
    /// Collapsed groups per remote machine.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remote_collapsed_groups: Vec<ClientRemoteCollapsedGroups>,
}

/// The resolved state root: `$XDG_STATE_HOME`, else `~/.local/state` —
/// the same root `update_check`'s cache resolves.
fn state_root(env: &Env) -> Option<PathBuf> {
    env.get(OsStr::new("XDG_STATE_HOME"))
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env.get(OsStr::new("HOME"))
                .filter(|v| !v.is_empty())
                .map(|home| PathBuf::from(home).join(".local/state"))
        })
}

fn preferences_dir(env: &Env) -> Option<PathBuf> {
    Some(state_root(env)?.join("hc-tui").join("client-shell"))
}

/// FNV-1a over a byte sequence — a stable, non-crypto hash so a file
/// name never embeds a socket path or ssh target.
fn fnv1a(bytes: impl IntoIterator<Item = u8>) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// The local endpoint's preferences file, keyed by its socket path.
pub fn path_for_local_endpoint(env: &Env, socket_path: &Path) -> Option<PathBuf> {
    let hash = fnv1a(socket_path.to_string_lossy().bytes());
    Some(preferences_dir(env)?.join(format!("local-{hash:016x}.json")))
}

/// A remote endpoint's preferences file, keyed by target + session —
/// the remote's own identity, length-prefixed so `(ab, c)` and
/// `(a, bc)` can never collide.
pub fn path_for_remote_endpoint(env: &Env, target: &str, session: &str) -> Option<PathBuf> {
    let mut bytes = Vec::new();
    for part in [target, session] {
        bytes.extend_from_slice(&(part.len() as u64).to_le_bytes());
        bytes.extend_from_slice(part.as_bytes());
    }
    let hash = fnv1a(bytes);
    Some(preferences_dir(env)?.join(format!("remote-{hash:016x}.json")))
}

/// The file's contents; `None` when it is missing, unreadable or
/// malformed — the caller then starts from config defaults rather
/// than trusting a half-written state.
pub fn load(path: &Path) -> Option<ClientChromePreferences> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Rewrites the file atomically (temp file + rename). An error leaves
/// any partial temp file removed and is reported to the caller.
pub fn store(path: &Path, preferences: &ClientChromePreferences) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("invalid client shell state path: {}", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create client shell state directory: {error}"))?;
    let content = serde_json::to_vec_pretty(preferences)
        .map_err(|error| format!("failed to encode client shell state: {error}"))?;
    let sequence = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
    let mut temp_name = path
        .file_name()
        .ok_or_else(|| format!("invalid client shell state path: {}", path.display()))?
        .to_os_string();
    temp_name.push(format!(".tmp-{}-{sequence}", std::process::id()));
    let temp_path = parent.join(temp_name);
    std::fs::write(&temp_path, content)
        .map_err(|error| format!("failed to write client shell state: {error}"))?;
    std::fs::rename(&temp_path, path).map_err(|error| {
        let _ = std::fs::remove_file(&temp_path);
        format!("failed to replace client shell state: {error}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestDir;
    use std::ffi::OsString;

    fn env_with(vars: &[(&str, &str)]) -> Env {
        vars.iter()
            .map(|(key, value)| (OsString::from(key), OsString::from(value)))
            .collect()
    }

    #[test]
    fn endpoint_paths_are_stable_and_distinct() {
        let env = env_with(&[("XDG_STATE_HOME", "/state")]);
        let first = path_for_local_endpoint(&env, Path::new("/run/hc/one.sock")).unwrap();
        let again = path_for_local_endpoint(&env, Path::new("/run/hc/one.sock")).unwrap();
        let second = path_for_local_endpoint(&env, Path::new("/run/hc/two.sock")).unwrap();
        assert_eq!(first, again);
        assert_ne!(first, second);
        assert_eq!(
            first.parent().unwrap(),
            Path::new("/state/hc-tui/client-shell")
        );
    }

    #[test]
    fn remote_preferences_distinguish_targets_sessions_and_component_boundaries() {
        let env = env_with(&[("XDG_STATE_HOME", "/state")]);
        let original = path_for_remote_endpoint(&env, "dev@host-a", "agents").unwrap();
        assert_eq!(
            original,
            path_for_remote_endpoint(&env, "dev@host-a", "agents").unwrap()
        );
        assert_ne!(
            original,
            path_for_remote_endpoint(&env, "other@host-a", "agents").unwrap()
        );
        assert_ne!(
            original,
            path_for_remote_endpoint(&env, "dev@host-a", "other").unwrap()
        );
        assert_ne!(
            path_for_remote_endpoint(&env, "ab", "c").unwrap(),
            path_for_remote_endpoint(&env, "a", "bc").unwrap()
        );
        assert_ne!(
            original,
            path_for_local_endpoint(&env, Path::new("dev@host-a/agents")).unwrap()
        );
    }

    #[test]
    fn state_root_prefers_xdg_then_home() {
        assert_eq!(
            path_for_local_endpoint(
                &env_with(&[("XDG_STATE_HOME", "/state"), ("HOME", "/home")]),
                Path::new("/s.sock")
            )
            .unwrap()
            .parent()
            .unwrap(),
            Path::new("/state/hc-tui/client-shell")
        );
        assert_eq!(
            path_for_local_endpoint(&env_with(&[("HOME", "/home")]), Path::new("/s.sock"))
                .unwrap()
                .parent()
                .unwrap(),
            Path::new("/home/.local/state/hc-tui/client-shell")
        );
        assert!(path_for_local_endpoint(&env_with(&[]), Path::new("/s.sock")).is_none());
    }

    #[test]
    fn legacy_preferences_default_remote_collapses() {
        let preferences: ClientChromePreferences =
            serde_json::from_str(r#"{"collapsed_groups":["/repo"]}"#)
                .expect("legacy client chrome preferences");

        assert_eq!(preferences.collapsed_groups, ["/repo"]);
        assert!(preferences.remote_collapsed_groups.is_empty());
    }

    #[test]
    fn malformed_and_missing_files_load_as_none() {
        let dir = TestDir::new("hctui-prefs-load");
        let path = dir.join("malformed.json");
        std::fs::write(&path, "not json {").unwrap();
        assert!(load(&path).is_none());
        assert!(load(&dir.join("missing.json")).is_none());
    }

    #[test]
    fn repeated_store_replaces_existing_preferences() {
        let dir = TestDir::new("hctui-prefs-store");
        let path = dir.join("prefs.json");
        store(
            &path,
            &ClientChromePreferences {
                sidebar_width: Some(24),
                ..ClientChromePreferences::default()
            },
        )
        .expect("first preference store");
        store(
            &path,
            &ClientChromePreferences {
                sidebar_width: Some(32),
                ..ClientChromePreferences::default()
            },
        )
        .expect("replacement preference store");
        assert_eq!(load(&path).and_then(|saved| saved.sidebar_width), Some(32));
    }

    #[test]
    fn concurrent_stores_leave_complete_preferences() {
        let dir = TestDir::new("hctui-prefs-concurrent");
        let path = dir.join("prefs.json");
        let writers = (20..28)
            .map(|width| {
                let path = path.clone();
                std::thread::spawn(move || {
                    store(
                        &path,
                        &ClientChromePreferences {
                            sidebar_width: Some(width),
                            ..ClientChromePreferences::default()
                        },
                    )
                })
            })
            .collect::<Vec<_>>();
        for writer in writers {
            writer.join().expect("preference writer").expect("store");
        }
        assert!(
            load(&path)
                .and_then(|saved| saved.sidebar_width)
                .is_some_and(|width| (20..28).contains(&width))
        );
    }
}
