//! One check run's lifecycle (S9b part 2): the detached worktree
//! it lives in, the stripped environment it spawns under, the
//! tick-polled timeout that escalates TERM then KILL, and the
//! `task.checked` append that records it. Scheduling (`owed`,
//! `env_for`, the paths) is [`crate::checks`]; process mechanics
//! are [`crate::checkrun`].

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use backlot_model::event::EventKind;
use backlot_model::state::{StudioState, TaskStatus};

use crate::checkrun::{self, CheckChild, CheckSpawn};
use crate::checks::{self, Owed};
use crate::cli::Fail;
use crate::workareas::{GitOut, GitRun};

/// A failing git call's first stderr line, for the stage's
/// once-per-tick error.
fn git_fail(out: &GitOut) -> String {
    out.stderr
        .lines()
        .next()
        .unwrap_or("git failed")
        .to_string()
}

/// `work` as the canonical absolute path `/proc/<pid>/cwd` reports
/// — the parent canonicalized so a deleted tree still resolves to
/// its last live path.
fn canon_root(work: &Path) -> PathBuf {
    if let Ok(p) = work.canonicalize() {
        return p;
    }
    match work
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .zip(work.file_name())
    {
        Some((parent, name)) => parent.join(name),
        None => work.to_path_buf(),
    }
}

/// Our own processes whose cwd is inside `root` — a `server &` a
/// check left in its tree, or one still parked in a tree since
/// deleted (`/proc/<pid>/cwd` then reads `<path> (deleted)`).
/// Linux `/proc` only; anywhere else there is nothing to sweep.
#[cfg(target_os = "linux")]
pub(crate) fn cwd_pids(root: &Path) -> Vec<u32> {
    use std::os::unix::fs::MetadataExt;
    let root = canon_root(root);
    let me = unsafe { libc::geteuid() };
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return out;
    };
    for e in entries.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else {
            continue;
        };
        if e.metadata().map(|m| m.uid()).unwrap_or(u32::MAX) != me {
            continue;
        }
        let Ok(cwd) = fs::read_link(e.path().join("cwd")) else {
            continue;
        };
        let cwd = cwd.to_string_lossy();
        let cwd = Path::new(cwd.strip_suffix(" (deleted)").unwrap_or(&cwd));
        if cwd.starts_with(&root) {
            out.push(pid);
        }
    }
    out
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn cwd_pids(_root: &Path) -> Vec<u32> {
    Vec::new()
}

/// KILL each of our processes parked inside `root` — whatever a
/// check left behind dies before its tree is reused or removed.
fn kill_cwd(root: &Path) {
    for pid in cwd_pids(root) {
        unsafe {
            libc::kill(pid as i32, libc::SIGKILL);
        }
    }
}

/// Remove a check worktree: every process still parked inside dies
/// first, then `worktree remove`, a plain directory delete plus
/// `worktree prune` when git cannot — a half-created tree counts.
pub(crate) fn remove_tree(git: &mut GitRun<'_>, repo: &str, work: &Path) {
    kill_cwd(work);
    let work_s = work.to_string_lossy().into_owned();
    let out = git(
        &["-C", repo, "worktree", "remove", "--force", &work_s],
        repo,
    );
    if out.code != 0 {
        let _ = fs::remove_dir_all(work);
        git(&["-C", repo, "worktree", "prune"], repo);
    }
}

/// The delivery's end SHA from its recorded `sha..sha` range.
fn end_sha(commits: &str) -> Option<&str> {
    commits
        .rsplit_once("..")
        .map(|(_, b)| b.trim())
        .filter(|b| !b.is_empty())
}

/// A check process in flight across ticks. It outlives the task's
/// `delivered` status — once started it finishes and records — and
/// the timeout escalates TERM, then KILL five seconds later.
pub struct Running {
    pub task: String,
    pub delivery: u32,
    pub delivery_seq: u64,
    pub name: String,
    worktree: PathBuf,
    child: Box<dyn CheckChild>,
    started_ms: u64,
    timeout_ms: u64,
    term_ms: Option<u64>,
    /// KILL went out once — nothing escalates past it.
    killed: bool,
}

/// What a finished run records.
pub struct Outcome {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub duration_ms: u64,
    pub output: Vec<u8>,
}

impl Running {
    /// One nonblocking poll: `Some` when the process finished. Past
    /// the deadline the run is TERM'd once and KILL'd five seconds
    /// later if still alive; a natural exit wins the race and the
    /// run is not `timed_out`.
    pub fn poll(&mut self, now_ms: u64) -> Option<Outcome> {
        if let Some(p) = self.child.poll() {
            return Some(Outcome {
                exit_code: p.code,
                // Escalated before it finished: the timeout owns the
                // result however the process then died.
                timed_out: self.term_ms.is_some(),
                duration_ms: now_ms.saturating_sub(self.started_ms),
                output: p.output,
            });
        }
        if now_ms >= self.timeout_ms {
            match self.term_ms {
                None => {
                    self.child.term();
                    self.term_ms = Some(now_ms);
                }
                Some(t) if now_ms >= t + 5_000 && !self.killed => {
                    self.child.kill();
                    self.killed = true;
                }
                _ => {}
            }
        }
        None
    }

    /// A `Running` over a scripted child, for the runner tests.
    #[cfg(test)]
    pub(crate) fn for_test(
        child: Box<dyn CheckChild>,
        timeout_ms: u64,
        worktree: PathBuf,
    ) -> Running {
        Running {
            task: "t1".into(),
            delivery: 1,
            delivery_seq: 5,
            name: "unit".into(),
            worktree,
            child,
            started_ms: 0,
            timeout_ms,
            term_ms: None,
            killed: false,
        }
    }
}

impl Drop for Running {
    /// A run the stage never finished — the stage stopped mid-check
    /// — still dies, its whole group, not just the shell.
    fn drop(&mut self) {
        self.child.kill();
    }
}

/// The `checks/run-<seq>` names still owed work: the last delivery
/// of a task still `delivered`, with a declared check unrecorded.
fn needed_run_dirs(state: &StudioState) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Some(studio) = state.studio.as_ref() else {
        return out;
    };
    for t in state.tasks.values() {
        if t.status != TaskStatus::Delivered {
            continue;
        }
        let Some(d) = t.deliveries.last() else {
            continue;
        };
        if !t.checks_missing(d.n, &studio.checks).is_empty() {
            out.insert(format!("run-{}", d.seq));
        }
    }
    out
}

/// The stage's first pass sweeps `checks/run-*` trees a dead stage
/// left behind — anything parked inside is KILL'd first — except a
/// tree a still-owed delivery needs.
pub fn prune_run_trees(git: &mut GitRun<'_>, dir: &Path, state: &StudioState) {
    let needed = needed_run_dirs(state);
    let Some(repo) = state.studio.as_ref().map(|s| s.repo.as_str()) else {
        return;
    };
    let Ok(entries) = fs::read_dir(dir.join("checks")) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with("run-") && !needed.contains(name) {
            remove_tree(git, repo, &e.path());
        }
    }
}

/// Prepare the delivery's worktree and spawn the check. A leftover
/// tree from a dead stage comes down first — whatever it finished
/// already has a `task.checked`, so only fresh work is owed.
pub fn start(
    git: &mut GitRun<'_>,
    spawn: &checkrun::Spawn,
    state: &StudioState,
    dir: &Path,
    stage_env: &BTreeMap<String, String>,
    owed: &Owed,
    now_ms: u64,
) -> Result<Running, Fail> {
    let studio = state.studio.as_ref().ok_or(Fail::refused("no studio"))?;
    let task = &state.tasks[&owed.task];
    let d = task
        .deliveries
        .iter()
        .find(|d| d.seq == owed.delivery_seq)
        .ok_or_else(|| Fail::refused(format!("no delivery seq {}", owed.delivery_seq)))?;
    let end = end_sha(&d.commits).ok_or_else(|| {
        Fail::refused(format!(
            "task '{}' delivery {} records no commit range to check out",
            owed.task, owed.delivery
        ))
    })?;
    let work = checks::run_dir(dir, owed.delivery_seq);
    if work.exists() {
        remove_tree(git, &studio.repo, &work);
    }
    fs::create_dir_all(work.parent().expect("checks/ has a parent"))
        .map_err(|e| Fail::refused(format!("checks dir: {e}")))?;
    let work_s = work.to_string_lossy().into_owned();
    let add = git(
        &[
            "-C",
            &studio.repo,
            "worktree",
            "add",
            "--detach",
            &work_s,
            end,
        ],
        &studio.repo,
    );
    if add.code != 0 {
        return Err(Fail::refused(format!(
            "check worktree at {end}: {}",
            git_fail(&add)
        )));
    }
    let child = spawn(&CheckSpawn {
        run: owed.check.run.clone(),
        dir: work.clone(),
        env: checks::env_for(stage_env, &studio.check_env),
    })
    .map_err(Fail::refused)?;
    Ok(Running {
        task: owed.task.clone(),
        delivery: owed.delivery,
        delivery_seq: owed.delivery_seq,
        name: owed.check.name.clone(),
        worktree: work,
        child,
        started_ms: now_ms,
        timeout_ms: now_ms + owed.check.timeout_s.saturating_mul(1000),
        term_ms: None,
        killed: false,
    })
}

/// Whether the delivery's worktree still has checks to serve: the
/// task is still `delivered` on this delivery and some declared
/// check other than `run`'s has no record yet (`run`'s lands just
/// after this call).
fn tree_needed(state: &StudioState, run: &Running) -> bool {
    let Some(t) = state.tasks.get(&run.task) else {
        return false;
    };
    if t.status != TaskStatus::Delivered {
        return false;
    }
    let Some(d) = t.deliveries.last() else {
        return false;
    };
    if d.seq != run.delivery_seq {
        return false;
    }
    state.studio.as_ref().is_some_and(|s| {
        s.checks
            .iter()
            .any(|c| c.name != run.name && !t.checked_of(d.n).any(|r| r.name == c.name))
    })
}

/// A finished run: the capped log lands in `checks/` first, then
/// `task.checked` records it by `stage`, and the worktree comes
/// down when this delivery's checks are done or the task moved on.
/// `state` is the pre-append view; `tree_needed` discounts the
/// record this call writes.
pub fn finish(
    git: &mut GitRun<'_>,
    dir: &Path,
    state: &StudioState,
    run: Running,
    out: Outcome,
) -> Result<(), Fail> {
    let rel = checks::log_rel(run.delivery_seq, &run.name);
    let path = dir.join(&rel);
    fs::create_dir_all(path.parent().expect("checks/ has a parent"))
        .map_err(|e| Fail::refused(format!("checks dir: {e}")))?;
    fs::write(&path, &out.output).map_err(|e| Fail::refused(format!("check log: {e}")))?;
    crate::act::record(
        dir,
        EventKind::TaskChecked {
            task: run.task.clone(),
            delivery: run.delivery,
            name: run.name.clone(),
            exit_code: out.exit_code,
            timed_out: out.timed_out,
            output: rel,
            duration_ms: out.duration_ms,
        },
    )?;
    if !tree_needed(state, &run)
        && let Some(repo) = state.studio.as_ref().map(|s| s.repo.as_str())
    {
        remove_tree(git, repo, &run.worktree);
    }
    Ok(())
}
