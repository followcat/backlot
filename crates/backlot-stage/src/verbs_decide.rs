//! `backlot decide` execution: formats a shared `AD-<n>` body, expands
//! `--notify`, atomically appends one or more `purpose: "decision"` messages
//! sharing a single `messages/<seq>-ad-<n>.md` file, and refreshes
//! `decisions/current.md` under `events.lock`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use backlot_model::decisions::{format_id, parse_decision_id, prepare_decision_body};
use backlot_model::event::{EventKind, NewEvent};
use backlot_model::state::StudioState;

use crate::cli::{Caller, Fail};
use crate::store::{self, Pending, PendingFile};
use crate::verbs::require_studio_dir;

static NEXT_TMP: AtomicU64 = AtomicU64::new(0);

struct TempFileGuard(PathBuf);

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn resolve_recipients(state: &StudioState, notify: Option<&str>) -> Result<Vec<String>, Fail> {
    let Some(raw) = notify else {
        return Ok(vec!["producer".to_string()]);
    };
    let mut out = Vec::new();
    for part in raw.split(',') {
        let token = part.trim();
        if token.is_empty() {
            return Err(Fail::usage("--notify must not contain empty roles"));
        }
        if token == "active-actors" {
            let mut actors: Vec<String> = state
                .tasks
                .values()
                .filter(|t| !t.status.is_terminal() && t.cast.is_some())
                .map(|t| format!("actor-{}-c{}", t.id, t.casts.len()))
                .filter(|role| !state.roles.get(role).is_some_and(|r| r.closed))
                .collect();
            actors.sort();
            for actor in actors {
                if !out.contains(&actor) {
                    out.push(actor);
                }
            }
        } else if !out.contains(&token.to_string()) {
            out.push(token.to_string());
        }
    }
    if out.is_empty() {
        return Err(Fail::refused("no active actors to notify"));
    }
    Ok(out)
}

/// Queue a shared architectural decision (`purpose = "decision"`) on the
/// studio ledger.
pub(crate) fn decide(
    base: &Path,
    caller: &Caller,
    id: &str,
    file: &Path,
    supersedes: Option<&str>,
    notify: Option<&str>,
) -> Result<String, Fail> {
    if !matches!(caller.role.as_str(), "user" | "producer" | "director") {
        return Err(Fail::refused(format!(
            "role '{}' cannot do this; the user, the producer or the director can",
            caller.role
        )));
    }
    let id_num = parse_decision_id(id)
        .ok_or_else(|| Fail::usage(format!("invalid --id '{id}': expected AD-<n>")))?;
    let canonical_id = format_id(id_num);
    let raw_body = fs::read_to_string(file)
        .map_err(|e| Fail::refused(format!("cannot read --file {}: {e}", file.display())))?;
    let formatted =
        prepare_decision_body(&canonical_id, supersedes, &raw_body).map_err(Fail::refused)?;

    let dir = store::studio_dir(base, &caller.studio);
    require_studio_dir(&dir, &caller.studio)?;

    let tmp_name = format!(
        ".tmp-decide-{}-{}",
        std::process::id(),
        NEXT_TMP.fetch_add(1, Ordering::Relaxed)
    );
    let tmp_path = dir.join(tmp_name);
    fs::write(&tmp_path, formatted.as_bytes())
        .map_err(|e| Fail::refused(format!("cannot stage decision body: {e}")))?;
    let _guard = TempFileGuard(tmp_path.clone());

    let by = caller.by();
    let notify_owned = notify.map(str::to_string);
    let events = store::append_many(&dir, move |state, seq| {
        let recipients = resolve_recipients(state, notify_owned.as_deref())?;
        let dest = format!("messages/{seq}-ad-{id_num}.md");
        let mut pending = Vec::with_capacity(recipients.len());
        for (i, to) in recipients.into_iter().enumerate() {
            let files = if i == 0 {
                vec![PendingFile {
                    src: tmp_path.clone(),
                    dest: dest.clone(),
                    append: None,
                }]
            } else {
                Vec::new()
            };
            pending.push(Pending {
                event: NewEvent::new(
                    by.clone(),
                    EventKind::MessageQueued {
                        to,
                        file: Some(dest.clone()),
                        purpose: Some("decision".into()),
                        source_seq: None,
                    },
                ),
                files,
            });
        }
        Ok(pending)
    })?;

    let first_seq = events.first().map(|e| e.seq).unwrap_or(0);
    let recipients: Vec<&str> = events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::MessageQueued { to, .. } => Some(to.as_str()),
            _ => None,
        })
        .collect();
    Ok(format!(
        "event {first_seq}: decision {canonical_id} → {}",
        recipients.join(", ")
    ))
}
