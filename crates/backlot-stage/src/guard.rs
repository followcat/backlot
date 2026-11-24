//! Outward-action guard (D3): pass-through execution of the real
//! binary past `<role>/bin`, refusal message, and ledger
//! (`role.refused` + `outward action` attention) recording.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use backlot_model::event::{EventKind, OUTWARD_ACTION};
use backlot_model::{EventBy, NewEvent};

use crate::cli::{Env, check_studio_name};
use crate::guard_rules::cap_command;
pub(crate) use crate::guard_rules::refused_command;
use crate::runners::executable;
use crate::store::{self, Pending, StoreError};

/// Comment placed on every generated shim so `find_real_binary` can
/// skip shims even when PATH or symlinks mask `shim_dir`.
pub(crate) const SHIM_MARKER: &str = "# backlot-guard-shim";

fn is_guard_shim_file(path: &Path) -> bool {
    let Ok(mut f) = File::open(path) else {
        return false;
    };
    let mut buf = [0u8; 128];
    let Ok(n) = f.read(&mut buf) else {
        return false;
    };
    std::str::from_utf8(&buf[..n]).is_ok_and(|head| head.contains(SHIM_MARKER))
}

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

/// Per-role guard root (`<studio>/roles/<role>` or
/// `<state>/hollywood/roles/hollywood`) when env carries role identity.
pub(crate) fn role_dir_from_env(env: &Env) -> Option<PathBuf> {
    let state_dir = env.get("BACKLOT_STATE_DIR")?;
    let role = env.get("BACKLOT_ROLE")?;
    if role == backlot_model::hollywood::HOLLYWOOD {
        return Some(
            PathBuf::from(state_dir)
                .join("hollywood")
                .join("roles")
                .join(role),
        );
    }
    let studio = check_studio_name(env.get("BACKLOT_STUDIO")?).ok()?;
    Some(
        store::studio_dir(Path::new(state_dir), &studio)
            .join("roles")
            .join(role),
    )
}

#[derive(Debug)]
enum Skip {
    Duplicate,
}

/// Append `role.refused` and (once per open episode) raise an
/// `outward action` attention (`needs_user: true`) in the studio ledger.
pub(crate) fn record_refusal(env: &Env, cwd: &Path, command: &str) -> Result<(), String> {
    let Some(state_dir) = env.get("BACKLOT_STATE_DIR") else {
        return Ok(());
    };
    let Some(studio_raw) = env.get("BACKLOT_STUDIO") else {
        return Ok(());
    };
    let Ok(studio) = check_studio_name(studio_raw) else {
        return Ok(());
    };
    let Some(role) = env.get("BACKLOT_ROLE") else {
        return Ok(());
    };
    if role == backlot_model::hollywood::HOLLYWOOD || role == "actor" {
        return Ok(());
    }
    let sdir = store::studio_dir(Path::new(state_dir), &studio);
    if !sdir.join("events.jsonl").is_file() {
        return Ok(());
    }

    let role_s = role.to_string();
    let task = env.get("BACKLOT_TASK").map(String::from);
    let session = env.get("HC_TERMINAL_ID").map(String::from);
    let cmd_s = cap_command(command);
    let cwd_s = cwd.to_string_lossy().into_owned();

    let by = EventBy {
        role: role_s.clone(),
        session,
    };
    let refused_kind = EventKind::RoleRefused {
        role: role_s.clone(),
        task,
        command: cmd_s.clone(),
        cwd: cwd_s,
    };
    store::append(&sdir, move |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(by, refused_kind),
            files: Vec::new(),
        })
    })
    .map_err(|e| format!("role.refused: {e:?}"))?;

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
                    needs_user: true,
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

pub(crate) fn run_in(
    args: &[String],
    env: &Env,
    cwd: &Path,
    exec_process: bool,
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

    if let Some(cmd) = refused_command(name, rest) {
        let _ = record_refusal(env, cwd, &cmd);
        let msg = format!("refused by backlot: {cmd} is the person's to run; ask with backlot ask");
        return (ExitCode::from(126), Some(msg));
    }

    let shim_dir = role_dir_from_env(env).map(|r| r.join("bin"));
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
            let err = Command::new(&real).args(rest).exec();
            return (ExitCode::from(126), Some(format!("{name}: {err}")));
        }
    }

    match Command::new(&real).args(rest).status() {
        Ok(st) => {
            let code = st.code().and_then(|c| u8::try_from(c).ok()).unwrap_or(1);
            (ExitCode::from(code), None)
        }
        Err(e) => (ExitCode::from(126), Some(format!("{name}: {e}"))),
    }
}

/// Entry point for `backlot guard <name> -- <args...>`.
pub fn run(args: &[String]) -> ExitCode {
    let env = Env::process();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (code, msg) = run_in(args, &env, &cwd, true);
    if let Some(line) = msg {
        eprintln!("{line}");
    }
    code
}
