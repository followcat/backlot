//! Studio lifecycle handlers for `plugin.backlot.req` (B1b): `studio
//! new|finish|prune|delete|restore` and the New Studio dialog's
//! `repos` / `runners` probes, each run by the same code the local CLI
//! uses.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use backlot_model::event::StudioMode;
use backlot_model::req_results::{StudioFinishResult, StudioNewResult, StudioPlan};
use backlot_model::req_wire::ReqAction;
use serde_json::Value;

use crate::args::{Cmd, TextSource};
use crate::cli::{Caller, Env, Fail, check_studio_name};
use crate::daemon::Stage;
use crate::{commands, confirm, delete, draft, prune, repos, restore, store};

/// Whether this action belongs to the studio lifecycle half of the
/// dispatcher.
pub(crate) fn handles(action: &ReqAction) -> bool {
    matches!(
        action,
        ReqAction::StudioNew { .. }
            | ReqAction::StudioFinish { .. }
            | ReqAction::StudioPrune { .. }
            | ReqAction::StudioDelete { .. }
            | ReqAction::StudioRestore { .. }
            | ReqAction::Repos { .. }
            | ReqAction::Runners
            | ReqAction::Version
    )
}

/// Run one studio-lifecycle request on the server's base.
pub(crate) async fn dispatch(
    stage: &Stage,
    env: &Env,
    action: ReqAction,
    token: Option<String>,
) -> Result<Value, Fail> {
    match action {
        ReqAction::StudioNew {
            id,
            title,
            repo,
            cast,
            mode,
            init,
            no_git,
            ask,
            dry_run,
        } => studio_new(
            &stage.base,
            env,
            id,
            title,
            repo,
            cast,
            mode,
            init,
            no_git,
            ask,
            dry_run,
        ),
        ReqAction::StudioFinish {
            studio,
            reason,
            force,
        } => studio_finish(&stage.base, env, studio, reason, force),
        ReqAction::StudioPrune {
            studio,
            all_finished,
            dry_run,
            force,
        } => prune_cmd(&stage.base, studio, all_finished, dry_run, force, token),
        ReqAction::StudioDelete {
            studio,
            dry_run,
            force,
        } => delete_cmd(stage, studio, dry_run, force, token).await,
        ReqAction::StudioRestore {
            archived,
            name,
            dry_run,
        } => restore_cmd(&stage.base, archived, name, dry_run, token),
        ReqAction::Repos { from, all } => {
            to_json(repos::scan(env, Some(&stage.base), from.as_deref(), all)?)
        }
        ReqAction::Runners => to_json(crate::runners::detect_report::rows(env)),
        ReqAction::Version => to_json(format!("backlot {}", env!("CARGO_PKG_VERSION"))),
        _ => unreachable!("handles() gates the studio actions"),
    }
}

#[allow(clippy::too_many_arguments)]
fn studio_new(
    base: &Path,
    env: &Env,
    id: String,
    title: String,
    repo: String,
    cast: String,
    mode: Option<StudioMode>,
    init: bool,
    no_git: bool,
    ask: Option<String>,
    dry_run: bool,
) -> Result<Value, Fail> {
    let id = check_studio_name(&id)?;
    crate::args_studio::check_repo_absolute(&repo)?;
    let repo = crate::args_studio::canonical_repo(&repo);
    let ask = match ask {
        Some(body) if body.trim().is_empty() => return Err(Fail::refused("the ask is empty")),
        other => other,
    };
    let cast_file = TempFile::new("cast", &cast)?;
    if dry_run {
        draft::check(base, &id, &repo, cast_file.path(), env, init, no_git)?;
        return to_json(StudioNewResult {
            studio: id,
            title,
            repo,
            dry_run: true,
            seq: None,
            ask_seq: None,
            first_message: ask.as_deref().map(crate::studio_fmt::first_message_line),
        });
    }
    let ask_file = match &ask {
        Some(body) => Some(TempFile::new("ask", body)?),
        None => None,
    };
    let caller = Caller::doc_user(&id);
    let cmd = Cmd::StudioNew {
        title: Some(title.clone()),
        repo: Some(repo.clone()),
        cast_file: Some(cast_file.path().to_path_buf()),
        dry_run: false,
        interactive: false,
        mode,
        init,
        no_git,
        ask: ask_file
            .as_ref()
            .map(|f| TextSource::File(f.path().to_path_buf())),
    };
    let built = commands::build(&caller, cmd, env, base)?;
    let dir = store::studio_dir(base, &id);
    let event = store::append(&dir, built)?;
    let ask_seq = match ask_file {
        Some(file) => {
            let queued = commands::ask_cmd(caller.by(), file.path().to_path_buf())?;
            Some(store::append(&dir, queued)?.seq)
        }
        None => None,
    };
    to_json(StudioNewResult {
        studio: id,
        title,
        repo,
        dry_run: false,
        seq: Some(event.seq),
        ask_seq,
        first_message: None,
    })
}

fn studio_finish(
    base: &Path,
    env: &Env,
    studio: String,
    reason: String,
    force: bool,
) -> Result<Value, Fail> {
    let id = check_studio_name(&studio)?;
    let caller = Caller::doc_user(&id);
    let built = commands::build(
        &caller,
        Cmd::StudioFinish {
            reason: reason.clone(),
            force,
        },
        env,
        base,
    )?;
    let event = store::append(&store::studio_dir(base, &id), built)?;
    to_json(StudioFinishResult {
        studio: id,
        seq: event.seq,
        reason,
    })
}

fn prune_cmd(
    base: &Path,
    studio: Option<String>,
    all_finished: bool,
    dry_run: bool,
    force: bool,
    token: Option<String>,
) -> Result<Value, Fail> {
    let studio = resolve_prune_studio(studio, all_finished)?;
    if dry_run {
        let report = prune::report(base, studio, all_finished, true, force)?;
        return to_json(StudioPlan {
            confirm_token: confirm::token(&report)?,
            report,
        });
    }
    let fresh = prune::report(base, studio.clone(), all_finished, true, force)?;
    confirm::verify(&fresh, token.as_deref())?;
    to_json(prune::report(base, studio, all_finished, false, force)?)
}

fn resolve_prune_studio(
    studio: Option<String>,
    all_finished: bool,
) -> Result<Option<String>, Fail> {
    match studio {
        Some(name) => Ok(Some(check_studio_name(&name)?)),
        None if all_finished => Ok(None),
        None => Err(Fail::usage("missing studio (or --all-finished)")),
    }
}

async fn delete_cmd(
    stage: &Stage,
    studio: Option<String>,
    dry_run: bool,
    force: bool,
    token: Option<String>,
) -> Result<Value, Fail> {
    let studio =
        studio.ok_or_else(|| Fail::usage("missing studio: set BACKLOT_STUDIO or pass --studio"))?;
    let id = check_studio_name(&studio)?;
    let by = Caller::doc_user(&id).by();
    let client = Some(&stage.client);
    if dry_run {
        let report = delete::execute(&stage.base, &id, by, client, true, force).await?;
        return to_json(StudioPlan {
            confirm_token: confirm::token(&report)?,
            report,
        });
    }
    let fresh = delete::execute(&stage.base, &id, by.clone(), client, true, force).await?;
    confirm::verify(&fresh, token.as_deref())?;
    to_json(delete::execute(&stage.base, &id, by, client, false, force).await?)
}

fn restore_cmd(
    base: &Path,
    archived: String,
    name: Option<String>,
    dry_run: bool,
    token: Option<String>,
) -> Result<Value, Fail> {
    if dry_run {
        let report = restore::execute(base, &archived, name.as_deref(), true)?;
        return to_json(StudioPlan {
            confirm_token: confirm::token(&report)?,
            report,
        });
    }
    let fresh = restore::execute(base, &archived, name.as_deref(), true)?;
    if fresh.needs_confirm() {
        confirm::verify(&fresh, token.as_deref())?;
    }
    to_json(restore::execute(base, &archived, name.as_deref(), false)?)
}

fn to_json<T: serde::Serialize>(val: T) -> Result<Value, Fail> {
    serde_json::to_value(val).map_err(|e| Fail::refused(format!("result serialize: {e}")))
}

/// A request's content staged on the hub so the unchanged local code
/// can read it as a file; removed when the handler returns.
pub(crate) struct TempFile(PathBuf);

impl TempFile {
    fn new(kind: &str, body: &str) -> Result<Self, Fail> {
        stage_content(kind, body)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

/// Write request content (a cast file, an ask, a brief) to a temp file
/// under the system temp dir, removed on drop — the hub never resolves a
/// client path, so content is what travels. Shared with the person half
/// of the dispatcher (`studio cast set`).
pub(crate) fn stage_content(kind: &str, body: &str) -> Result<TempFile, Fail> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "backlot-req-{kind}-{}-{}.txt",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, body)
        .map_err(|e| Fail::refused(format!("cannot stage the {kind}: {e}")))?;
    Ok(TempFile(path))
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
