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
use crate::driver::MachineDriver;
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
/// branch, real git. Returns the `sha..sha` the event records — in a
/// workspace, `member:sha..sha` per comma-separated part (D10). A
/// no-git studio records the text verbatim.
/// The gate, through a [`MachineDriver`]'s git runner.
pub fn check_range_with(
    driver: &dyn MachineDriver,
    dir: &Path,
    studio: &str,
    task: &str,
    state: &StudioState,
    range: &str,
) -> Result<String, Fail> {
    let Some(cfg) = state.studio.as_ref() else {
        return Err(Fail::refused("the studio has no configuration"));
    };
    if cfg.no_git {
        return Ok(range.to_string());
    }
    let branch = format!("backlot/{studio}/{task}");
    if cfg.members.is_empty() {
        let work = work_area(dir, task)?;
        let mut git = driver.git();
        return resolve(&mut **git, &work, &branch, range);
    }
    let mut resolved = Vec::new();
    for part in range.split(',') {
        let part = part.trim();
        let (member, bare) = match part.split_once(':') {
            Some((m, r)) if !r.contains(':') => (Some(m.to_string()), r.to_string()),
            _ => (None, part.to_string()),
        };
        let members: Vec<String> = match &member {
            Some(m) => vec![m.clone()],
            None => cfg
                .members
                .iter()
                .filter(|m| tip_on_branch(&*driver, dir, task, m, &bare, &branch))
                .cloned()
                .collect(),
        };
        match members.as_slice() {
            [] => {
                return Err(Fail::refused(format!(
                    "--commits '{part}' resolves in no member repository — \
                     '<member>:{bare}' pins it explicitly"
                )));
            }
            [m] => {
                let work = work_area(dir, &format!("{task}/{}", leaf(m)))?;
                let sha = {
                    let mut git = driver.git();
                    resolve(&mut **git, &work, &branch, &bare)?
                };
                resolved.push(format!("{}:{sha}", leaf(m)));
            }
            many => {
                let names: Vec<String> = many.iter().map(|m| leaf(m)).collect();
                return Err(Fail::refused(format!(
                    "--commits '{part}' resolves in several member repositories \
                     ({}) — pin it as '<member>:{bare}'",
                    names.join(", ")
                )));
            }
        }
    }
    Ok(resolved.join(","))
}

/// A member repository's leaf name — the `work/<task>/<member>`
/// subdir and the `member:` prefix's spelling (D10).
fn leaf(member: &str) -> String {
    Path::new(member)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| member.to_string())
}

/// Does `bare`'s `B` resolve and sit on `backlot/<studio>/<task>`
/// inside `work/<task>/<member>` — the bare-range member probe.
fn tip_on_branch(
    driver: &dyn MachineDriver,
    dir: &Path,
    task: &str,
    member: &str,
    bare: &str,
    branch: &str,
) -> bool {
    let Some(b) = bare.rsplit("..").next().filter(|b| !b.is_empty()) else {
        return false;
    };
    let work = dir.join("work").join(task).join(leaf(member));
    if !work.is_dir() {
        return false;
    }
    let cwd = work.to_string_lossy().into_owned();
    let mut runner = driver.git();
    let mut git = |args: &[&str]| runner(args, &cwd);
    let tip = git(&[
        "rev-parse",
        "--verify",
        "--end-of-options",
        &format!("{b}^{{commit}}"),
    ]);
    if tip.code != 0 {
        return false;
    }
    git(&["merge-base", "--is-ancestor", tip.stdout.trim(), branch]).code == 0
}

/// One recorded range part: `member:a..b` or a bare `a..b` —
/// `(member leaf, end sha)`.
fn range_part(part: &str) -> (Option<&str>, &str) {
    let (member, rest) = match part.split_once(':') {
        Some((m, r)) if !r.contains(':') => (Some(m), r),
        _ => (None, part),
    };
    (member, rest.rsplit("..").next().unwrap_or(rest))
}

/// The `commits` string of the delivery a terminal task was
/// accepted on: the delivery its last accepting review named, else
/// the latest — the strictest read of what must already sit on
/// integration.
fn accepted_commits(t: &Task) -> Option<&str> {
    let d = t
        .reviews
        .iter()
        .rev()
        .find(|r| matches!(r.verdict, Verdict::Accepted | Verdict::PartiallyAccepted))
        .and_then(|r| t.deliveries.iter().find(|d| d.n == r.delivery))
        .or_else(|| t.deliveries.last())?;
    Some(&d.commits)
}

/// The `task.cast` git gate (S9e): every accepted dependency's
/// accepted delivery must already be an ancestor of
/// `backlot/<studio>/integration` — the task's work area is cut from
/// the integration tip, so an unmerged dependency would start the
/// actor without its code. Dependencies still open are the model's
/// refusal, not this one's. A member-qualified range part checks in
/// `work/integration/<member>` (or the member repository itself), a
/// bare part in `work/integration` (or the studio repo) — the same
/// object store. A no-git studio merges nothing and skips the gate.
/// The gate, through a [`MachineDriver`]'s git runner.
pub fn check_dependencies_with(
    driver: &dyn MachineDriver,
    dir: &Path,
    studio: &str,
    state: &StudioState,
    task: &str,
) -> Result<(), Fail> {
    let (Some(t), Some(cfg)) = (state.tasks.get(task), state.studio.as_ref()) else {
        return Ok(()); // the model's own refusal names what is missing
    };
    if cfg.no_git {
        return Ok(());
    }
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
        let Some(commits) = accepted_commits(d) else {
            continue;
        };
        for part in commits.split(',') {
            let (member, end) = range_part(part);
            let work = match member {
                Some(m) => dir.join("work").join("integration").join(m),
                None => dir.join("work").join("integration"),
            };
            let area = if work.is_dir() {
                work
            } else {
                match member {
                    Some(m) => cfg
                        .members
                        .iter()
                        .find(|p| leaf(p) == m)
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from(&cfg.repo)),
                    None => PathBuf::from(&cfg.repo),
                }
            };
            let cwd = area.to_string_lossy().into_owned();
            let mut git = driver.git();
            let tip = git(
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
                    let out = git(&["merge-base", "--is-ancestor", end, &branch], &cwd);
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
                break;
            }
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
