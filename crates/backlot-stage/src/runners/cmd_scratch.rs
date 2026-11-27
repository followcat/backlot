//! Command Code (`cmd`) worktree scratchpad path computation.
//!
//! Mirrors `scratchpadBaseDir` and `sanitizeCwdComponent` in
//! `command-code` `1.93.0` (`dist/cli.mjs` line 2) so the `"worktree"`
//! profile can pre-allow only the role's own worktree scratch directory
//! without opening the shared `/tmp/commandcode-<uid>/` parent.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const MAX_COMPONENT_LEN: usize = 200;
const BASE64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn encode_base64url_prefix_8(digest: &[u8; 32]) -> String {
    let mut out = String::with_capacity(8);
    for chunk in digest[..6].as_chunks::<3>().0 {
        let n = ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | (chunk[2] as u32);
        out.push(BASE64URL[((n >> 18) & 0x3f) as usize] as char);
        out.push(BASE64URL[((n >> 12) & 0x3f) as usize] as char);
        out.push(BASE64URL[((n >> 6) & 0x3f) as usize] as char);
        out.push(BASE64URL[(n & 0x3f) as usize] as char);
    }
    out
}

/// Command Code's `sanitizeCwdComponent(cwd)`: replace every non-`[A-Za-z0-9]`
/// char with `-`; keep up to 200 chars verbatim (preserving case, camelCase,
/// and leading `-`); longer inputs keep 200 sanitized chars plus `"-"` and 8
/// base64url chars of `sha256(cwd)`.
pub(crate) fn sanitize_cwd_component(cwd: &str) -> String {
    let replaced: String = cwd
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    if !replaced.is_empty() && replaced.len() <= MAX_COMPONENT_LEN {
        return replaced;
    }
    let digest = backlot_model::hollywood::sha256(cwd.as_bytes());
    let hash = encode_base64url_prefix_8(&digest);
    let prefix = if replaced.is_empty() {
        "root"
    } else {
        &replaced[..replaced.len().min(MAX_COMPONENT_LEN)]
    };
    format!("{prefix}-{hash}")
}

fn container_dir(stage_env: &BTreeMap<String, String>) -> PathBuf {
    #[cfg(unix)]
    let folder = format!("commandcode-{}", unsafe { libc::getuid() });
    #[cfg(not(unix))]
    let folder = "commandcode".to_string();

    let base = stage_env
        .get("COMMANDCODE_SCRATCHPAD_BASE")
        .filter(|s| !s.is_empty() && Path::new(s).is_absolute())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join(folder)
}

/// Command Code's worktree-scoped parent scratch directory
/// (`/tmp/commandcode-<uid>/<encoded_cwd>`), or `None` when `cwd` is empty
/// or relative. Never returns `/tmp/commandcode-<uid>` itself.
pub(crate) fn worktree_dir(cwd: &str, stage_env: &BTreeMap<String, String>) -> Option<String> {
    if cwd.is_empty() || !Path::new(cwd).is_absolute() {
        return None;
    }
    let component = sanitize_cwd_component(cwd);
    if component.is_empty() {
        return None;
    }
    Some(
        container_dir(stage_env)
            .join(component)
            .to_string_lossy()
            .into_owned(),
    )
}
