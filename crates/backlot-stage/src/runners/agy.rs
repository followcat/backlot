//! Antigravity (`agy`): the runner mints the session id — the
//! stage finds it under `~/.gemini/antigravity-cli/conversations/`
//! after launch and records `role.session`: each `<uuid>.db` is a
//! sqlite file whose bytes embed the workspace's `file://` URIs
//! verbatim, so a byte scan finds the role's own — no sqlite read
//! needed. The launch always passes `--mode accept-edits` (the
//! measured work posture, for either permission profile),
//! `--effort <effort>`, `--model <model>` and `--settings <file>`
//! naming the workdir's session-scoped file; `-i` carries the
//! first prompt on a fresh launch only — a `--conversation <id>`
//! resume sends the prompt after the composer opens, devin's
//! rule.
//!
//! Isolation reality: `~/.gemini/antigravity-cli` hangs off `HOME`
//! only — no per-role tree exists. The boundary is the workdir's
//! own `.agy-settings.json`, written before every start and resume
//! with `toolPermission = "always-proceed"`: `--settings` is
//! session-only and never saved, so the role's tools proceed
//! without the prompts a role cannot answer while the user's
//! global settings stay untouched.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use backlot_model::ActorIdentity;

use super::splice::{splice_member, write600};
use super::{Launch, LaunchCtx, PlanError, Prep, Session};

/// The attention reason a failed `prepare` raises.
pub const AGY_PREP_FAILED: &str = "agy prep failed";

/// The model ids `agy models` lists, in its own order — the
/// Gemini families, the orchestrators, claude and gpt-oss; the
/// thinking depth is part of each id and `--effort` sets the
/// session's level on top. An id the list does not name still
/// goes in as free text.
pub const MODELS: &[&str] = &[
    "gemini-4-neon-high",
    "gemini-4-neon-xhigh",
    "gemini-4-neon-medium",
    "gemini-4-neon-low",
    "gemini-4-carbon-medium",
    "gemini-4-carbon-high",
    "gemini-4-carbon-low",
    "gemini-4-argon-medium",
    "gemini-4-argon-high",
    "gemini-4-argon-low",
    "gemini-4-hydrogen-high",
    "gemini-4-hydrogen-xhigh",
    "gemini-4-hydrogen-medium",
    "gemini-4-hydrogen-low",
    "gemini-pro-orchestrator-neon",
    "gemini-pro-orchestrator",
    "gemini-pro-orchestrator-argon",
    "gemini-3.8-flash-high",
    "gemini-3.8-flash-medium",
    "gemini-3.8-flash-low",
    "claude-opus-5-5-low",
    "claude-opus-5-5-medium",
    "claude-opus-5-5-high",
    "claude-sonnet-5-5-low",
    "claude-sonnet-5-5-medium",
    "claude-sonnet-5-5-high",
    "gpt-oss-120b-medium",
];

/// `--effort`'s accepted values, `agy --help`.
pub const EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// The per-launch filesystem work: the workdir's session-scoped
/// settings file.
#[derive(Debug, Clone, PartialEq)]
pub struct AgyPrep {
    /// `<cwd>/.agy-settings.json`.
    pub settings: PathBuf,
}

/// The settings file's bytes: `{"toolPermission":"always-proceed"}`
/// written fresh, or the member spliced byte-exact into an
/// existing object — every other member (the role's own settings)
/// keeps its whitespace and order. `always-proceed` is the posture
/// the measured account runs; the file is session-only, so it
/// never rewrites the user's.
pub(crate) fn settings_content(existing: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let fresh = || Ok(b"{\"toolPermission\":\"always-proceed\"}\n".to_vec());
    let Some(text) = existing.and_then(|b| std::str::from_utf8(b).ok()) else {
        return fresh();
    };
    match splice_member(text.trim(), "toolPermission", "\"always-proceed\"") {
        Some(spliced) => Ok(format!("{spliced}\n").into_bytes()),
        // A file that is not a JSON object is healed, not preserved.
        None => fresh(),
    }
}

/// Write `<cwd>/.agy-settings.json` before the launch — agy reads
/// it once at startup, so the value must be in place before
/// `terminal.run` and a failure is an attention, never a
/// half-allowed role. The file lands mode 600 like every
/// stage-written file; always `Ok(false)` — the workdir itself is
/// the boundary and there is no open-dir drift to report.
pub fn prepare(p: &AgyPrep) -> Result<bool, String> {
    let dir = p.settings.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let existing = std::fs::read(&p.settings).ok();
    let bytes = settings_content(existing.as_deref())?;
    write600(&p.settings, &bytes).map_err(|e| format!("{}: {e}", p.settings.display()))?;
    Ok(false)
}

/// A file's birth time, or its mtime where birth time is
/// unsupported.
fn file_time_ms(path: &Path) -> Option<u64> {
    let md = std::fs::metadata(path).ok()?;
    let t = md.created().or_else(|_| md.modified()).ok()?;
    Some(
        t.duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    )
}

/// `path`'s `file://` URI the way a conversation db embeds it:
/// unreserved bytes plus `/` and `,` pass through, everything
/// else is `%XX` UTF-8 — `acme shop` lands as `acme%20shop`. The
/// raw path itself stays a needle too: either form in the file
/// counts as the match.
fn uri_needles(cwd: &str) -> [Vec<u8>; 2] {
    const KEEP: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~/,:";
    let mut enc = Vec::with_capacity(cwd.len() + 8);
    for &b in cwd.as_bytes() {
        if KEEP.contains(&b) {
            enc.push(b);
        } else {
            enc.extend_from_slice(format!("%{b:02X}").as_bytes());
        }
    }
    [
        format!("file://{cwd}").into_bytes(),
        [b"file://".as_slice(), &enc].concat(),
    ]
}

/// `hay` holds `needle` anywhere — a plain byte scan; the dbs
/// embed the URI inside protobuf blobs, never aligned.
fn contains_bytes(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

/// The session id a live agy role's launch minted: the newest
/// `<uuid>.db` under `conversations/` created at or after the
/// role's start whose bytes name `file://<cwd>` — percent-encoded
/// or verbatim — and no other role already claims.
pub fn session_at(
    conversations: &Path,
    cwd: &str,
    started_ms: u64,
    taken: &BTreeSet<String>,
) -> Option<String> {
    let needles = uri_needles(cwd);
    let entries = std::fs::read_dir(conversations).ok()?;
    let mut best: Option<(u64, String)> = None;
    for e in entries.filter_map(|e| e.ok()) {
        let path = e.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let Some(stem) = name.strip_suffix(".db") else {
            continue;
        };
        if stem.is_empty() || taken.contains(stem) {
            continue;
        }
        let Some(ms) = file_time_ms(&path) else {
            continue;
        };
        if ms < started_ms {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if !needles.iter().any(|n| contains_bytes(&bytes, n)) {
            continue;
        }
        if best.as_ref().is_none_or(|(b, _)| ms > *b) {
            best = Some((ms, stem.to_string()));
        }
    }
    best.map(|(_, id)| id)
}

/// The session id a resumed terminal's argv carries: the value
/// after `--conversation`. `-c`/`--continue` are booleans —
/// continue the most recent — and name no id.
pub fn session_in_argv(argv: &[String]) -> Option<String> {
    let flag = argv.iter().position(|a| a == "--conversation")?;
    argv.get(flag + 1).cloned()
}

pub fn plan(identity: &ActorIdentity, ctx: &LaunchCtx) -> Result<Launch, PlanError> {
    match identity.permission_profile.as_str() {
        // Both profiles take the measured posture — accept edits,
        // everything else still asks.
        "standard" | "worktree" => {}
        other => return Err(PlanError::profile(other)),
    }
    let settings = Path::new(ctx.cwd).join(".agy-settings.json");
    let mut argv = vec!["agy".to_string()];
    if let Some(id) = ctx.resume {
        argv.extend(["--conversation".into(), id.to_string()]);
    }
    argv.extend([
        "--mode".into(),
        "accept-edits".into(),
        "--effort".into(),
        identity.effort.clone().unwrap_or_default(),
        "--model".into(),
        identity.model.clone(),
        "--settings".into(),
        settings.to_string_lossy().into_owned(),
    ]);
    // `-i` carries the first prompt on a fresh launch only — a
    // resume's arrives after the composer opens, devin's rule.
    if ctx.resume.is_none()
        && let Some(p) = ctx.prompt
    {
        argv.extend(["-i".into(), p.into()]);
    }
    Ok(Launch {
        argv,
        env: BTreeMap::new(),
        session: Session::PostLaunch,
        hooks: None,
        prep: Some(Prep::Agy(AgyPrep { settings })),
    })
}
