//! Two-layer terminal user-input capture storage & ledger sync for opening
//! design rounds when `design_gate = "required"`:
//! 1. Verbatim hook capture to `<studio>/design/.hook-capture.jsonl` and
//!    cursor-tracked `design.entry` (`user_input`, `hook`) ingestion.
//! 2. Pending-note sidecar `<studio>/design/.pending-user-note.json` for
//!    gating `backlot design next` until `backlot design note --from user`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use backlot_model::Event;
use backlot_model::EventBy;
use backlot_model::NewEvent;
use backlot_model::design::{DesignGate, DesignPhase, DesignSlot, DesignState, EntrySource};
use backlot_model::event::EventKind;
use backlot_model::state::StudioState;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cli::{Env, Fail, check_studio_name};
use crate::config;
use crate::design_doc;
use crate::hook::{LINE_CAP, cut};
use crate::store::{self, Pending, PendingFile};

const CAPTURE_FILE: &str = ".hook-capture.jsonl";
const CURSOR_FILE: &str = ".hook-capture-cursor.json";
const PENDING_NOTE_FILE: &str = ".pending-user-note.json";

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct CaptureRecord {
    pub ts_ms: u64,
    pub version: u32,
    pub round: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub prompt: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
struct CaptureCursor {
    offset: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PendingUserNote {
    version: u32,
    round: u32,
    ts_ms: u64,
    after_user_input_seq: u64,
}

pub(crate) fn hook_capture_path(dir: &Path) -> PathBuf {
    dir.join("design").join(CAPTURE_FILE)
}

fn cursor_path(dir: &Path) -> PathBuf {
    dir.join("design").join(CURSOR_FILE)
}

pub(crate) fn pending_note_path(dir: &Path) -> PathBuf {
    dir.join("design").join(PENDING_NOTE_FILE)
}

pub(crate) fn active_waiting_state(state: &StudioState) -> Option<&DesignState> {
    if state
        .studio
        .as_ref()
        .is_none_or(|s| s.design_gate != DesignGate::Required)
        || state.design_phase() != DesignPhase::WaitingUser
    {
        return None;
    }
    state
        .design
        .as_ref()
        .filter(|d| d.version == state.requirement_version && d.round > 0)
}

fn latest_user_input_seq(d: &DesignState) -> u64 {
    d.rounds
        .iter()
        .find(|r| r.round == d.round)
        .and_then(|r| {
            r.entries
                .iter()
                .filter(|e| e.slot == DesignSlot::UserInput)
                .map(|e| e.seq)
                .max()
        })
        .unwrap_or(0)
}

fn ignored_prompt(prompt: &str) -> bool {
    let trimmed = prompt.trim();
    trimmed.is_empty() || trimmed.starts_with("[backlot]")
}

fn serialize_capped_record(mut rec: CaptureRecord) -> String {
    let line = |r: &CaptureRecord| -> String {
        let mut s = serde_json::to_string(r).unwrap_or_default();
        s.push('\n');
        s
    };
    let full = line(&rec);
    if full.len() <= LINE_CAP {
        return full;
    }
    let orig = rec.prompt;
    let mut lo = 0usize;
    let mut hi = orig.len();
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        rec.prompt = cut(&orig, mid).to_string();
        if line(&rec).len() <= LINE_CAP {
            lo = rec.prompt.len();
            if lo < mid && mid - lo <= 4 {
                break;
            }
        } else {
            hi = mid - 1;
        }
    }
    rec.prompt = cut(&orig, lo).to_string();
    line(&rec)
}

/// Inspect a normalized hook payload and return `(capture_path, json_line)` when
/// all capture conditions hold (`hook_capture` enabled, `BACKLOT_ROLE=producer`,
/// studio `design_gate = required`, `WaitingUser`, normalized `UserPromptSubmit`,
/// non-empty prompt without `[backlot]` prefix). Silently returns `None` otherwise.
pub(crate) fn prepare_hook_capture(
    env: &Env,
    normalized: &Value,
    ts_ms: u64,
) -> Option<(PathBuf, String)> {
    if normalized.get("hook_event_name").and_then(Value::as_str) != Some("UserPromptSubmit") {
        return None;
    }
    let prompt = normalized.get("prompt").and_then(Value::as_str)?;
    if ignored_prompt(prompt) {
        return None;
    }
    if env.get("BACKLOT_ROLE") != Some("producer") {
        return None;
    }
    let state_dir = env.get("BACKLOT_STATE_DIR").filter(|s| !s.is_empty())?;
    let studio_raw = env.get("BACKLOT_STUDIO").filter(|s| !s.is_empty())?;
    let studio = check_studio_name(studio_raw).ok()?;
    if let Ok(cfg) = config::design(&env.vars)
        && !cfg.hook_capture
    {
        return None;
    }
    let studio_dir = store::studio_dir(Path::new(state_dir), &studio);
    let state = store::load_state(&studio_dir).ok()?;
    let d = active_waiting_state(&state)?;
    let session_id = normalized
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let rec = CaptureRecord {
        ts_ms,
        version: d.version,
        round: d.round,
        session_id,
        prompt: prompt.to_string(),
    };
    Some((hook_capture_path(&studio_dir), serialize_capped_record(rec)))
}

/// True when `dir/design/.pending-user-note.json` marks an unrecorded producer
/// terminal turn for the current `(version, round)` in `WaitingUser`.
pub(crate) fn has_pending_note(dir: &Path, state: &StudioState) -> bool {
    let Some(d) = active_waiting_state(state) else {
        return false;
    };
    let Ok(raw) = fs::read_to_string(pending_note_path(dir)) else {
        return false;
    };
    let Ok(note) = serde_json::from_str::<PendingUserNote>(&raw) else {
        return false;
    };
    note.version == d.version
        && note.round == d.round
        && latest_user_input_seq(d) <= note.after_user_input_seq
}

/// Clear `.pending-user-note.json` after a ledger commit resolves `WaitingUser`
/// or appends a `UserInput` entry.
pub(crate) fn clear_pending_on_append(dir: &Path, state: &StudioState, stamped: &[Event]) {
    let should_clear = stamped.iter().any(|ev| {
        matches!(
            ev.kind,
            EventKind::DesignEntry {
                slot: DesignSlot::UserInput,
                ..
            } | EventKind::DesignOpened { .. }
                | EventKind::DesignSigned { .. }
                | EventKind::ReqVersion { .. }
        )
    });
    if !should_clear {
        return;
    }
    if state
        .studio
        .as_ref()
        .is_none_or(|s| s.design_gate != DesignGate::Required)
        && !pending_note_path(dir).exists()
    {
        return;
    }
    let _ = fs::remove_file(pending_note_path(dir));
}

fn read_cursor(dir: &Path, file_len: u64) -> u64 {
    let offset = fs::read_to_string(cursor_path(dir))
        .ok()
        .and_then(|s| serde_json::from_str::<CaptureCursor>(&s).ok())
        .map_or(0, |c| c.offset);
    if offset > file_len { 0 } else { offset }
}

fn write_cursor(dir: &Path, offset: u64) -> Result<(), Fail> {
    let bytes = serde_json::to_vec(&CaptureCursor { offset })
        .map_err(|e| Fail::refused(format!("design capture cursor: {e}")))?;
    design_doc::atomic_write(&cursor_path(dir), &bytes)
        .map_err(|e| Fail::refused(format!("design capture cursor: {e}")))
}

fn unread_complete_lines(dir: &Path) -> Option<(u64, Vec<(u64, String)>)> {
    let bytes = fs::read(hook_capture_path(dir)).ok()?;
    let file_len = bytes.len() as u64;
    let start = read_cursor(dir, file_len) as usize;
    if start >= bytes.len() {
        return None;
    }
    let slice = &bytes[start..];
    let last_nl = slice.iter().rposition(|&b| b == b'\n')?;
    let complete = &slice[..=last_nl];
    let mut lines = Vec::new();
    let mut pos = 0usize;
    for part in complete.split_inclusive(|&b| b == b'\n') {
        pos += part.len();
        let next_off = (start + pos) as u64;
        if let Ok(text) = std::str::from_utf8(part) {
            let trimmed = text.trim_end_matches(['\r', '\n']);
            if !trimmed.is_empty() {
                lines.push((next_off, trimmed.to_string()));
            }
        }
    }
    (!lines.is_empty()).then_some(((start + pos) as u64, lines))
}

pub(crate) fn advance_cursor_past_unread(dir: &Path) -> Result<(), Fail> {
    if let Some((end_off, _)) = unread_complete_lines(dir) {
        write_cursor(dir, end_off)?;
    }
    Ok(())
}

pub(crate) fn ingest_capture_lines(dir: &Path, version: u32, round: u32) -> Result<(), Fail> {
    let Some((_, lines)) = unread_complete_lines(dir) else {
        return Ok(());
    };
    for (next_off, raw) in lines {
        if let Ok(rec) = serde_json::from_str::<CaptureRecord>(&raw)
            && rec.version == version
            && rec.round == round
            && !ignored_prompt(&rec.prompt)
        {
            append_hook_user_input(dir, &rec.prompt)?;
        }
        write_cursor(dir, next_off)?;
    }
    Ok(())
}

struct TempFileGuard(PathBuf);

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn append_hook_user_input(dir: &Path, prompt: &str) -> Result<(), Fail> {
    if !dir.join("events.jsonl").is_file() {
        return Err(Fail::refused("studio archived"));
    }
    let n = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = dir
        .join("design")
        .join(format!(".new-hook-input-{}-{n}.md", std::process::id()));
    design_doc::atomic_write(&tmp, prompt.as_bytes())
        .map_err(|e| Fail::refused(format!("design hook input file: {e}")))?;
    let _cleanup = TempFileGuard(tmp.clone());

    let src = tmp;
    let body = prompt.to_string();
    store::append(dir, move |state, seq| {
        let Some(d) = active_waiting_state(state) else {
            return Err(Fail::refused("design is no longer waiting on user"));
        };
        if body.trim().is_empty() {
            return Err(Fail::refused("captured user input is empty"));
        }
        let version = d.version;
        let round = d.round;
        let dest = format!("design/v{version}/r{round}-{seq}-user_input.md");
        Ok(Pending {
            event: NewEvent::new(
                EventBy::role("stage"),
                EventKind::DesignEntry {
                    version,
                    round,
                    slot: DesignSlot::UserInput,
                    file: dest.clone(),
                    source: Some(EntrySource::Hook),
                    refs: Vec::new(),
                    questions: Vec::new(),
                },
            ),
            files: vec![PendingFile {
                src,
                dest,
                append: None,
            }],
        })
    })
    .map(|_| ())
    .or_else(|e| match e {
        store::StoreError::Build(Fail::Refused(_)) => Ok(()),
        other => Err(Fail::refused(format!(
            "append captured user input: {other:?}"
        ))),
    })
}

pub(crate) fn mark_pending_user_note(dir: &Path, d: &DesignState, ts_ms: u64) -> Result<(), Fail> {
    let note = PendingUserNote {
        version: d.version,
        round: d.round,
        ts_ms,
        after_user_input_seq: latest_user_input_seq(d),
    };
    let bytes =
        serde_json::to_vec(&note).map_err(|e| Fail::refused(format!("pending user note: {e}")))?;
    design_doc::atomic_write(&pending_note_path(dir), &bytes)
        .map_err(|e| Fail::refused(format!("pending user note: {e}")))
}
