//! Machine-local, repo-scoped operator notes
//! (`<state_dir>/repo-notes/<sha256(canonical_repo)[..16]>.md`):
//! `0600` storage, stage bridging from studio `purpose = "repo_note"`
//! messages, `card apply` execution, user-only `show`/`remove`, and
//! studio prompt path lookup.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use backlot_model::EventBy;
use backlot_model::event::{EventKind, NewEvent};
use backlot_model::hollywood::{CardAction, CardBase, HollywoodKind, NewHollywoodEvent};
use backlot_model::repo_notes::{
    check_text_safe, entry_sha256, has_card_marker, parse_sections, remove_section,
    repo_hash_prefix, upsert_section, valid_entry_id,
};
use serde::{Deserialize, Serialize};

use crate::args::Common;
use crate::args_studio::canonical_repo;
use crate::cli::{
    Caller, Env, Fail, base as cli_base, load as load_studio, resolve_base, studio_name,
};
use crate::daemon::Stage;
use crate::store::{self, HPending, Pending, PendingFile};
use crate::verbs::require_studio_dir;

static NEXT_TMP: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Serialize, Deserialize)]
struct RepoNoteRequest {
    repo: String,
    entry_id: String,
    text: String,
    why: String,
}

struct TempFileGuard(PathBuf);

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Directory holding repo-scoped notes under `<state_dir>/repo-notes`.
pub(crate) fn notes_dir(base: &Path) -> PathBuf {
    base.join("repo-notes")
}

/// Notes file path `<state_dir>/repo-notes/<sha256(canonical_repo)[..16]>.md`.
pub(crate) fn notes_path(base: &Path, repo: &str) -> PathBuf {
    let canon = canonical_repo(repo);
    notes_dir(base).join(format!("{}.md", repo_hash_prefix(&canon)))
}

/// If `dir` is `<state_dir>/studios/<studio>`, return `<state_dir>`.
pub(crate) fn state_dir_from_studio_dir(studio_dir: &Path) -> Option<PathBuf> {
    let parent = studio_dir.parent()?;
    (parent.file_name()?.to_str() == Some("studios"))
        .then(|| parent.parent())?
        .map(Path::to_path_buf)
}

/// Existing notes file for `repo`, if present on disk.
pub(crate) fn existing_notes_path(base: &Path, repo: &str) -> Option<PathBuf> {
    let p = notes_path(base, repo);
    p.is_file().then_some(p)
}

/// Existing notes file looked up from `<state_dir>/studios/<studio>`.
pub(crate) fn existing_notes_path_for_studio_dir(studio_dir: &Path, repo: &str) -> Option<PathBuf> {
    let base = state_dir_from_studio_dir(studio_dir)?;
    existing_notes_path(&base, repo)
}

/// Canonical SHA-256 of `(canonical_repo, entry_id)` section on disk, if any.
pub(crate) fn entry_digest(base: &Path, repo: &str, entry_id: &str) -> Option<String> {
    let path = notes_path(base, repo);
    let raw = fs::read_to_string(&path).ok()?;
    let sections = parse_sections(&raw);
    entry_sha256(sections.get(entry_id))
}

fn ensure_dir_0700(dir: &Path) -> Result<(), Fail> {
    fs::create_dir_all(dir).map_err(|e| Fail::refused(format!("{}: {e}", dir.display())))?;
    #[cfg(unix)]
    {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
            .map_err(|e| Fail::refused(format!("{}: {e}", dir.display())))?;
    }
    Ok(())
}

fn write_atomic_0600(path: &Path, bytes: &[u8]) -> Result<(), Fail> {
    let parent = path
        .parent()
        .ok_or_else(|| Fail::refused("invalid repo-notes path"))?;
    ensure_dir_0700(parent)?;
    let tmp = parent.join(format!(
        ".tmp-note-{}-{}",
        std::process::id(),
        NEXT_TMP.fetch_add(1, Ordering::Relaxed)
    ));
    let res = (|| -> std::io::Result<()> {
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        opts.mode(0o600);
        let mut file = opts.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        #[cfg(unix)]
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res.map_err(|e| Fail::refused(format!("cannot write {}: {e}", path.display())))
}

/// Apply one `CardAction::RepoNote` atomically with `0600` permissions.
/// Returns `Ok(false)` when `<!-- card c<card_seq> -->` is already recorded.
pub(crate) fn apply_card(
    base: &Path,
    repo: &str,
    entry_id: &str,
    card_seq: u64,
    text: &str,
) -> Result<bool, Fail> {
    let canon = canonical_repo(repo);
    let path = notes_path(base, &canon);
    let existing = match fs::read_to_string(&path) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return Err(Fail::refused(format!(
                "cannot read {}: {e}",
                path.display()
            )));
        }
    };
    if existing
        .as_deref()
        .is_some_and(|doc| has_card_marker(doc, entry_id, card_seq))
    {
        return Ok(false);
    }
    let next = upsert_section(existing.as_deref(), &canon, entry_id, card_seq, text);
    write_atomic_0600(&path, next.as_bytes())?;
    Ok(true)
}

/// Studio-side proposal: `backlot repo-note propose --id <slug> --text <text> --why <reason>`.
pub(crate) fn propose_in_studio(
    base: &Path,
    caller: &Caller,
    entry_id: &str,
    text: &str,
    why: &str,
) -> Result<String, Fail> {
    if !matches!(caller.role.as_str(), "director" | "producer" | "user") {
        return Err(Fail::refused(format!(
            "role '{}' cannot propose a repo note; the user, the producer or the director can",
            caller.role
        )));
    }
    if !valid_entry_id(entry_id) {
        return Err(Fail::refused(format!(
            "invalid --id '{entry_id}' (expected [a-z0-9_-]{{1,40}})"
        )));
    }
    check_text_safe(text).map_err(Fail::refused)?;
    if why.trim().is_empty() {
        return Err(Fail::refused("--why must not be empty"));
    }

    let dir = store::studio_dir(base, &caller.studio);
    require_studio_dir(&dir, &caller.studio)?;
    let state = load_studio(&dir)?;
    let Some(studio_cfg) = &state.studio else {
        return Err(Fail::refused(format!("no studio '{}'", caller.studio)));
    };
    let repo = canonical_repo(&studio_cfg.repo);
    let payload = RepoNoteRequest {
        repo,
        entry_id: entry_id.to_string(),
        text: text.to_string(),
        why: why.trim().to_string(),
    };
    let encoded = serde_json::to_string_pretty(&payload)
        .map_err(|e| Fail::refused(format!("encode repo-note payload: {e}")))?;

    let tmp = dir.join(format!(
        ".tmp-rnote-{}-{}",
        std::process::id(),
        NEXT_TMP.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&tmp, format!("{encoded}\n").as_bytes())
        .map_err(|e| Fail::refused(format!("cannot stage repo-note request: {e}")))?;
    let _guard = TempFileGuard(tmp.clone());

    let by = caller.by();
    let event = store::append(&dir, move |_, seq| {
        let dest = format!("messages/{seq}-repo-note.md");
        Ok::<Pending, Fail>(Pending {
            event: NewEvent::new(
                by,
                EventKind::MessageQueued {
                    to: "producer".into(),
                    file: Some(dest.clone()),
                    purpose: Some("repo_note".into()),
                    source_seq: None,
                },
            ),
            files: vec![PendingFile {
                src: tmp,
                dest,
                append: None,
            }],
        })
    })?;
    Ok(crate::render::confirm(&event))
}

fn resolve_repo_for_admin(
    common: &Common,
    env: &Env,
    base: &Path,
    repo_arg: Option<&str>,
) -> Result<String, Fail> {
    if let Some(raw) = repo_arg {
        crate::args_studio::check_repo_absolute(raw)?;
        return Ok(canonical_repo(raw));
    }
    let studio = studio_name(common, env)?;
    let resolved = resolve_base(env, common, base, &studio)?;
    let sdir = store::studio_dir(&resolved, &studio);
    require_studio_dir(&sdir, &studio)?;
    let state = load_studio(&sdir)?;
    let cfg = state
        .studio
        .as_ref()
        .ok_or_else(|| Fail::refused(format!("no studio '{studio}'")))?;
    Ok(canonical_repo(&cfg.repo))
}

/// User-only `backlot repo-note show|remove`.
pub(crate) fn admin(
    common: &Common,
    cmd: crate::args::RepoNoteCmd,
    env: &Env,
) -> Result<String, Fail> {
    let base = cli_base(env, common)?;
    match cmd {
        crate::args::RepoNoteCmd::Show { repo } => {
            let canon = resolve_repo_for_admin(common, env, &base, repo.as_deref())?;
            let path = notes_path(&base, &canon);
            match fs::read_to_string(&path) {
                Ok(text) => Ok(text.trim_end().to_string()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    Ok(format!("no repo notes for {canon}"))
                }
                Err(e) => Err(Fail::refused(format!(
                    "cannot read {}: {e}",
                    path.display()
                ))),
            }
        }
        crate::args::RepoNoteCmd::Remove { id, repo } => {
            if !valid_entry_id(&id) {
                return Err(Fail::refused(format!(
                    "invalid --id '{id}' (expected [a-z0-9_-]{{1,40}})"
                )));
            }
            let canon = resolve_repo_for_admin(common, env, &base, repo.as_deref())?;
            let path = notes_path(&base, &canon);
            let existing = fs::read_to_string(&path)
                .map_err(|_| Fail::refused(format!("no repo notes for '{canon}'")))?;
            match remove_section(&existing, &canon, &id).map_err(Fail::refused)? {
                Some(updated) => write_atomic_0600(&path, updated.as_bytes())?,
                None => {
                    fs::remove_file(&path).map_err(|e| {
                        Fail::refused(format!("cannot remove {}: {e}", path.display()))
                    })?;
                }
            }
            Ok(format!("removed repo-note '{id}' for {canon}"))
        }
        crate::args::RepoNoteCmd::Propose { .. } => {
            unreachable!("repo-note propose goes through exec_event")
        }
    }
}

impl Stage {
    /// Bridge uncarded studio `MessageQueued { purpose: Some("repo_note"), .. }`
    /// requests into `CardAction::RepoNote` cards (`by: stage`).
    pub(crate) fn propose_repo_notes(&self, hdir: &Path) -> Result<(), Fail> {
        let names = store::studios(&self.base).map_err(|e| Fail::refused(e.to_string()))?;
        for studio_name in names {
            let sdir = store::studio_dir(&self.base, &studio_name);
            let Ok(state) = store::load_state(&sdir) else {
                continue;
            };
            for m in &state.messages {
                if m.purpose.as_deref() != Some("repo_note") {
                    continue;
                }
                if self.hstate.borrow().has_repo_note_card(&studio_name, m.seq) {
                    continue;
                }
                let Some(rel) = m.file.as_deref() else {
                    continue;
                };
                let Ok(raw) = fs::read_to_string(sdir.join(rel)) else {
                    continue;
                };
                let Ok(req) = serde_json::from_str::<RepoNoteRequest>(&raw) else {
                    continue;
                };
                if !valid_entry_id(&req.entry_id) || check_text_safe(&req.text).is_err() {
                    continue;
                }
                let repo = canonical_repo(&req.repo);
                let prev_sha256 = entry_digest(&self.base, &repo, &req.entry_id);
                let action = CardAction::RepoNote {
                    studio: studio_name.clone(),
                    repo: repo.clone(),
                    entry_id: req.entry_id.clone(),
                    text: req.text,
                    source_seq: Some(m.seq),
                };
                let base = CardBase::RepoNoteEntry {
                    repo,
                    entry_id: req.entry_id,
                    prev_sha256,
                };
                fs::create_dir_all(hdir)
                    .map_err(|e| Fail::refused(format!("{}: {e}", hdir.display())))?;
                let why = req.why;
                let event = NewHollywoodEvent::new(
                    EventBy::role("stage"),
                    HollywoodKind::CardProposed { action, why, base },
                );
                let _ = store::append_hollywood_many(hdir, |hs, seq| {
                    let superseded = hs.open_cards_superseded_by(&match &event.kind {
                        HollywoodKind::CardProposed { action, .. } => action.clone(),
                        _ => unreachable!(),
                    });
                    let mut out = Vec::with_capacity(superseded.len() + 1);
                    out.push(HPending {
                        event: event.clone(),
                        files: Vec::new(),
                    });
                    for old in superseded {
                        out.push(HPending {
                            event: NewHollywoodEvent::new(
                                EventBy::role("stage"),
                                HollywoodKind::CardSuperseded {
                                    card: old,
                                    by_card: seq,
                                    reason: "a newer card replaces it".into(),
                                },
                            ),
                            files: Vec::new(),
                        });
                    }
                    Ok::<_, std::convert::Infallible>(out)
                });
                self.hrefresh(hdir);
            }
        }
        Ok(())
    }
}
