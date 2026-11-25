//! Outward-action guard (D3): pass-through execution of the real
//! binary past `<role>/bin`, refusal message, and ledger
//! (`role.refused` + `outward action` attention) recording.

use std::collections::BTreeMap;
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
        via: None,
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
        match Command::new(&real).args(rest).status() {
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
const SHIM_NAMES: &[&str] = &["gh", "hc", "scp", "rsync", "ssh", "npm", "cargo"];

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

fn write_exec(path: &Path, body: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&tmp, path)
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
    let backlot_bin = Path::new(exe_dir).join("backlot");
    let quoted_bin = shell_quote(&backlot_bin.to_string_lossy());

    let bin_dir = role_dir.join("bin");
    std::fs::create_dir_all(&bin_dir)?;
    for name in SHIM_NAMES {
        let body = format!("#!/bin/sh\n{SHIM_MARKER}\nexec {quoted_bin} guard {name} -- \"$@\"\n");
        write_exec(&bin_dir.join(name), &body)?;
    }

    let hooks_dir = role_dir.join("git-hooks");
    std::fs::create_dir_all(&hooks_dir)?;
    let pre_push =
        format!("#!/bin/sh\n{SHIM_MARKER}\nexec {quoted_bin} guard git-push -- \"$@\"\n");
    write_exec(&hooks_dir.join("pre-push"), &pre_push)?;

    for hook in GIT_PASSTHROUGH_HOOKS {
        write_exec(&hooks_dir.join(hook), &passthrough_hook_script(hook))?;
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
