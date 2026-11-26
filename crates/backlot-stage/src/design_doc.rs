//! Immutable opening design section files, atomic `design/v<rv>.md`
//! assembly, and frozen `design/v<rv>-final.md` snapshots.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use backlot_model::design::{
    DesignPhase, DesignQuestion, DesignSlot, DesignState, EntrySource, SignMode,
};
use backlot_model::state::StudioState;
use backlot_model::{Event, EventKind};

use crate::args::Common;
use crate::args_design::QuestionInput;
use crate::cli::{Env, Fail};
use crate::store::{self, PendingFile, StoreError};

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

struct TempCleanup(Option<PathBuf>);

impl Drop for TempCleanup {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = fs::remove_file(path);
        }
    }
}

/// Atomically write `bytes` to `dest` via a temp file in `dest.parent()`.
pub(crate) fn atomic_write(dest: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = dest
        .parent()
        .ok_or_else(|| std::io::Error::other("missing destination parent directory"))?;
    fs::create_dir_all(parent)?;
    let n = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = parent.join(format!(".tmp-{}-{n}", std::process::id()));
    let mut guard = TempCleanup(Some(tmp.clone()));
    {
        let mut f = OpenOptions::new().create_new(true).write(true).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_data()?;
    }
    fs::rename(&tmp, dest)?;
    guard.0 = None;
    Ok(())
}

/// Copy one [`PendingFile`] (plus optional trailing annotation) atomically.
pub(crate) fn copy_pending_atomic(pf: &PendingFile, dest: &Path) -> std::io::Result<()> {
    let mut bytes = fs::read(&pf.src)?;
    if let Some(extra) = &pf.append {
        bytes.extend_from_slice(extra.as_bytes());
    }
    atomic_write(dest, &bytes)
}

/// Reject empty/whitespace-only section payloads for `design.entry` before copying.
pub(crate) fn verify_pending_entries<E>(
    stamped: &[Event],
    copies: &[PendingFile],
) -> Result<(), StoreError<E>> {
    for ev in stamped {
        let EventKind::DesignEntry { file, .. } = &ev.kind else {
            continue;
        };
        let Some(pf) = copies.iter().find(|pf| pf.dest == *file) else {
            return Err(StoreError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("missing pending section file for {file}"),
            )));
        };
        let text = fs::read_to_string(&pf.src).map_err(StoreError::Io)?;
        if text.trim().is_empty() {
            return Err(StoreError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "design entry file cannot be empty",
            )));
        }
    }
    Ok(())
}

/// Update `design/v<rv>.md` (and `design/v<rv>-final.md` on approval) for any
/// design events appended in `stamped`.
pub(crate) fn sync_on_append<E>(
    dir: &Path,
    state: &StudioState,
    stamped: &[Event],
) -> Result<(), StoreError<E>> {
    let mut touched_versions = BTreeSet::new();
    let mut approved_versions = BTreeSet::new();
    for ev in stamped {
        match &ev.kind {
            EventKind::DesignOpened { version, .. } | EventKind::DesignEntry { version, .. } => {
                touched_versions.insert(*version);
            }
            EventKind::DesignSigned { version, mode, .. } => {
                touched_versions.insert(*version);
                if *mode == SignMode::Approved {
                    approved_versions.insert(*version);
                }
            }
            _ => {}
        }
    }
    if touched_versions.is_empty() {
        return Ok(());
    }
    for ver in touched_versions {
        let Some(d) = design_for_version(state, ver) else {
            continue;
        };
        let md = render_version_markdown(dir, state, d, None).map_err(StoreError::Io)?;
        atomic_write(&dir.join(format!("design/v{ver}.md")), md.as_bytes())
            .map_err(StoreError::Io)?;
        if approved_versions.contains(&ver) {
            atomic_write(&dir.join(format!("design/v{ver}-final.md")), md.as_bytes())
                .map_err(StoreError::Io)?;
        }
    }
    Ok(())
}

/// Render assembled Markdown for `version`, optionally restricted to `round`.
pub(crate) fn render_markdown(
    dir: &Path,
    state: &StudioState,
    version: u32,
    round: Option<u32>,
) -> Result<Option<String>, Fail> {
    let Some(d) = design_for_version(state, version) else {
        return Ok(None);
    };
    if let Some(r) = round
        && !d.rounds.iter().any(|rd| rd.round == r)
    {
        return Err(Fail::refused(format!("no round {r} in design v{version}")));
    }
    render_version_markdown(dir, state, d, round)
        .map(Some)
        .map_err(|e| Fail::refused(format!("cannot render design v{version}: {e}")))
}

/// Read-only `backlot design show [--version <n>] [--round <r>] [--json]`.
pub(crate) fn design_show(
    common: &Common,
    env: &Env,
    base: &Path,
    version: Option<u32>,
    round: Option<u32>,
    json: bool,
) -> Result<String, Fail> {
    let studio = crate::cli::studio_name(common, env)?;
    let base = crate::cli::resolve_base(env, common, base, &studio)?;
    let dir = store::studio_dir(&base, &studio);
    crate::verbs::require_studio_dir(&dir, &studio)?;
    let state = store::load_state(&dir).map_err(|e| {
        Fail::refused(match e {
            StoreError::Io(err) => err.to_string(),
            StoreError::Corrupt(c) => c.to_string(),
            StoreError::Refused(r) => r.to_string(),
            StoreError::Build(()) => "failed to load studio state".into(),
        })
    })?;
    let target_version = version
        .or_else(|| state.design.as_ref().map(|d| d.version))
        .or_else(|| state.design_history.keys().next_back().copied());
    let Some(ver) = target_version else {
        if json {
            return serde_json::to_string_pretty(&serde_json::json!({
                "studio": studio,
                "design": serde_json::Value::Null,
            }))
            .map_err(|e| Fail::refused(format!("design show json: {e}")));
        }
        return Ok("no opening design recorded yet".to_string());
    };
    let d = design_for_version(&state, ver)
        .ok_or_else(|| Fail::refused(format!("no opening design for req v{ver}")))?;
    if let Some(r) = round
        && !d.rounds.iter().any(|rd| rd.round == r)
    {
        return Err(Fail::refused(format!("no round {r} in design v{ver}")));
    }
    if json {
        let mut owned = d.clone();
        if let Some(r) = round {
            owned.rounds.retain(|rd| rd.round == r);
        }
        return serde_json::to_string_pretty(&owned)
            .map_err(|e| Fail::refused(format!("design show json: {e}")));
    }
    render_markdown(&dir, &state, ver, round)?
        .ok_or_else(|| Fail::refused(format!("no opening design for req v{ver}")))
}

fn design_for_version(state: &StudioState, version: u32) -> Option<&DesignState> {
    state
        .design
        .as_ref()
        .filter(|d| d.version == version)
        .or_else(|| state.design_history.get(&version))
}

fn render_version_markdown(
    dir: &Path,
    state: &StudioState,
    d: &DesignState,
    only_round: Option<u32>,
) -> std::io::Result<String> {
    let mut out = String::new();
    let req_summary = state
        .requirements
        .iter()
        .find(|r| r.version == d.version)
        .map(|r| r.summary.as_str())
        .unwrap_or("");
    if req_summary.is_empty() {
        let _ = writeln!(out, "# Design discussion · req v{}", d.version);
    } else {
        let _ = writeln!(
            out,
            "# Design discussion · req v{}: {req_summary}",
            d.version
        );
    }
    if let Some(reason) = &d.skip_reason {
        let _ = writeln!(out, "\n> Skip suggested: {reason}");
    }
    for r in &d.rounds {
        if only_round.is_some_and(|want| r.round != want) {
            continue;
        }
        let _ = writeln!(out, "\n## R{}", r.round);
        for e in &r.entries {
            let heading = match e.slot {
                DesignSlot::Proposal => {
                    format!("### §R{}.W Proposal · {} (#{})", r.round, e.by, e.seq)
                }
                DesignSlot::Buildability => {
                    format!("### §R{}.D Buildability · {} (#{})", r.round, e.by, e.seq)
                }
                DesignSlot::Summary => {
                    format!("### §R{}.P Summary · {} (#{})", r.round, e.by, e.seq)
                }
                DesignSlot::UserInput => {
                    let src = match e.source {
                        Some(EntrySource::Tell) => "tell",
                        Some(EntrySource::Hook) => "hook",
                        Some(EntrySource::ProducerNote) => "producer_note",
                        None => "note",
                    };
                    format!("### §R{}.U User input · {src} (#{})", r.round, e.seq)
                }
                DesignSlot::Supplement => {
                    let anchor = if e.by == "director" {
                        format!("§R{}.D+", r.round)
                    } else {
                        format!("§R{}.W+", r.round)
                    };
                    format!("### {anchor} Supplement · {} (#{})", e.by, e.seq)
                }
            };
            let _ = writeln!(out, "\n{heading}");
            if !e.refs.is_empty() {
                let refs = e
                    .refs
                    .iter()
                    .map(|seq| format!("#{seq}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = writeln!(out, "\nRefs: {refs}");
            }
            let body = fs::read_to_string(dir.join(&e.file))?;
            let trimmed = body.trim_end();
            if !trimmed.is_empty() {
                let _ = writeln!(out, "\n{trimmed}");
            }
            if !e.questions.is_empty() {
                let _ = writeln!(out, "\n#### Questions\n");
                for q in &e.questions {
                    let sec = q
                        .section_ref
                        .as_deref()
                        .map(|s| format!(", ref: {s}"))
                        .unwrap_or_default();
                    let _ = writeln!(
                        out,
                        "- **{}**: {} *(recommendation: {}{sec})*",
                        q.id, q.prompt, q.recommendation
                    );
                }
            }
        }
    }
    if only_round.is_none() && d.phase == DesignPhase::Signed {
        let sign_seq = d.signed_seq.unwrap_or(0);
        let mode = match d.sign_mode {
            Some(SignMode::Approved) => "approved",
            Some(SignMode::Skipped) => "skipped",
            None => "signed",
        };
        let stamp = utc_timestamp(d.signed_ts_ms.unwrap_or(0));
        let _ = writeln!(out, "\n## Sign-off (#{sign_seq})\n");
        let _ = writeln!(out, "- Signer: user");
        let _ = writeln!(out, "- Mode: {mode}");
        let _ = writeln!(out, "- Round: r{}", d.round);
        let _ = writeln!(out, "- Timestamp: {stamp}");
        if let Some(note) = &d.sign_note {
            let _ = writeln!(out, "- Note: {note}");
        }
    }
    Ok(out)
}

/// Format unix milliseconds as `YYYY-MM-DD HH:MM:SS UTC`.
fn utc_timestamp(ms: u64) -> String {
    let total_secs = ms / 1000;
    let sod = total_secs % 86_400;
    let hh = sod / 3_600;
    let mm = (sod % 3_600) / 60;
    let ss = sod % 60;
    let z = (total_secs / 86_400) as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02} UTC")
}

const MAX_QUESTIONS: usize = 3;

fn parse_question_json(raw: &str, label: &str) -> Result<Vec<DesignQuestion>, Fail> {
    let val: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| Fail::usage(format!("bad {label} JSON: {e}")))?;
    if val.is_array() {
        serde_json::from_value(val).map_err(|e| Fail::usage(format!("bad {label} array: {e}")))
    } else {
        let q: DesignQuestion =
            serde_json::from_value(val).map_err(|e| Fail::usage(format!("bad {label}: {e}")))?;
        Ok(vec![q])
    }
}

pub(crate) fn load_questions(
    inputs: &[QuestionInput],
    questions_file: Option<&Path>,
) -> Result<Vec<DesignQuestion>, Fail> {
    let mut out = Vec::new();
    for item in inputs {
        match item {
            QuestionInput::Inline(raw) => out.extend(parse_question_json(raw, "--question")?),
            QuestionInput::File(path) => {
                let raw = fs::read_to_string(path).map_err(|e| {
                    Fail::refused(format!("cannot read --question {}: {e}", path.display()))
                })?;
                out.extend(parse_question_json(&raw, "--question")?);
            }
        }
    }
    if let Some(path) = questions_file {
        let raw = fs::read_to_string(path).map_err(|e| {
            Fail::refused(format!(
                "cannot read --questions-file {}: {e}",
                path.display()
            ))
        })?;
        out.extend(parse_question_json(&raw, "--questions-file")?);
    }
    if out.len() > MAX_QUESTIONS {
        return Err(Fail::refused(format!(
            "design summary accepts at most {MAX_QUESTIONS} questions, got {}",
            out.len()
        )));
    }
    Ok(out)
}
