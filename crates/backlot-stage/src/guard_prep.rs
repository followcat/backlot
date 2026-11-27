//! Per-role guard directory preparation: PATH shims, `pre-push` hook,
//! passthrough git hooks, and `GIT_CONFIG_*` environment setup.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::guard::SHIM_MARKER;
use crate::runners::executable;
use crate::store;

/// Locate the real executable `name` on `path_env`, skipping `shim_dir`
/// (both lexically and canonically) and any generated backlot shim file.
pub(crate) fn find_real_binary(
    name: &str,
    path_env: Option<&str>,
    shim_dir: Option<&Path>,
) -> Option<PathBuf> {
    let path_env = path_env?;
    let canon_shim = shim_dir.and_then(|d| std::fs::canonicalize(d).ok());
    for entry in path_env.split(':').filter(|s| !s.is_empty()) {
        let dir = Path::new(entry);
        if let Some(sd) = shim_dir
            && dir == sd
        {
            continue;
        }
        if let (Some(cs), Ok(cd)) = (&canon_shim, std::fs::canonicalize(dir))
            && cd == *cs
        {
            continue;
        }
        let candidate = dir.join(name);
        if executable(&candidate) && !is_guard_shim_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// Remove `extra_bin` and any directory containing a backlot guard shim
/// from a `:`-separated `PATH` string.
pub(crate) fn strip_shim_dirs(path_env: &str, extra_bin: Option<&Path>) -> String {
    let canon_extra = extra_bin.and_then(|p| std::fs::canonicalize(p).ok());
    let filtered: Vec<&str> = path_env
        .split(':')
        .filter(|seg| !seg.is_empty())
        .filter(|seg| {
            let dir = Path::new(seg);
            if let Some(eb) = extra_bin
                && dir == eb
            {
                return false;
            }
            if let (Some(ce), Ok(cd)) = (&canon_extra, std::fs::canonicalize(dir))
                && cd == *ce
            {
                return false;
            }
            !is_guard_shim_file(&dir.join("git")) && !is_guard_shim_file(&dir.join("gh"))
        })
        .collect();
    filtered.join(":")
}

pub(crate) fn is_guard_shim_file(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = [0u8; 128];
    let Ok(n) = f.read(&mut buf) else {
        return false;
    };
    std::str::from_utf8(&buf[..n]).is_ok_and(|head| head.contains(SHIM_MARKER))
}

/// Per-role guard root for a studio role: `<state>/studios/<studio>/roles/<role>`.
pub(crate) fn studio_role_dir(state_dir: &str, studio: &str, role: &str) -> PathBuf {
    store::studio_dir(Path::new(state_dir), studio)
        .join("roles")
        .join(role)
}

/// Per-role guard root for Hollywood: `<state>/hollywood/roles/hollywood`.
pub(crate) fn hollywood_role_dir(state_dir: &str) -> PathBuf {
    Path::new(state_dir)
        .join("hollywood")
        .join("roles")
        .join(backlot_model::hollywood::HOLLYWOOD)
}

/// Commands wrapped by per-role PATH shims in `<role>/bin`.
const SHIM_NAMES: &[&str] = &[
    "git",
    "gh",
    "hc",
    "scp",
    "rsync",
    "ssh",
    "npm",
    "cargo",
    "socat",
    "nc",
    "ncat",
    "systemd-run",
    "tmux",
];

/// Standard client-side git hooks wrapped for pass-through when a
/// role's `GIT_CONFIG_*` sets `core.hooksPath` to `<role>/git-hooks`.
const GIT_PASSTHROUGH_HOOKS: &[&str] = &[
    "applypatch-msg",
    "pre-applypatch",
    "post-applypatch",
    "pre-commit",
    "pre-merge-commit",
    "prepare-commit-msg",
    "commit-msg",
    "post-commit",
    "pre-rebase",
    "post-checkout",
    "post-merge",
    "post-rewrite",
];

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn passthrough_hook_script(hook: &str) -> String {
    format!(
        "#!/bin/sh\n\
         # backlot-guard-hook\n\
         hp=$(GIT_CONFIG_COUNT=0 git config --get core.hooksPath 2>/dev/null)\n\
         if [ -n \"$hp\" ]; then\n\
           case \"$hp\" in\n\
             /*) hook=\"$hp/{hook}\" ;;\n\
             *)\n\
               top=$(git rev-parse --show-toplevel 2>/dev/null)\n\
               hook=\"${{top:-.}}/$hp/{hook}\"\n\
               ;;\n\
           esac\n\
         else\n\
           common=$(git rev-parse --git-common-dir 2>/dev/null)\n\
           [ -n \"$common\" ] && hook=\"$common/hooks/{hook}\"\n\
         fi\n\
         if [ -n \"$hook\" ] && [ \"$hook\" != \"$0\" ] && [ -x \"$hook\" ]; then\n\
           exec \"$hook\" \"$@\"\n\
         fi\n\
         exit 0\n"
    )
}

/// Write per-role shims (`<role_dir>/bin/*`) and git hooks
/// (`<role_dir>/git-hooks/*`), rewritten before every launch/resume.
pub(crate) fn prepare_role(role_dir: &Path, exe_dir: &str) -> std::io::Result<()> {
    prepare_role_with(&crate::driver::LocalDriver::new(), role_dir, exe_dir)
}

/// [`prepare_role`] through a [`MachineDriver`], so each shim and hook
/// lands on the machine the role runs on.
pub(crate) fn prepare_role_with(
    driver: &dyn crate::driver::MachineDriver,
    role_dir: &Path,
    exe_dir: &str,
) -> std::io::Result<()> {
    let backlot_bin = Path::new(exe_dir).join("backlot");
    let quoted_bin = shell_quote(&backlot_bin.to_string_lossy());

    let bin_dir = role_dir.join("bin");
    driver.create_dir_all(&bin_dir)?;
    for name in SHIM_NAMES {
        let body = format!("#!/bin/sh\n{SHIM_MARKER}\nexec {quoted_bin} guard {name} -- \"$@\"\n");
        driver.write_executable(&bin_dir.join(name), &body)?;
    }

    let hooks_dir = role_dir.join("git-hooks");
    driver.create_dir_all(&hooks_dir)?;
    let pre_push =
        format!("#!/bin/sh\n{SHIM_MARKER}\nexec {quoted_bin} guard git-push -- \"$@\"\n");
    driver.write_executable(&hooks_dir.join("pre-push"), &pre_push)?;

    for hook in GIT_PASSTHROUGH_HOOKS {
        driver.write_executable(&hooks_dir.join(hook), &passthrough_hook_script(hook))?;
    }
    Ok(())
}

/// Prepend `<role_dir>/bin` to `PATH` ahead of `exe_dir`, and append
/// `core.hooksPath=<role_dir>/git-hooks` in `GIT_CONFIG_*` without
/// clobbering existing entries.
pub(crate) fn apply_env(
    env: &mut BTreeMap<String, String>,
    role_dir: &Path,
    exe_dir: &str,
    path_env: &str,
    configured_path: Option<&str>,
) {
    let base_path = match configured_path {
        Some(p) => format!("{exe_dir}:{p}"),
        None => path_env.to_owned(),
    };
    env.insert(
        "PATH".into(),
        format!("{}:{base_path}", role_dir.join("bin").display()),
    );

    let hooks_dir = role_dir.join("git-hooks");
    let mut n = env
        .get("GIT_CONFIG_COUNT")
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while env.contains_key(&format!("GIT_CONFIG_KEY_{n}"))
        || env.contains_key(&format!("GIT_CONFIG_VALUE_{n}"))
    {
        n += 1;
    }
    env.insert("GIT_CONFIG_COUNT".into(), (n + 1).to_string());
    env.insert(format!("GIT_CONFIG_KEY_{n}"), "core.hooksPath".into());
    env.insert(
        format!("GIT_CONFIG_VALUE_{n}"),
        hooks_dir.to_string_lossy().into_owned(),
    );
}
