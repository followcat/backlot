//! `deliver --commits A..B` is checked in the task's work area before
//! the append (S9b): both ends must resolve there, the range must not
//! be empty, and `B` must sit on the task's `backlot/<studio>/<task>`
//! branch. The ledger records the resolved `sha..sha`, never the
//! symbolic input. Git runs here in the CLI — the model stays
//! git-free.

use std::path::{Path, PathBuf};

use backlot_model::event::Verdict;
use backlot_model::state::{StudioState, Task, TaskStatus};

use crate::cli::Fail;
use crate::workareas::{self, GitRun};

/// `work/<task>` under the studio dir — the git context for every
/// range check. A missing area means the actor's terminal never ran:
/// the stage makes it when the actor starts.
fn work_area(dir: &Path, task: &str) -> Result<PathBuf, Fail> {
    let work = dir.join("work").join(task);
    if work.is_dir() {
        Ok(work)
    } else {
        Err(Fail::refused(format!(
            "task '{task}' has no work area at {}; the stage makes it when the actor starts",
            work.display()
        )))
    }
}

/// `rev-parse` one endpoint to a full commit sha inside `work`.
fn rev_parse(git: &mut GitRun<'_>, cwd: &str, spec: &str) -> Result<String, Fail> {
    let out = git(
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{spec}^{{commit}}"),
        ],
        cwd,
    );
    if out.code == 0 {
        Ok(out.stdout.trim().to_string())
    } else {
        Err(Fail::refused(format!(
            "--commits: cannot resolve '{spec}' in the task's work area ({})",
            workareas::err_line(&out)
        )))
    }
}

/// `merge-base --is-ancestor a b`: exit 0 yes, 1 no; anything else
/// is git failing, not an answer.
fn is_ancestor(git: &mut GitRun<'_>, cwd: &str, a: &str, b: &str) -> Result<bool, Fail> {
    let out = git(&["merge-base", "--is-ancestor", a, b], cwd);
    match out.code {
        0 => Ok(true),
        1 => Ok(false),
        _ => Err(Fail::refused(format!(
            "--commits: 'git merge-base' failed ({})",
            workareas::err_line(&out)
        ))),
    }
}

/// The checks, on an injected git so tests never spawn it: `range`
/// in, the resolved `sha..sha` out.
pub fn resolve(
    git: &mut GitRun<'_>,
    work: &Path,
    branch: &str,
    range: &str,
) -> Result<String, Fail> {
    let (a, b) = range
        .split_once("..")
        .filter(|(a, b)| !a.is_empty() && !b.is_empty())
        .ok_or_else(|| Fail::usage(format!("bad --commits '{range}', want A..B")))?;
    let cwd = work.to_string_lossy().into_owned();
    let sa = rev_parse(git, &cwd, a)?;
    let sb = rev_parse(git, &cwd, b)?;
    // Empty: B is already reachable from A — nothing would be
    // delivered. Only then: B must sit on the task's branch.
    if is_ancestor(git, &cwd, &sb, &sa)? {
        return Err(Fail::refused(format!(
            "--commits '{range}' is an empty range: '{b}' is already reachable from '{a}'"
        )));
    }
    if !is_ancestor(git, &cwd, &sb, branch)? {
        return Err(Fail::refused(format!(
            "--commits '{range}': '{b}' is not on branch '{branch}'"
        )));
    }
    Ok(format!("{sa}..{sb}"))
}

/// The CLI's check: the task's work area, its `backlot/<studio>/<task>`
/// branch, real git. Returns the `sha..sha` the event records.
pub fn check_range(dir: &Path, studio: &str, task: &str, range: &str) -> Result<String, Fail> {
    let work = work_area(dir, task)?;
    let branch = format!("backlot/{studio}/{task}");
    resolve(&mut workareas::real_git, &work, &branch, range)
}

/// The sha `A..B` records as `B` — the delivery's tip.
fn range_end(commits: &str) -> &str {
    commits.rsplit("..").next().unwrap_or(commits)
}

/// The tip of the delivery a terminal task was accepted on: the
/// delivery its last accepting review named, else the latest — the
/// strictest read of what must already sit on integration.
fn accepted_end(t: &Task) -> Option<&str> {
    let d = t
        .reviews
        .iter()
        .rev()
        .find(|r| matches!(r.verdict, Verdict::Accepted | Verdict::PartiallyAccepted))
        .and_then(|r| t.deliveries.iter().find(|d| d.n == r.delivery))
        .or_else(|| t.deliveries.last())?;
    Some(range_end(&d.commits))
}

/// The `task.cast` git gate (S9e): every accepted dependency's
/// accepted delivery must already be an ancestor of
/// `backlot/<studio>/integration` — the task's work area is cut from
/// the integration tip, so an unmerged dependency would start the
/// actor without its code. Dependencies still open are the model's
/// refusal, not this one's. Runs in `work/integration`, falling back
/// to the studio repo — the same object store.
pub fn check_dependencies(
    dir: &Path,
    studio: &str,
    state: &StudioState,
    task: &str,
) -> Result<(), Fail> {
    let (Some(t), Some(repo)) = (state.tasks.get(task), state.studio.as_ref()) else {
        return Ok(()); // the model's own refusal names what is missing
    };
    let work = dir.join("work").join("integration");
    let area = if work.is_dir() {
        work
    } else {
        PathBuf::from(&repo.repo)
    };
    let cwd = area.to_string_lossy().into_owned();
    let branch = format!("backlot/{studio}/integration");
    let mut unmerged: Vec<&str> = Vec::new();
    for dep in &t.dependencies {
        let Some(d) = state.tasks.get(dep.as_str()) else {
            continue; // unknown counts as open — the model refuses
        };
        if !matches!(
            d.status,
            TaskStatus::Accepted | TaskStatus::PartiallyAccepted
        ) {
            continue; // still open — the model's refusal names it
        }
        let Some(end) = accepted_end(d) else {
            continue;
        };
        let tip = workareas::real_git(
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                "--end-of-options",
                &format!("{branch}^{{commit}}"),
            ],
            &cwd,
        );
        let merged = match tip.code {
            // No integration branch yet: nothing is merged into it.
            0 => {
                let out = workareas::real_git(&["merge-base", "--is-ancestor", end, &branch], &cwd);
                match out.code {
                    0 => true,
                    1 => false,
                    _ => {
                        return Err(Fail::refused(format!(
                            "cast: merge check for '{dep}' failed ({})",
                            workareas::err_line(&out)
                        )));
                    }
                }
            }
            _ => false,
        };
        if !merged {
            unmerged.push(dep);
        }
    }
    if unmerged.is_empty() {
        Ok(())
    } else {
        Err(Fail::refused(format!(
            "merge {} into integration first",
            unmerged.join(", ")
        )))
    }
}
