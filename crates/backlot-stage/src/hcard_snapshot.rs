//! Paths a card references are resolved when it is proposed, and the
//! files it names are copied into the card's own directory under
//! Hollywood's ledger (`cards/c<n>/`). The proposer's path and its
//! hash go on the card, so an edited source makes the card `stale` —
//! the apply executes the bytes the person reviewed, never whatever
//! the file says later (field notes §1.1–1.2).

use std::path::{Path, PathBuf};

use backlot_model::hollywood::{CardAction, CardSnapshot, SnapshotFile, sha256_hex};

use crate::cli::Fail;
use crate::store::PendingFile;

/// The files a card's proposal resolved, absolute — what the dry run
/// and the snapshot read.
#[derive(Debug)]
pub(crate) struct ResolvedFiles {
    pub cast: PathBuf,
    pub brief: Option<PathBuf>,
}

/// The path the card's own copy of `leaf` gets, relative to Hollywood's
/// directory. `resolve_draft` reads it back unchanged at apply.
fn card_rel(seq: u64, leaf: &str) -> String {
    format!("cards/c{seq}/{leaf}")
}

/// Resolve every file argument of `action` against the proposer's
/// working directory, refusing one that is not there now — a card that
/// names a missing file fails at propose, never at apply.
pub(crate) fn resolve(action: &CardAction) -> Result<Option<ResolvedFiles>, Fail> {
    let cwd = std::env::current_dir()
        .map_err(|e| Fail::refused(format!("cannot read the working directory: {e}")))?;
    resolve_in(&cwd, action)
}

/// [`resolve`] against an explicit working directory — the seam its
/// test drives, so a relative path from another directory is what the
/// proposal resolves.
pub(crate) fn resolve_in(cwd: &Path, action: &CardAction) -> Result<Option<ResolvedFiles>, Fail> {
    match action {
        CardAction::StudioNew { cast, brief, .. } => {
            let cast = resolve_file(cwd, cast, "--cast")?;
            let brief = resolve_file(cwd, brief, "--brief")?;
            Ok(Some(ResolvedFiles {
                cast,
                brief: Some(brief),
            }))
        }
        CardAction::Cast { cast, .. } => {
            let cast = resolve_file(cwd, cast, "--file")?;
            Ok(Some(ResolvedFiles { cast, brief: None }))
        }
        _ => Ok(None),
    }
}

/// One file argument: absolute stays as spelled, relative joins the
/// working directory, and `..` is normalised away. A file that does not
/// exist is refused with the flag that named it.
fn resolve_file(cwd: &Path, raw: &str, flag: &str) -> Result<PathBuf, Fail> {
    let abs = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        crate::guard_git_env::normalize_resolve(cwd, Path::new(raw))
    };
    if !abs.is_file() {
        return Err(Fail::usage(format!(
            "{flag}: file not found: {}",
            abs.display()
        )));
    }
    Ok(abs)
}

/// The same action with its file arguments absolute — what the dry run
/// and `card_base` read before the copies are made.
pub(crate) fn with_absolute(action: &CardAction, files: Option<&ResolvedFiles>) -> CardAction {
    let Some(files) = files else {
        return action.clone();
    };
    match action {
        CardAction::StudioNew {
            studio,
            title,
            repo,
            no_git,
            ..
        } => CardAction::StudioNew {
            studio: studio.clone(),
            title: title.clone(),
            repo: repo.clone(),
            cast: files.cast.to_string_lossy().into_owned(),
            brief: files
                .brief
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            no_git: *no_git,
            snapshot: None,
        },
        CardAction::Cast { studio, .. } => CardAction::Cast {
            studio: studio.clone(),
            cast: files.cast.to_string_lossy().into_owned(),
            snapshot: None,
        },
        other => other.clone(),
    }
}

/// The action as the card stores it — its copied files under
/// `cards/c<seq>/`, plus the source paths and hashes to check later.
/// The copies ride the same append as the event, so a proposal never
/// references a file it did not write.
pub(crate) fn to_card_local(
    action: &CardAction,
    files: Option<&ResolvedFiles>,
    seq: u64,
) -> Result<(CardAction, Vec<PendingFile>), Fail> {
    let Some(files) = files else {
        return Ok((action.clone(), Vec::new()));
    };
    match action {
        CardAction::StudioNew {
            studio,
            title,
            repo,
            no_git,
            ..
        } => {
            let brief = files
                .brief
                .as_ref()
                .expect("a studio-new draft resolves its brief");
            let cast_rel = card_rel(seq, "cast.toml");
            let brief_rel = card_rel(seq, "brief.md");
            let snapshot = CardSnapshot {
                cast: Some(snapshot_of(&files.cast)?),
                brief: Some(snapshot_of(brief)?),
            };
            let action = CardAction::StudioNew {
                studio: studio.clone(),
                title: title.clone(),
                repo: repo.clone(),
                cast: cast_rel.clone(),
                brief: brief_rel.clone(),
                no_git: *no_git,
                snapshot: Some(snapshot),
            };
            let copies = vec![
                PendingFile {
                    src: files.cast.clone(),
                    dest: cast_rel,
                    append: None,
                },
                PendingFile {
                    src: brief.clone(),
                    dest: brief_rel,
                    append: None,
                },
            ];
            Ok((action, copies))
        }
        CardAction::Cast { studio, .. } => {
            let cast_rel = card_rel(seq, "cast.toml");
            let action = CardAction::Cast {
                studio: studio.clone(),
                cast: cast_rel.clone(),
                snapshot: Some(CardSnapshot {
                    cast: Some(snapshot_of(&files.cast)?),
                    brief: None,
                }),
            };
            let copies = vec![PendingFile {
                src: files.cast.clone(),
                dest: cast_rel,
                append: None,
            }];
            Ok((action, copies))
        }
        other => Ok((other.clone(), Vec::new())),
    }
}

/// The source path and the hash of its bytes right now.
fn snapshot_of(path: &Path) -> Result<SnapshotFile, Fail> {
    let bytes = std::fs::read(path)
        .map_err(|e| Fail::refused(format!("cannot read {}: {e}", path.display())))?;
    Ok(SnapshotFile::new(
        path.to_string_lossy().into_owned(),
        sha256_hex(&bytes),
    ))
}

/// `Some(reason)` when a source file a card copied has changed since
/// the proposal — "like the other base checks". Reads the proposer's
/// path, never the card's copy.
pub(crate) fn stale(action: &CardAction) -> Option<String> {
    let snapshot = match action {
        CardAction::StudioNew {
            snapshot: Some(s), ..
        }
        | CardAction::Cast {
            snapshot: Some(s), ..
        } => s,
        _ => return None,
    };
    snapshot.stale(|src| std::fs::read(src).ok())
}

/// The `producer=… director=… writer=…` line `card list` and `card
/// show` print for a card that carries a cast snapshot, read from the
/// card's own copy. `None` for a card with no cast file or one that no
/// longer parses.
pub(crate) fn roles_summary(hdir: &Path, action: &CardAction) -> Option<String> {
    let cast = match action {
        CardAction::StudioNew { cast, .. } | CardAction::Cast { cast, .. } => cast,
        _ => return None,
    };
    let path = crate::hcard_new::resolve_draft(hdir, cast);
    let file = crate::studio_cast::read(&path).ok()?;
    let text = backlot_model::recast::identity_text;
    Some(format!(
        "producer={} director={} writer={}",
        text(&file.roles.producer),
        text(&file.roles.director),
        text(&file.roles.writer),
    ))
}
