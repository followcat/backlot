//! `backlot server adopt [--dry-run] [--stop-stages] [--core <id>]`:
//! promote one legacy `<core_id>` directory into `server/`, symlink back so live
//! git worktrees keep resolving, archive empty/secondary core dirs under
//! `server/adopted-<stamp>/`, and leave `server-pointer.json` tombstones.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use hc_proto::endpoint::{self, Endpoint};
use hc_proto::terminal::ListParams;

use crate::args::Common;
use crate::cli::{Env, Fail};
use crate::server_adopt_fs::{self, AdoptPlan, RunningStage};
use crate::server_lock::{self, DEFAULT_SESSION};
use crate::{link, server_mode, store};

#[derive(Debug)]
struct SessionProbe {
    session: String,
    socket: PathBuf,
    core_id: String,
    legacy_stages: Vec<String>,
}

pub(crate) fn run(
    common: &Common,
    env: &Env,
    dry_run: bool,
    stop_stages: bool,
    core_arg: Option<&str>,
) -> Result<String, Fail> {
    if let Some(srv) = server_lock::running_server(&env.vars) {
        return Err(server_lock::running_server_refusal(&srv));
    }

    let root = store::root_dir(&env.vars);
    let server = root.join("server");
    let probes = discover_hc_sessions(common, env)?;
    let running_stages: Vec<RunningStage> = probes
        .iter()
        .flat_map(|p| {
            p.legacy_stages.iter().map(move |addr| RunningStage {
                socket: p.socket.clone(),
                session_label: p.session.clone(),
                address: addr.clone(),
            })
        })
        .collect();

    if !running_stages.is_empty() && !stop_stages {
        let list: Vec<String> = running_stages
            .iter()
            .map(|s| format!("{} ({})", s.address, s.session_label))
            .collect();
        return Err(Fail::refused(format!(
            "running backlot-stage found: {}; stop it first or pass --stop-stages",
            list.join(", ")
        )));
    }

    let candidates = discover_cores(&root)?;
    if candidates.is_empty() {
        if server.is_dir() {
            return Ok("nothing to adopt (already using server/)".to_string());
        }
        return Ok(format!(
            "nothing to adopt (no legacy core directories under {})",
            root.display()
        ));
    }

    let primary_id = pick_primary(&root, &candidates, core_arg, &probes)?;
    let moves = server_adopt_fs::plan_moves(&root.join(&primary_id), &server)?;
    let secondaries: Vec<String> = candidates
        .iter()
        .filter(|id| *id != &primary_id)
        .cloned()
        .collect();
    let plan = AdoptPlan {
        root,
        server,
        stamp: server_adopt_fs::utc_stamp(),
        primary_id,
        stages_to_stop: running_stages,
        moves,
        secondaries,
        pointers: candidates,
    };

    if dry_run {
        return Ok(server_adopt_fs::render_dry_run(&plan));
    }
    server_adopt_fs::execute(env, &plan)
}

fn discover_hc_sessions(common: &Common, env: &Env) -> Result<Vec<SessionProbe>, Fail> {
    let mut targets: Vec<(String, PathBuf)> = Vec::new();
    let mut seen_sockets: BTreeSet<PathBuf> = BTreeSet::new();

    if let Some(explicit) = env.get("HC_SOCKET").filter(|s| !s.is_empty()) {
        let sock = PathBuf::from(explicit);
        if sock.exists() && seen_sockets.insert(sock.clone()) {
            let sess = common
                .session
                .as_deref()
                .or_else(|| env.get("HC_SESSION"))
                .unwrap_or(DEFAULT_SESSION)
                .to_string();
            targets.push((sess, sock));
        }
    } else if let Some(dir) = herdr_runtime_dir(env)
        && let Ok(entries) = fs::read_dir(&dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("sock")
                && seen_sockets.insert(path.clone())
            {
                let sess = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("default")
                    .to_string();
                targets.push((sess, path));
            }
        }
    }

    targets.sort_by(|a, b| a.1.cmp(&b.1));
    if targets.is_empty() {
        return Ok(Vec::new());
    }
    let rt = link::runtime()?;
    let mut out = Vec::new();
    for (sess, sock) in targets {
        let mut probe_vars = env.vars.clone();
        probe_vars.insert("HC_SOCKET".into(), sock.to_string_lossy().into_owned());
        let probe_env = Env { vars: probe_vars };
        if let Ok(Some(probe)) = rt.block_on(probe_one(&probe_env, &sess, sock)) {
            out.push(probe);
        }
    }
    Ok(out)
}

async fn probe_one(
    env: &Env,
    session: &str,
    socket: PathBuf,
) -> Result<Option<SessionProbe>, Fail> {
    let Ok(lnk) = link::connect(env, Some(OsStr::new(session)), None).await else {
        return Ok(None);
    };
    let list = lnk
        .client
        .terminal_list(ListParams {
            all: true,
            ..ListParams::default()
        })
        .await
        .map_err(|e| Fail::refused(format!("terminal.list failed: {e}")))?;
    let legacy_stages = list
        .terminals
        .iter()
        .filter(|t| server_mode::is_legacy_stage_terminal(t))
        .map(|t| t.address.clone())
        .collect();
    Ok(Some(SessionProbe {
        session: session.to_string(),
        socket,
        core_id: lnk.core_id,
        legacy_stages,
    }))
}

fn herdr_runtime_dir(env: &Env) -> Option<PathBuf> {
    if let Some(run) = env.get("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(run).join("herdr-core"));
    }
    if env.get("HOME").is_some() && !env.vars.contains_key("XDG_RUNTIME_DIR") {
        let eenv = env_to_endpoint_env(env);
        if let Ok(Endpoint::Local(local)) =
            endpoint::resolve(&eenv, Some(OsStr::new("default")), None, &endpoint::Sys)
        {
            return local.socket.parent().map(Path::to_path_buf);
        }
    }
    None
}

fn env_to_endpoint_env(env: &Env) -> endpoint::Env {
    env.vars
        .iter()
        .map(|(k, v)| (std::ffi::OsString::from(k), std::ffi::OsString::from(v)))
        .collect()
}

fn discover_cores(root: &Path) -> Result<Vec<String>, Fail> {
    let Ok(entries) = fs::read_dir(root) else {
        return Ok(Vec::new());
    };
    let mut cores = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "server" || name.starts_with('.') || server_lock::is_pointer_dir(&path) {
            continue;
        }
        cores.push(name);
    }
    cores.sort();
    Ok(cores)
}

fn pick_primary(
    root: &Path,
    candidates: &[String],
    core_arg: Option<&str>,
    probes: &[SessionProbe],
) -> Result<String, Fail> {
    if let Some(raw) = core_arg {
        let san = store::sanitize(raw);
        if let Some(found) = candidates.iter().find(|c| c.as_str() == raw || **c == san) {
            return Ok(found.clone());
        }
        if server_lock::is_pointer_dir(&root.join(&san)) {
            return Err(Fail::refused(format!("core '{raw}' was already adopted")));
        }
        return Err(Fail::refused(format!(
            "no core directory '{san}' under {}",
            root.display()
        )));
    }
    if candidates.len() == 1 {
        return Ok(candidates[0].clone());
    }
    for want in [DEFAULT_SESSION, "default"] {
        if let Some(probe) = probes.iter().find(|p| p.session == want) {
            let san = store::sanitize(&probe.core_id);
            if let Some(found) = candidates
                .iter()
                .find(|c| **c == san || *c == &probe.core_id)
            {
                return Ok(found.clone());
            }
        }
    }
    for probe in probes {
        let san = store::sanitize(&probe.core_id);
        if let Some(found) = candidates
            .iter()
            .find(|c| **c == san || *c == &probe.core_id)
        {
            return Ok(found.clone());
        }
    }
    let non_empty: Vec<&String> = candidates
        .iter()
        .filter(|c| server_adopt_fs::has_meaningful_state(&root.join(c)))
        .collect();
    if non_empty.len() == 1 {
        return Ok(non_empty[0].clone());
    }
    Err(Fail::refused(format!(
        "multiple core directories found ({}); pass --core <id>",
        candidates.join(", ")
    )))
}
