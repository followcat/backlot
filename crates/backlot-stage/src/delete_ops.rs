//! Remote execution, terminal closing, and atomic archive rename helpers
//! for `backlot studio delete`.

use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use backlot_model::EventBy;
use backlot_model::event::{EventKind, NewEvent};
use hc_proto::client::{CallError, Client};
use hc_proto::endpoint::Endpoint;
use hc_proto::jsonrpc::error_kind;
use hc_proto::terminal::{ListParams, RunParams, Terminal, TerminalParams};

use crate::args::Common;
use crate::cli::{Env, Fail};
use crate::link;
use crate::store;

pub(crate) async fn run_remote(
    env: &Env,
    common: &Common,
    target: &str,
    id: &str,
    dry_run: bool,
    force: bool,
    json: bool,
) -> Result<String, Fail> {
    let link = link::connect(env, common.session.as_deref().map(OsStr::new), Some(target)).await?;
    let remote_session = match &link.endpoint {
        Endpoint::Remote(r) => r.session.clone(),
        Endpoint::Local(l) => l.session.clone(),
    };
    let mut args = vec![
        "studio".to_string(),
        "delete".into(),
        id.to_string(),
        "--session".into(),
        remote_session,
        "--as".into(),
        "user".into(),
    ];
    if dry_run {
        args.push("--dry-run".into());
    }
    if force {
        args.push("--force".into());
    }
    if json {
        args.push("--json".into());
    }
    let quoted_args = args
        .iter()
        .map(|a| sh_quote(a))
        .collect::<Vec<_>>()
        .join(" ");
    let line = format!(
        "b=$(command -v backlot 2>/dev/null); \
         [ -n \"$b\" ] || {{ s=${{SHELL:-/bin/sh}}; \
         b=$(if command -v timeout >/dev/null 2>&1; then timeout 3 \"$s\" -lc 'command -v backlot' </dev/null 2>/dev/null; \
         else \"$s\" -lc 'command -v backlot' </dev/null 2>/dev/null; fi | tail -n 1); }}; \
         [ -n \"$b\" ] || [ ! -x \"$HOME/.local/bin/backlot\" ] || b=\"$HOME/.local/bin/backlot\"; \
         [ -n \"$b\" ] || {{ printf 'backlot is not installed on %s\\n' {} >&2; exit 127; }}; \
         exec \"$b\" {quoted_args}",
        sh_quote(target),
    );
    let res = link
        .client
        .terminal_run(RunParams {
            line: Some(line),
            wait: true,
            rm: true,
            no_parent: true,
            no_notify: true,
            timeout: Some(30.0),
            ..Default::default()
        })
        .await
        .map_err(|e| Fail::refused(format!("remote studio delete failed: {e}")))?;
    let _ = link
        .client
        .terminal_rm(TerminalParams {
            terminal: res.terminal.address.clone(),
            caller: None,
        })
        .await;
    let output = res.output.unwrap_or_default();
    let ended = res.wait.as_ref().is_some_and(|w| w.ended);
    let code = res
        .wait
        .as_ref()
        .and_then(|w| w.exit_code)
        .or(res.terminal.exit_code);
    if !ended {
        return Err(Fail::refused("remote studio delete timed out"));
    }
    if code == Some(0) {
        return Ok(output.trim_end_matches('\n').to_string());
    }
    let msg = output.trim();
    Err(Fail::refused(if msg.is_empty() {
        format!("remote studio delete exited with code {:?}", code)
    } else {
        msg.to_string()
    }))
}

pub(crate) async fn list_studio_terminals(
    client: &Client,
    id: &str,
) -> Result<Vec<Terminal>, Fail> {
    let list = client
        .terminal_list(ListParams {
            all: true,
            ..ListParams::default()
        })
        .await
        .map_err(Fail::call)?;
    Ok(list
        .terminals
        .into_iter()
        .filter(|t| {
            !t.address.contains('/')
                && t.labels.get("backlot.studio").is_some_and(|s| s == id)
                && !matches!(
                    t.labels.get("backlot.role").map(String::as_str),
                    Some("stage" | "hollywood" | "hollywood-handoff" | "hollywood-previous")
                )
                && !matches!(t.name.as_deref(), Some("backlot-stage"))
                && !t
                    .name
                    .as_deref()
                    .is_some_and(backlot_model::hollywood::is_hollywood_terminal_name)
        })
        .collect())
}

pub(crate) fn close_roles_in_ledger(
    dir: &Path,
    state: &backlot_model::StudioState,
    by: &EventBy,
) -> Result<(), Fail> {
    let roles: Vec<String> = state
        .roles
        .iter()
        .filter(|(_, r)| r.held || (r.live && !r.closed))
        .map(|(name, _)| name.clone())
        .collect();
    for role in roles {
        let by = by.clone();
        store::append(dir, |_, _| {
            Ok::<_, Fail>(store::Pending {
                event: NewEvent::new(
                    by,
                    EventKind::RoleClosed {
                        role,
                        reason: Some("deleted".into()),
                    },
                ),
                files: Vec::new(),
            })
        })?;
    }
    Ok(())
}

fn is_not_found(e: &CallError) -> bool {
    matches!(e, CallError::Rpc(_)) && e.is_kind(error_kind::NOT_FOUND)
}

pub(crate) async fn close_terminals(
    driver: &dyn crate::driver::MachineDriver,
    client: &Client,
    terms: &[Terminal],
) -> Result<(), Fail> {
    let can_rm = driver.supports(client, hc_proto::method::TERMINAL_RM);
    for t in terms {
        let label = t.name.as_deref().unwrap_or(&t.address);
        if t.state == "live" {
            if let Err(e) = driver.kill(client, &t.address).await
                && !is_not_found(&e)
            {
                return Err(Fail::refused(format!(
                    "failed to kill terminal {label}: {e}"
                )));
            }
            let mut exited = false;
            for _ in 0..20 {
                match driver.get(client, &t.address).await {
                    Ok(cur) if cur.state == "finished" => {
                        exited = true;
                        break;
                    }
                    Ok(_) => tokio::time::sleep(Duration::from_millis(50)).await,
                    Err(e) if is_not_found(&e) => {
                        exited = true;
                        break;
                    }
                    Err(e) => {
                        return Err(Fail::refused(format!(
                            "failed checking terminal {label} after kill: {e}"
                        )));
                    }
                }
            }
            if !exited {
                return Err(Fail::refused(format!(
                    "terminal {label} did not exit after kill"
                )));
            }
        }
        if can_rm
            && let Err(e) = driver.rm(client, &t.address).await
            && !is_not_found(&e)
        {
            return Err(Fail::refused(format!(
                "failed to remove terminal {label}: {e}"
            )));
        }
    }
    Ok(())
}

pub(crate) fn rename_noreplace(src: &Path, dst: &Path) -> Result<(), Fail> {
    #[cfg(target_os = "linux")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let src_c = CString::new(src.as_os_str().as_bytes())
            .map_err(|_| Fail::refused("invalid source path for rename"))?;
        let dst_c = CString::new(dst.as_os_str().as_bytes())
            .map_err(|_| Fail::refused("invalid destination path for rename"))?;
        let rc = unsafe {
            libc::syscall(
                libc::SYS_renameat2,
                libc::AT_FDCWD,
                src_c.as_ptr(),
                libc::AT_FDCWD,
                dst_c.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if rc == 0 {
            return Ok(());
        }
        let err = std::io::Error::last_os_error();
        if !matches!(err.raw_os_error(), Some(libc::ENOSYS | libc::EINVAL)) {
            return Err(Fail::refused(format!(
                "cannot archive {} to {}: {err}",
                src.display(),
                dst.display()
            )));
        }
    }
    if dst.exists() {
        return Err(Fail::refused(format!(
            "archive target already exists: {}",
            dst.display()
        )));
    }
    fs::rename(src, dst).map_err(|e| {
        Fail::refused(format!(
            "cannot archive {} to {}: {e}",
            src.display(),
            dst.display()
        ))
    })
}

pub(crate) fn utc_stamp_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    #[cfg(unix)]
    {
        let timestamp: libc::time_t = secs.min(i64::MAX as u64) as libc::time_t;
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        if !unsafe { libc::gmtime_r(&timestamp, &mut tm) }.is_null() {
            return format!(
                "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
                tm.tm_year + 1900,
                tm.tm_mon + 1,
                tm.tm_mday,
                tm.tm_hour,
                tm.tm_min,
                tm.tm_sec
            );
        }
    }
    format!("{secs}Z")
}

fn sh_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}
