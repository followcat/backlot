// Derived from herdr-core crates/herdr-core/src/{client,machines}.rs and
// platform/unix.rs at ec8adcdf (Apache-2.0).
//
//! The shared ssh control-socket directory every remote stream asks for:
//! one private 0700 dir per user so a burst of streams rides one login,
//! and `None` — always off Unix, or when no private directory is usable —
//! leaves every stream its own login.

#[cfg(unix)]
use crate::endpoint::Env;
#[cfg(unix)]
use std::ffi::OsStr;
use std::path::PathBuf;

/// The control-socket directory every remote stream shares its login
/// through: `$XDG_RUNTIME_DIR/hc-tui-ssh` when the variable is set, else
/// `hc-tui-ssh-<uid>` under the temp dir (`/tmp` on macOS). `None` —
/// always off Unix, or whenever no private directory is usable — leaves
/// every stream its own login (the user's ssh config may still share it).
pub(super) fn control_dir() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        control_dir_in(&crate::endpoint::env_snapshot())
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// Resolves and prepares the control dir for `env`.
#[cfg(unix)]
fn control_dir_in(env: &Env) -> Option<PathBuf> {
    let dir = match env.get(OsStr::new("XDG_RUNTIME_DIR")) {
        Some(runtime) if !runtime.is_empty() => {
            PathBuf::from(runtime.as_os_str()).join("hc-tui-ssh")
        }
        _ => control_parent().join(format!("hc-tui-ssh-{}", unsafe { libc::getuid() })),
    };
    private_dir(dir)
}

/// The fallback parent of the control dir. macOS has no
/// `XDG_RUNTIME_DIR`, and `$TMPDIR` is a long per-user path that pushes
/// `dir/%C` past `CONTROL_PATH_MAX` — `control_options` would drop the
/// options and every stream would silently become its own SSH login.
/// `/tmp` keeps the socket name short; `private_dir` still enforces a
/// user-owned 0700 directory.
#[cfg(target_os = "macos")]
fn control_parent() -> PathBuf {
    PathBuf::from("/tmp")
}

/// Everywhere else the system temp dir is short enough.
#[cfg(all(unix, not(target_os = "macos")))]
fn control_parent() -> PathBuf {
    std::env::temp_dir()
}

/// Creates `dir` private to this user (0700) and returns it, or `None`
/// when it exists and this user does not own it or group/other can
/// write it — a control socket in such a directory could be taken over.
#[cfg(unix)]
fn private_dir(dir: PathBuf) -> Option<PathBuf> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let safe = |meta: &std::fs::Metadata| {
        meta.is_dir()
            && meta.uid() == unsafe { libc::getuid() }
            && meta.permissions().mode() & 0o022 == 0
    };
    if let Ok(meta) = std::fs::metadata(&dir)
        && !safe(&meta)
    {
        return None;
    }
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).ok()?;
    let meta = std::fs::metadata(&dir).ok()?;
    safe(&meta).then_some(dir)
}
