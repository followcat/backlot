//! `backlot studio delete <studio> [--dry-run] [--force] [--json]`:
//! finish (if live), close every terminal of the studio, prune
//! worktrees and branches safely, record `studio.deleted`, and archive
//! the ledger under `<base>/archive/<id>-<UTC stamp>`.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use backlot_model::EventBy;
use backlot_model::event::{EventKind, NewEvent};
use backlot_model::validate::validate;
use serde::{Deserialize, Serialize};

use crate::args::Common;
use crate::cli::{Env, Fail};
use crate::delete_ops::{
    close_roles_in_ledger, close_terminals, list_studio_terminals, rename_noreplace, run_remote,
    utc_stamp_now,
};
use crate::delete_render::format_report;
use crate::link;
use crate::prune::{self, Plan, Step};
use crate::store;
use crate::workareas::real_git;

/// Summary of what `backlot studio delete` did or (with `--dry-run`) would do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteReport {
    pub studio: String,
    pub dry_run: bool,
    pub force: bool,
    pub finished: bool,
    pub closed_terminals: Vec<String>,
    pub removed_worktrees: Vec<String>,
    pub pruned_worktrees: Vec<String>,
    pub deleted_branches: Vec<String>,
    pub kept: Vec<String>,
    pub archive: String,
    pub archived: bool,
    /// A real run that kept items without `--force`: nothing was
    /// archived, and the caller turns this into the refusal.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub blocked: bool,
}

pub(crate) fn run(
    env: &Env,
    common: &Common,
    studio: Option<String>,
    dry_run: bool,
    force: bool,
    json: bool,
) -> Result<String, Fail> {
    let role = common
        .role
        .clone()
        .or_else(|| env.get("BACKLOT_ROLE").map(String::from))
        .unwrap_or_else(|| "user".to_string());
    if role != "user" {
        return Err(Fail::refused(format!(
            "role '{role}' cannot do this; the user can"
        )));
    }
    let id = resolve_studio_id(env, common, studio)?;
    if let Some(target) = common.remote.as_deref()
        && env.get("BACKLOT_STATE_DIR").is_none()
    {
        return link::runtime()?
            .block_on(run_remote(env, common, target, &id, dry_run, force, json));
    }
    let by = EventBy {
        role,
        session: env.get("HC_TERMINAL_ID").map(String::from),
        via: None,
    };
    link::runtime()?.block_on(run_local(env, common, &id, by, dry_run, force, json))
}

fn resolve_studio_id(env: &Env, common: &Common, studio: Option<String>) -> Result<String, Fail> {
    match studio {
        Some(name) => {
            let name = crate::cli::check_studio_name(&name)?;
            if let Some(pinned) = common.studio.as_deref()
                && pinned != name
            {
                return Err(Fail::usage(format!(
                    "studio '{name}' disagrees with --studio '{pinned}'"
                )));
            }
            if let Some(pinned) = env.get("BACKLOT_STUDIO")
                && pinned != name
            {
                return Err(Fail::refused(format!(
                    "studio '{name}' disagrees with BACKLOT_STUDIO '{pinned}'"
                )));
            }
            Ok(name)
        }
        None => crate::cli::studio_name(common, env),
    }
}

async fn run_local(
    env: &Env,
    common: &Common,
    id: &str,
    by: EventBy,
    dry_run: bool,
    force: bool,
    json: bool,
) -> Result<String, Fail> {
    let has_core_pin = common.session.is_some()
        || common.remote.is_some()
        || env.get("HC_SESSION").is_some()
        || env.get("HC_CORE_SOCKET").is_some()
        || env.get("HC_SOCKET").is_some_and(|p| Path::new(p).exists());
    let (base, connected) = if env.get("BACKLOT_STATE_DIR").is_none() || has_core_pin {
        let link = link::connect(
            env,
            common.session.as_deref().map(OsStr::new),
            common.remote.as_deref(),
        )
        .await?;
        let base = match env.get("BACKLOT_STATE_DIR") {
            Some(d) => PathBuf::from(d),
            None => crate::server_lock::effective_base(&env.vars, &link.core_id),
        };
        (base, Some(link))
    } else {
        (
            PathBuf::from(env.get("BACKLOT_STATE_DIR").expect("checked above")),
            None,
        )
    };
    let base = crate::cli::resolve_base(env, common, &base, id)?;
    let client = connected.as_ref().map(|l| &l.client);
    let report = execute(&base, id, by, client, dry_run, force).await?;
    if report.blocked {
        return Err(Fail::refused(format_report(&report, json)?));
    }
    format_report(&report, json)
}

/// The work itself, on an explicit base and (when the caller has one)
/// a connected core: shared by the local CLI and the request queue.
pub(crate) async fn execute(
    base: &Path,
    id: &str,
    by: EventBy,
    client: Option<&hc_proto::client::Client>,
    dry_run: bool,
    force: bool,
) -> Result<DeleteReport, Fail> {
    let dir = store::studio_dir(base, id);
    if !dir.join("events.jsonl").is_file() {
        return Err(Fail::refused(format!("no studio '{id}'")));
    }
    let mut state = store::load_state(&dir).map_err(crate::hwood::store_fail)?;
    if state.studio.is_none() {
        return Err(Fail::refused(format!("studio '{id}' has no events")));
    }

    let archive_name = format!("{id}-{}", utc_stamp_now());
    let rel_archive = format!("archive/{archive_name}");
    let dest = store::archive_dir(base).join(&archive_name);
    if dest.exists() {
        return Err(Fail::refused(format!(
            "archive target already exists: {}",
            dest.display()
        )));
    }

    // Step 1: Finish if live.
    let will_finish = state.finished.is_none();
    if will_finish {
        let finish_ev = NewEvent::new(
            by.clone(),
            EventKind::StudioFinished {
                reason: "deleted".into(),
            },
        );
        validate(&state, &finish_ev).map_err(|r| Fail::refused(r.to_string()))?;
        if !dry_run {
            store::append(&dir, |_, _| {
                Ok::<_, Fail>(store::Pending {
                    event: finish_ev,
                    files: Vec::new(),
                })
            })?;
            state = store::load_state(&dir).map_err(crate::hwood::store_fail)?;
        }
    }

    // Step 2: Close studio terminals and prevent stage resume.
    let matched = match client {
        Some(client) => list_studio_terminals(client, id).await?,
        None => Vec::new(),
    };
    let closed_terminals: Vec<String> = matched
        .iter()
        .map(|t| t.name.clone().unwrap_or_else(|| t.address.clone()))
        .collect();
    if !dry_run {
        close_roles_in_ledger(&dir, &state, &by)?;
        if let Some(client) = client {
            close_terminals(client, &matched).await?;
        }
    }

    // Step 3: Prune worktrees and branches safely.
    let cfg = state.studio.as_ref().expect("checked studio exists");
    let mut git = real_git;
    let (plan, mut steps) = prune::plan(&mut git, cfg);
    let repos = match plan {
        Plan::NotWorkTree => {
            return Err(Fail::refused(format!(
                "studio '{id}': {} is not a git work tree",
                cfg.repo
            )));
        }
        Plan::Repos(repos) => repos,
    };
    for repo in repos {
        let label = (!cfg.members.is_empty()).then(|| prune::member_leaf(&repo));
        match prune::prune_studio(&mut git, id, &dir, &repo, label.as_deref(), dry_run, force) {
            Ok(more) => steps.extend(more),
            Err(e) if cfg.members.is_empty() => return Err(e),
            Err(e) => steps.push(Step::KeptRepo(
                prune::member_leaf(&repo),
                e.message().to_string(),
            )),
        }
    }

    let mut report = DeleteReport {
        studio: id.to_string(),
        dry_run,
        force,
        finished: will_finish,
        closed_terminals,
        removed_worktrees: Vec::new(),
        pruned_worktrees: Vec::new(),
        deleted_branches: Vec::new(),
        kept: Vec::new(),
        // A dry run names the shape, not the second it happened to
        // run in — the review's token must survive the commit's own
        // fresh dry run.
        archive: if dry_run {
            format!("archive/{id}-<stamp>")
        } else {
            rel_archive.clone()
        },
        archived: false,
        blocked: false,
    };
    for step in steps {
        match step {
            Step::RemovedWorktree(p) => report.removed_worktrees.push(p),
            Step::PrunedWorktree(p) => report.pruned_worktrees.push(p),
            Step::DeletedBranch(b) => report.deleted_branches.push(b),
            Step::KeptWorktree(p, why) => report.kept.push(format!("kept worktree {p} — {why}")),
            Step::KeptBranch(b, why) => report.kept.push(format!("kept branch {b} — {why}")),
            Step::KeptRepo(r, why) => report.kept.push(format!("kept {r} — {why}")),
        }
    }

    if !dry_run && !force && !report.kept.is_empty() {
        report.blocked = true;
        return Ok(report);
    }

    // Step 4: Archive ledger.
    if !dry_run {
        fs::create_dir_all(store::archive_dir(base))
            .map_err(|e| Fail::refused(format!("cannot create archive dir: {e}")))?;
        if dest.exists() {
            return Err(Fail::refused(format!(
                "archive target already exists: {}",
                dest.display()
            )));
        }
        store::append(&dir, |_, _| {
            Ok::<_, Fail>(store::Pending {
                event: NewEvent::new(
                    by.clone(),
                    EventKind::StudioDeleted {
                        archive: rel_archive.clone(),
                    },
                ),
                files: Vec::new(),
            })
        })?;
        rename_noreplace(&dir, &dest)?;
        report.archived = true;
    }

    Ok(report)
}
