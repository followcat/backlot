//! `backlot studio restore <archived-name> [--name <new-name>] [--dry-run]`
//! (L7, M3): bring an archived studio back under `studios/`, finished,
//! so its ledger, `reports/`, `messages/` and history read again and
//! `reopen` works from there. Work areas are not recreated — the
//! studio comes back as the finished thing it was archived as. A name
//! already taken under `studios/` refuses; `--name` renames the copy.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::args::Common;
use crate::cli::{Env, Fail};
use crate::store;

/// What a restore did or (with `--dry-run`) would do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RestoreReport {
    pub archived: String,
    pub name: String,
    pub dest: String,
    pub dry_run: bool,
    /// The copy lands under a new name (`--name`).
    pub renamed: bool,
}

impl RestoreReport {
    /// A restore needs the person's confirmation only when it renames
    /// the copy; replacing an existing studio is refused outright.
    pub(crate) fn needs_confirm(&self) -> bool {
        self.renamed
    }
}

/// `backlot studio restore ...` — the user's verb, then the copy.
pub(crate) fn run(
    env: &Env,
    common: &Common,
    archived: &str,
    new_name: Option<&str>,
    dry_run: bool,
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
    let base = crate::cli::base(env, common)?;
    let report = execute(&base, archived, new_name, dry_run)?;
    format_report(&report, json)
}

/// The copy itself on an explicit base — shared by the CLI and the
/// request queue.
pub(crate) fn execute(
    base: &Path,
    archived: &str,
    new_name: Option<&str>,
    dry_run: bool,
) -> Result<RestoreReport, Fail> {
    let src = find_archived(base, archived)?;
    // The studio id the restored ledger records under: the archive's
    // own `<id>` prefix, or the `--name` name.
    let (default_id, _) = split_archive(archived);
    let name = match new_name {
        Some(n) => crate::cli::check_studio_name(n)?,
        None => crate::cli::check_studio_name(&default_id)?,
    };
    let dest = store::studio_dir(base, &name);
    if dest.join("events.jsonl").is_file() || dest.exists() {
        return Err(Fail::refused(format!(
            "a studio named '{name}' already exists — pick another name with --name"
        )));
    }
    let report = RestoreReport {
        archived: archived.to_string(),
        name,
        dest: dest.to_string_lossy().into_owned(),
        dry_run,
        renamed: new_name.is_some(),
    };
    if dry_run {
        return Ok(report);
    }
    copy_tree(&src, &dest)?;
    Ok(report)
}

/// The human line or the `--json` document.
pub(crate) fn format_report(r: &RestoreReport, json: bool) -> Result<String, Fail> {
    if json {
        return serde_json::to_string_pretty(r)
            .map_err(|e| Fail::refused(format!("studio restore: {e}")));
    }
    if r.dry_run {
        return Ok(format!(
            "dry run: would restore {} as finished studio '{}' at {} — nothing copied",
            r.archived, r.name, r.dest
        ));
    }
    Ok(format!(
        "restored {} as finished studio '{}' — reopen a person with 'backlot reopen --person <key>'",
        r.archived, r.name
    ))
}

/// The archive entry named `archived`, or one carrying that studio id
/// when a bare id is given and exactly one entry matches.
fn find_archived(base: &Path, archived: &str) -> Result<std::path::PathBuf, Fail> {
    let entries = store::archived_studios(base)
        .map_err(|e| Fail::refused(format!("cannot read the archive: {e}")))?;
    if let Some((_, dir)) = entries.iter().find(|(name, _)| name == archived) {
        return Ok(dir.clone());
    }
    let matches: Vec<&(String, std::path::PathBuf)> = entries
        .iter()
        .filter(|(name, _)| split_archive(name).0 == archived)
        .collect();
    match matches.as_slice() {
        [one] => Ok(one.1.clone()),
        [] => Err(Fail::refused(format!("no archived studio '{archived}'"))),
        _ => Err(Fail::refused(format!(
            "several archived studios carry the id '{archived}' — name one by its full archive entry"
        ))),
    }
}

/// The `(id, stamp)` an archive entry's name carries.
fn split_archive(name: &str) -> (String, String) {
    match name.rsplit_once('-') {
        Some((id, stamp)) => (id.to_string(), stamp.to_string()),
        None => (name.to_string(), String::new()),
    }
}

/// Copy a directory tree — `archive/<id>-<stamp>` back into
/// `studios/<name>` — with no symlink following. The ledger is the
/// source of truth; `work/` is not required for a finished studio and
/// is copied only if it is still there.
fn copy_tree(src: &Path, dst: &Path) -> Result<(), Fail> {
    std::fs::create_dir_all(dst)
        .map_err(|e| Fail::refused(format!("cannot create {}: {e}", dst.display())))?;
    let entries = std::fs::read_dir(src)
        .map_err(|e| Fail::refused(format!("cannot read the archive: {e}")))?;
    for entry in entries.flatten() {
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|e| Fail::refused(format!("cannot read {}: {e}", from.display())))?;
        if file_type.is_dir() {
            copy_tree(&from, &to)?;
        } else if file_type.is_file() {
            std::fs::copy(&from, &to)
                .map_err(|e| Fail::refused(format!("cannot copy {}: {e}", from.display())))?;
        }
    }
    Ok(())
}
