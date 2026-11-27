//! Outward-action guard (D3 & push-perms §2.1): pass-through execution of
//! the real binary past `<role>/bin`, 3-way classification (`Allow`/`Card`/`Refuse`),
//! refusal message, and ledger (`role.refused` + `outward action` attention) recording.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use backlot_model::event::{EventKind, OUTWARD_ACTION, PushPolicy, RefusalDisposition};
use backlot_model::{EventBy, NewEvent};

use crate::cli::{Env, check_studio_name};
use crate::guard_git::{GitContext, is_fast_path};
pub(crate) use crate::guard_prep::{
    apply_env, find_real_binary, hollywood_role_dir, is_guard_shim_file, prepare_role,
    prepare_role_with, studio_role_dir,
};
use crate::guard_rules::{GuardVerdict, cap_command, classify_with_context};
use crate::store::{self, Pending, StoreError};

/// Comment placed on every generated shim so `find_real_binary` can
/// skip shims even when PATH or symlinks mask `shim_dir`.
pub(crate) const SHIM_MARKER: &str = "# backlot-guard-shim";

/// Per-role guard root (`<studio>/roles/<role>` or
/// `<state>/hollywood/roles/hollywood`) when env carries role identity
/// or `PATH` contains `<role_dir>/bin` with generated shims.
pub(crate) fn role_dir_from_env(env: &Env) -> Option<PathBuf> {
    if let (Some(state_dir), Some(role)) = (env.get("BACKLOT_STATE_DIR"), env.get("BACKLOT_ROLE")) {
        if role == backlot_model::hollywood::HOLLYWOOD {
            return Some(
                PathBuf::from(state_dir)
                    .join("hollywood")
                    .join("roles")
                    .join(role),
            );
        }
        if let Some(studio_raw) = env.get("BACKLOT_STUDIO")
            && let Ok(studio) = check_studio_name(studio_raw)
        {
            return Some(
                store::studio_dir(Path::new(state_dir), &studio)
                    .join("roles")
                    .join(role),
            );
        }
    }
    let path_env = env.get("PATH")?;
    for entry in path_env.split(':').filter(|s| !s.is_empty()) {
        let dir = Path::new(entry);
        if dir.file_name().and_then(|n| n.to_str()) == Some("bin")
            && is_guard_shim_file(&dir.join("git"))
            && let Some(role_dir) = dir.parent()
            && role_dir.join("git-hooks/pre-push").is_file()
        {
            return Some(role_dir.to_path_buf());
        }
    }
    None
}

/// Derive `(studio_dir, role_key)` from env or inferred `role_dir`.
pub(crate) fn studio_role_from_env(env: &Env) -> Option<(PathBuf, String)> {
    if let (Some(state_dir), Some(studio_raw), Some(role)) = (
        env.get("BACKLOT_STATE_DIR"),
        env.get("BACKLOT_STUDIO"),
        env.get("BACKLOT_ROLE"),
    ) {
        if role == backlot_model::hollywood::HOLLYWOOD || role == "actor" {
            return None;
        }
        let studio = check_studio_name(studio_raw).ok()?;
        let sdir = store::studio_dir(Path::new(state_dir), &studio);
        return Some((sdir, role.to_string()));
    }
    let rd = role_dir_from_env(env)?;
    let role = rd.file_name()?.to_str()?.to_string();
    let roles_dir = rd.parent()?;
    if roles_dir.file_name().and_then(|n| n.to_str()) != Some("roles") {
        return None;
    }
    let sdir = roles_dir.parent()?.to_path_buf();
    sdir.join("events.jsonl").is_file().then_some((sdir, role))
}

fn fast_work_area(sdir: &Path, role: &str) -> Option<PathBuf> {
    if role == "director" {
        return Some(sdir.join("work").join("integration"));
    }
    backlot_model::state::actor_task(role).map(|t| sdir.join("work").join(t))
}

pub(crate) struct StudioGuardInfo {
    pub policy: PushPolicy,
    pub role_dir: Option<PathBuf>,
    pub work_area: Option<PathBuf>,
    pub repo_root: Option<PathBuf>,
    pub members: Vec<String>,
}

/// Load studio `PushPolicy` and role work area from the ledger.
pub(crate) fn load_studio_guard_info(env: &Env) -> StudioGuardInfo {
    let role_dir = role_dir_from_env(env);
    let Some((sdir, role)) = studio_role_from_env(env) else {
        return StudioGuardInfo {
            policy: PushPolicy::default(),
            role_dir,
            work_area: None,
            repo_root: None,
            members: Vec::new(),
        };
    };
    let Ok(state) = store::load_state(&sdir) else {
        return StudioGuardInfo {
            policy: PushPolicy::default(),
            role_dir,
            work_area: fast_work_area(&sdir, &role),
            repo_root: None,
            members: Vec::new(),
        };
    };
    let Some(cfg) = state.studio else {
        return StudioGuardInfo {
            policy: PushPolicy::default(),
            role_dir,
            work_area: fast_work_area(&sdir, &role),
            repo_root: None,
            members: Vec::new(),
        };
    };
    let work_area = crate::workareas::role_dir(&sdir, &cfg.repo, &role, cfg.no_git);
    StudioGuardInfo {
        policy: cfg.push,
        role_dir,
        work_area: Some(work_area),
        repo_root: Some(PathBuf::from(cfg.repo)),
        members: cfg.members,
    }
}

#[derive(Debug)]
enum Skip {
    Duplicate,
}

/// Append `role.refused` and (for `Refused` only, once per open episode) raise an
/// informational `outward action` attention (`needs_user: false`) in the studio
/// ledger: the role was told to `backlot ask` if it still needs the command, so
/// nothing waits on the person.
pub(crate) fn record_verdict(
    env: &Env,
    cwd: &Path,
    command: &str,
    argv: &[String],
    rule: Option<&str>,
    disposition: RefusalDisposition,
) -> Result<(), String> {
    let Some((sdir, role_s)) = studio_role_from_env(env) else {
        return Ok(());
    };
    if !sdir.join("events.jsonl").is_file() {
        return Ok(());
    }

    let task = env.get("BACKLOT_TASK").map(String::from);
    let session = env.get("HC_TERMINAL_ID").map(String::from);
    let cmd_s = cap_command(command);
    let cwd_s = cwd.to_string_lossy().into_owned();

    let by = EventBy {
        role: role_s.clone(),
        session,
        via: None,
    };
    let refused_kind = EventKind::RoleRefused {
        role: role_s.clone(),
        task,
        command: cmd_s.clone(),
        cwd: cwd_s,
        argv: argv.to_vec(),
        rule: rule.map(String::from),
        disposition,
    };
    store::append(&sdir, move |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(by, refused_kind),
            files: Vec::new(),
        })
    })
    .map_err(|e| format!("role.refused: {e:?}"))?;

    if disposition == RefusalDisposition::Card {
        return Ok(());
    }

    let detail = cmd_s;
    let res = store::append(&sdir, move |state, _| {
        if state
            .open_attention()
            .any(|a| a.role == role_s && a.reason == OUTWARD_ACTION && a.detail == detail)
        {
            return Err(Skip::Duplicate);
        }
        Ok(Pending {
            event: NewEvent::new(
                EventBy::role("stage"),
                EventKind::AttentionRaised {
                    role: role_s,
                    reason: OUTWARD_ACTION.into(),
                    detail,
                    needs_user: false,
                    account: None,
                    reset_at_ms: None,
                },
            ),
            files: Vec::new(),
        })
    });
    match res {
        Ok(_) | Err(StoreError::Build(Skip::Duplicate)) => Ok(()),
        Err(e) => Err(format!("attention.raised: {e:?}")),
    }
}

#[cfg(test)]
pub(crate) fn run_in(
    args: &[String],
    env: &Env,
    cwd: &Path,
    exec_process: bool,
) -> (ExitCode, Option<String>) {
    run_in_with_stdin(args, env, cwd, exec_process, "")
}

pub(crate) fn run_in_with_stdin(
    args: &[String],
    env: &Env,
    cwd: &Path,
    exec_process: bool,
    stdin_data: &str,
) -> (ExitCode, Option<String>) {
    let Some(name) = args.first().map(String::as_str) else {
        return (
            ExitCode::from(2),
            Some("usage: backlot guard <command> -- [<args>...]".into()),
        );
    };
    let rest = if args.get(1).map(String::as_str) == Some("--") {
        &args[2..]
    } else {
        &args[1..]
    };

    let role_dir = role_dir_from_env(env);
    let mut fast_ok = false;
    if name == "git" {
        let default_policy = PushPolicy::default();
        let fast_work = studio_role_from_env(env).and_then(|(s, r)| fast_work_area(&s, &r));
        let fast_ctx = GitContext {
            policy: &default_policy,
            cwd,
            role_dir: role_dir.as_deref(),
            work_area: fast_work.as_deref(),
            repo_root: None,
            members: &[],
            env,
        };
        if is_fast_path(rest, &fast_ctx) {
            fast_ok = true;
        }
    }

    if !fast_ok {
        let info = load_studio_guard_info(env);
        let ctx = GitContext {
            policy: &info.policy,
            cwd,
            role_dir: info.role_dir.as_deref(),
            work_area: info.work_area.as_deref(),
            repo_root: info.repo_root.as_deref(),
            members: &info.members,
            env,
        };
        let m = classify_with_context(name, rest, stdin_data, &ctx);
        match m.verdict {
            GuardVerdict::Allow => {
                if name == "git-push" {
                    return (ExitCode::SUCCESS, None);
                }
            }
            GuardVerdict::Card { command, rule } => {
                let _ = record_verdict(
                    env,
                    &m.cwd,
                    &command,
                    &m.argv,
                    Some(&rule),
                    RefusalDisposition::Card,
                );
                let msg = format!(
                    "refused by backlot ({rule}): sent to the person as a card; wait for the result"
                );
                return (ExitCode::from(126), Some(msg));
            }
            GuardVerdict::Refuse { command, rule } => {
                let _ = record_verdict(
                    env,
                    &m.cwd,
                    &command,
                    &m.argv,
                    Some(&rule),
                    RefusalDisposition::Refused,
                );
                let msg = format!(
                    "refused by backlot: {command} is the person's to run; ask with backlot ask ({rule})"
                );
                return (ExitCode::from(126), Some(msg));
            }
        }
    }

    let shim_dir = role_dir.map(|r| r.join("bin"));
    let Some(real) = find_real_binary(name, env.get("PATH"), shim_dir.as_deref()) else {
        return (
            ExitCode::from(127),
            Some(format!("{name}: command not found")),
        );
    };

    if exec_process {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            for attempt in 0..5 {
                let err = Command::new(&real).args(rest).exec();
                if err.raw_os_error() == Some(libc::ETXTBSY) && attempt < 4 {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                }
                return (ExitCode::from(126), Some(format!("{name}: {err}")));
            }
        }
    }

    for attempt in 0..5 {
        let mut cmd = Command::new(&real);
        cmd.args(rest).current_dir(cwd);
        if !env.vars.is_empty() {
            cmd.env_clear().envs(&env.vars);
        }
        match cmd.status() {
            Ok(st) => {
                let code = st.code().and_then(|c| u8::try_from(c).ok()).unwrap_or(1);
                return (ExitCode::from(code), None);
            }
            Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) && attempt < 4 => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(e) => return (ExitCode::from(126), Some(format!("{name}: {e}"))),
        }
    }
    unreachable!()
}

/// Entry point for `backlot guard <name> -- <args...>`.
pub fn run(args: &[String]) -> ExitCode {
    let env = Env::process();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut stdin_data = String::new();
    if args.first().map(String::as_str) == Some("git-push") {
        let _ = std::io::stdin()
            .take(1 << 20)
            .read_to_string(&mut stdin_data);
    }
    let (code, msg) = run_in_with_stdin(args, &env, &cwd, true, &stdin_data);
    if let Some(line) = msg {
        eprintln!("{line}");
    }
    code
}
