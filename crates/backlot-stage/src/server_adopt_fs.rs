//! Filesystem planning, execution, symlinks, and dry-run rendering for `server adopt`.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::cli::{Env, Fail};
use crate::server_lock::{self, DEFAULT_SESSION, POINTER_FILE, ServerPointer};
use crate::{link, server_mode};

pub(crate) const PROMOTE_ITEMS: &[&str] = &[
    "studios",
    "hollywood",
    "archive",
    "config.toml",
    "agent.env",
    "processed-reqs.jsonl",
];

#[derive(Debug, Clone)]
pub(crate) struct RunningStage {
    pub(crate) socket: PathBuf,
    pub(crate) session_label: String,
    pub(crate) address: String,
}

#[derive(Debug, Clone)]
pub(crate) struct MoveOp {
    pub(crate) from: PathBuf,
    pub(crate) to: PathBuf,
    pub(crate) symlink_from: PathBuf,
    pub(crate) symlink_target: String,
}

#[derive(Debug)]
pub(crate) struct AdoptPlan {
    pub(crate) root: PathBuf,
    pub(crate) server: PathBuf,
    pub(crate) stamp: String,
    pub(crate) primary_id: String,
    pub(crate) stages_to_stop: Vec<RunningStage>,
    pub(crate) moves: Vec<MoveOp>,
    pub(crate) secondaries: Vec<String>,
    pub(crate) pointers: Vec<String>,
}

pub(crate) fn has_meaningful_state(dir: &Path) -> bool {
    PROMOTE_ITEMS.iter().any(|item| dir.join(item).exists())
}

pub(crate) fn plan_moves(primary: &Path, server: &Path) -> Result<Vec<MoveOp>, Fail> {
    let mut ops = Vec::new();
    for item in PROMOTE_ITEMS {
        let from = primary.join(item);
        if !from.exists() {
            continue;
        }
        if fs::symlink_metadata(&from)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            continue;
        }
        let to = server.join(item);
        if to.exists() {
            check_no_conflict(&from, &to, item)?;
        }
        ops.push(MoveOp {
            from: from.clone(),
            to,
            symlink_from: from,
            symlink_target: format!("../server/{item}"),
        });
    }
    Ok(ops)
}

pub(crate) fn execute(env: &Env, plan: &AdoptPlan) -> Result<String, Fail> {
    if !plan.stages_to_stop.is_empty() {
        let rt = link::runtime()?;
        for st in &plan.stages_to_stop {
            let mut vars = env.vars.clone();
            vars.insert("HC_SOCKET".into(), st.socket.to_string_lossy().into_owned());
            let senv = Env { vars };
            rt.block_on(async {
                let lnk = link::connect(&senv, Some(OsStr::new(&st.session_label)), None).await?;
                server_mode::stop_terminal(&lnk.client, &st.address).await
            })?;
        }
    }

    fs::create_dir_all(&plan.server)
        .map_err(|e| Fail::refused(format!("create {}: {e}", plan.server.display())))?;

    for op in &plan.moves {
        move_entry(&op.from, &op.to)?;
        symlink_relative(&op.symlink_target, &op.symlink_from)?;
    }

    if !plan.secondaries.is_empty() {
        let adopted_root = plan.server.join(format!("adopted-{}", plan.stamp));
        fs::create_dir_all(&adopted_root)
            .map_err(|e| Fail::refused(format!("create {}: {e}", adopted_root.display())))?;
        for sec in &plan.secondaries {
            let src = plan.root.join(sec);
            let dst = adopted_root.join(sec);
            fs::rename(&src, &dst).map_err(|e| {
                Fail::refused(format!(
                    "archive {} -> {}: {e}",
                    src.display(),
                    dst.display()
                ))
            })?;
        }
    }

    let now = server_lock::now_ms();
    let server_str = plan.server.to_string_lossy().into_owned();
    for id in &plan.pointers {
        let ptr = ServerPointer {
            version: 1,
            core_id: id.clone(),
            session: DEFAULT_SESSION.to_string(),
            server_dir: server_str.clone(),
            adopted_ms: now,
        };
        server_lock::write_pointer(&plan.root.join(id), &ptr)?;
    }

    Ok(render_summary(plan))
}

pub(crate) fn render_dry_run(plan: &AdoptPlan) -> String {
    let mut lines = vec![format!(
        "[dry-run] promote core '{}' into {}",
        plan.primary_id,
        plan.server.display()
    )];
    for st in &plan.stages_to_stop {
        lines.push(format!(
            "[dry-run] would stop legacy stage {} ({})",
            st.address, st.session_label
        ));
    }
    for op in &plan.moves {
        lines.push(format!(
            "[dry-run] would move {} -> {} and symlink -> {}",
            op.from.display(),
            op.to.display(),
            op.symlink_target
        ));
    }
    for sec in &plan.secondaries {
        lines.push(format!(
            "[dry-run] would archive {} -> {}/adopted-{}/{sec}",
            plan.root.join(sec).display(),
            plan.server.display(),
            plan.stamp
        ));
    }
    for id in &plan.pointers {
        lines.push(format!(
            "[dry-run] would write {}",
            plan.root.join(id).join(POINTER_FILE).display()
        ));
    }
    lines.join("\n")
}

pub(crate) fn utc_stamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3_600, (rem % 3_600) / 60, rem % 60);
    // Howard Hinnant's civil-from-days algorithm (UTC).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let yr = y + i64::from(mo <= 2);
    format!("{yr:04}{mo:02}{d:02}-{h:02}{m:02}{s:02}Z")
}

fn check_no_conflict(from: &Path, to: &Path, item: &str) -> Result<(), Fail> {
    if from.is_dir() && to.is_dir() {
        let mut existing = BTreeSet::new();
        for e in fs::read_dir(to)
            .map_err(|e| Fail::refused(format!("read {}: {e}", to.display())))?
            .flatten()
        {
            existing.insert(e.file_name());
        }
        for e in fs::read_dir(from)
            .map_err(|e| Fail::refused(format!("read {}: {e}", from.display())))?
            .flatten()
        {
            if existing.contains(&e.file_name()) {
                return Err(Fail::refused(format!(
                    "adopt conflict: {item}/{} exists in both {} and {}",
                    e.file_name().to_string_lossy(),
                    from.display(),
                    to.display()
                )));
            }
        }
        return Ok(());
    }
    let a = fs::read(from).map_err(|e| Fail::refused(format!("read {}: {e}", from.display())))?;
    let b = fs::read(to).map_err(|e| Fail::refused(format!("read {}: {e}", to.display())))?;
    if a != b {
        return Err(Fail::refused(format!(
            "adopt conflict: {} differs from {}",
            from.display(),
            to.display()
        )));
    }
    Ok(())
}

fn move_entry(from: &Path, to: &Path) -> Result<(), Fail> {
    if !to.exists() {
        return fs::rename(from, to).map_err(|e| {
            Fail::refused(format!("move {} -> {}: {e}", from.display(), to.display()))
        });
    }
    if from.is_dir() && to.is_dir() {
        for e in fs::read_dir(from)
            .map_err(|e| Fail::refused(format!("read {}: {e}", from.display())))?
            .flatten()
        {
            let target = to.join(e.file_name());
            fs::rename(e.path(), &target).map_err(|err| {
                Fail::refused(format!(
                    "move {} -> {}: {err}",
                    e.path().display(),
                    target.display()
                ))
            })?;
        }
        fs::remove_dir(from)
            .map_err(|e| Fail::refused(format!("remove merged dir {}: {e}", from.display())))?;
        return Ok(());
    }
    fs::remove_file(from).map_err(|e| Fail::refused(format!("remove {}: {e}", from.display())))
}

#[cfg(unix)]
fn symlink_relative(target: &str, link_path: &Path) -> Result<(), Fail> {
    std::os::unix::fs::symlink(target, link_path)
        .map_err(|e| Fail::refused(format!("symlink {} -> {target}: {e}", link_path.display())))
}

#[cfg(not(unix))]
fn symlink_relative(_target: &str, _link_path: &Path) -> Result<(), Fail> {
    Err(Fail::refused("server adopt symlinks require unix"))
}

fn render_summary(plan: &AdoptPlan) -> String {
    let mut out = format!(
        "adopted '{}' -> {} ({} moved, {} archived)",
        plan.primary_id,
        plan.server.display(),
        plan.moves.len(),
        plan.secondaries.len()
    );
    if !plan.stages_to_stop.is_empty() {
        out.push_str(&format!(", stopped {} stage(s)", plan.stages_to_stop.len()));
    }
    out
}
