//! Preflight execution for proposed checks in `design write --slot buildability`
//! and `studio check add --preflight`. Runs each check once under a stripped
//! environment; director preflights additionally enforce the director role's
//! guard shims (`<role_dir>/bin` first on `PATH` and `core.hooksPath`) without
//! leaking `BACKLOT_*` or `HC_*` secrets.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use backlot_model::event::CheckDecl;
use backlot_model::state::StudioConfig;

use crate::checks;
use crate::cli::{Env, Fail};
use crate::guard;
use crate::guard_prep::strip_shim_dirs;
use crate::hcard_outward_env::cap_tail;
use crate::workareas;

/// Short per-check timeout cap for opening-design preflights (120s).
pub(crate) const PREFLIGHT_TIMEOUT_S: u64 = 120;

fn resolve_exe_dir(path_env: &str) -> String {
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
        && crate::runners::executable(&parent.join("backlot"))
    {
        return parent.to_string_lossy().into_owned();
    }
    for seg in path_env.split(':').filter(|s| !s.is_empty()) {
        let dir = Path::new(seg);
        if crate::runners::executable(&dir.join("backlot")) {
            return seg.to_string();
        }
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_string_lossy().into_owned()))
        .unwrap_or_default()
}

/// Build the stripped, director-guarded environment for preflight execution:
/// 1) Strip secrets / `BACKLOT_*` / `HC_*` via [`checks::env_for`].
/// 2) Strip any inherited guard shim dirs from `PATH`, then apply the director's
///    guard via [`guard::apply_env`] so `<studio>/roles/director/bin` leads `PATH`
///    and `core.hooksPath` is appended in `GIT_CONFIG_*`.
pub(crate) fn build_director_preflight_env(
    studio_dir: &Path,
    stage_env: &BTreeMap<String, String>,
    check_env: &[String],
) -> Result<Vec<(String, String)>, Fail> {
    let mut map: BTreeMap<String, String> =
        checks::env_for(stage_env, check_env).into_iter().collect();
    let raw_path = map
        .get("PATH")
        .cloned()
        .unwrap_or_else(|| "/usr/bin:/bin".to_string());
    let clean_path = strip_shim_dirs(&raw_path, None);
    let clean_path = if clean_path.is_empty() {
        "/usr/bin:/bin".to_string()
    } else {
        clean_path
    };
    let exe_dir = resolve_exe_dir(&clean_path);
    let role_dir = studio_dir.join("roles").join("director");
    if !role_dir.join("bin/git").is_file() && crate::runners::executable(&Path::new(&exe_dir).join("backlot")) {
        guard::prepare_role(&role_dir, &exe_dir)
            .map_err(|e| Fail::refused(format!("director guard setup: {e}")))?;
    }
    guard::apply_env(&mut map, &role_dir, &exe_dir, &clean_path, None);
    Ok(map.into_iter().collect())
}

fn director_worktree(studio_dir: &Path, studio: &StudioConfig) -> PathBuf {
    workareas::role_dir(studio_dir, &studio.repo, "director", studio.no_git)
}

/// Run proposed buildability checks once inside the director's worktree under the
/// stripped + director-guarded environment.
pub(crate) fn run_director_preflight(
    studio_dir: &Path,
    studio: &StudioConfig,
    env: &Env,
    checks_to_run: &[CheckDecl],
) -> Result<(), Fail> {
    if checks_to_run.is_empty() {
        return Ok(());
    }
    let work_dir = director_worktree(studio_dir, studio);
    if !work_dir.is_dir() {
        return Err(Fail::refused(format!(
            "director worktree does not exist: {}",
            work_dir.display()
        )));
    }
    let preflight_env = build_director_preflight_env(studio_dir, &env.vars, &studio.check_env)?;
    run_checks_list(checks_to_run, &work_dir, &preflight_env)
}

/// Run user-added `--preflight` check in the director worktree (when initialized)
/// or bound repo directory under `checks::env_for`.
pub(crate) fn run_user_preflight(
    studio_dir: &Path,
    studio: &StudioConfig,
    env: &Env,
    checks_to_run: &[CheckDecl],
) -> Result<(), Fail> {
    if checks_to_run.is_empty() {
        return Ok(());
    }
    let dir_work = director_worktree(studio_dir, studio);
    let cwd = if dir_work.is_dir() {
        dir_work
    } else {
        PathBuf::from(&studio.repo)
    };
    if !cwd.is_dir() {
        return Err(Fail::refused(format!(
            "preflight working directory does not exist: {}",
            cwd.display()
        )));
    }
    let mut stage_vars = env.vars.clone();
    stage_vars
        .entry("PATH".into())
        .or_insert_with(|| "/usr/bin:/bin".into());
    let clean_env = checks::env_for(&stage_vars, &studio.check_env);
    run_checks_list(checks_to_run, &cwd, &clean_env)
}

fn run_checks_list(
    checks_to_run: &[CheckDecl],
    dir: &Path,
    env: &[(String, String)],
) -> Result<(), Fail> {
    for c in checks_to_run {
        let secs = c.timeout_s.min(PREFLIGHT_TIMEOUT_S);
        let res = crate::checkrun::run_shell_bounded(&c.run, dir, env, Duration::from_secs(secs))
            .map_err(|e| Fail::refused(format!("preflight check '{}': {e}", c.name)))?;
        let tail = cap_tail(&res.output);
        if res.timed_out {
            return Err(format_preflight_fail(
                &c.name,
                &format!("timed out after {secs}s"),
                &tail,
            ));
        }
        if res.exit_code != Some(0) {
            let status = res
                .exit_code
                .map(|code| format!("exited {code}"))
                .unwrap_or_else(|| "killed by signal".to_string());
            return Err(format_preflight_fail(&c.name, &status, &tail));
        }
        if let Some(reason) = checks::check_expect(c.expect.as_deref(), c.min_matches, &res.output) {
            return Err(format_preflight_fail(&c.name, &reason, &tail));
        }
    }
    Ok(())
}

fn format_preflight_fail(name: &str, why: &str, tail: &str) -> Fail {
    if tail.is_empty() {
        Fail::refused(format!("preflight check '{name}' failed ({why})"))
    } else {
        Fail::refused(format!(
            "preflight check '{name}' failed ({why}):\n{tail}"
        ))
    }
}
