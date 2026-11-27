//! Antigravity (`agy`): the runner mints the session id — the
//! stage finds it after launch and records `role.session`. agy
//! 1.5.14 keeps `~/.gemini/antigravity-cli/cache/last_conversations.json`,
//! mapping an absolute cwd to the id of the last conversation
//! started there; the role's own cwd names its conversation
//! directly. Older agy has no map, and the role's workspace URI
//! may not survive in the db at all, so the fallback stays: each
//! `<uuid>.db` under `conversations/` is a sqlite file whose bytes
//! embed the workspace's `file://` URIs verbatim, and a byte scan
//! finds the role's own — no sqlite read needed. The launch always
//! passes `--mode accept-edits` (the measured work posture, for either permission profile),
//! `--effort <effort>`, `--model <model>` and `--settings <file>`
//! naming the workdir's session-scoped file; `-i` carries the
//! first prompt on a fresh launch only — a `--conversation <id>`
//! resume sends the prompt after the composer opens, devin's
//! rule.
//!
//! Isolation reality: `~/.gemini/antigravity-cli` hangs off `HOME`
//! only — no per-role tree exists. The boundary is the workdir's
//! own `.agy-settings.json`, written before every start and resume
//! with `toolPermission = "always-proceed"`, policy-aware
//! `permissions.deny`, and (when `[stage] answer_trust` is on)
//! `trustedWorkspaces` containing the canonical absolute work-area
//! path: `--settings` is session-only and never saved, so the role's
//! tools proceed and the folder-trust dialog stays away while the
//! user's global settings stay untouched.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use backlot_model::ActorIdentity;

use super::splice::{splice_member, splice_permissions, write600};
use super::{Launch, LaunchCtx, PlanError, Prep, Session, predates_start};

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

/// The per-launch filesystem work: the workdir's session-scoped
/// settings file.
#[derive(Debug, Clone, PartialEq)]
pub struct AgyPrep {
    /// `<cwd>/.agy-settings.json`.
    pub settings: PathBuf,
    /// `<cwd>/.agents/hooks.json`.
    pub hooks: PathBuf,
    /// `<studio>/roles/<role>/signals.jsonl`.
    pub signals: PathBuf,
    /// Work-area path to add to `trustedWorkspaces`, or `None` when
    /// `[stage] answer_trust` is off.
    pub trusted_workspace: Option<PathBuf>,
    /// When true, deny git write commands in `.agy-settings.json`.
    pub readonly: bool,
}

/// Command prefixes denied by agy's session `.agy-settings.json`
/// (`command(<prefix>)`, no trailing ` *` — measured in agy 1.5.16) as
/// defence in depth ahead of the outward-action shims (D3).
pub(crate) const DENY_PREFIXES: &[&str] = &[
    "gh release create",
    "gh release delete",
    "gh release edit",
    "gh release upload",
    "gh release delete-asset",
    "gh repo create",
    "gh repo delete",
    "gh repo edit",
    "gh repo fork",
    "gh repo rename",
    "gh repo archive",
    "gh repo unarchive",
    "gh repo sync",
    "gh repo clone",
    "gh repo set-default",
    "gh repo deploy-key",
    "gh issue create",
    "gh issue close",
    "gh issue comment",
    "gh issue delete",
    "gh issue edit",
    "gh issue reopen",
    "gh issue transfer",
    "gh issue lock",
    "gh issue unlock",
    "gh issue pin",
    "gh issue unpin",
    "gh issue develop",
    "gh api",
    "hc update",
    "hc machine",
    "hc server stop",
    "hc run",
    "hc exec",
    "hc send",
    "hc keys",
    "hc connect",
    "systemd-run",
    "tmux new -d",
    "tmux new-session -d",
    "scp",
    "rsync",
    "ssh",
    "npm publish",
    "cargo publish",
];

const READONLY_DENY_PREFIXES: &[&str] = &[
    "git add",
    "git commit",
    "git checkout",
    "git switch",
    "git restore",
    "git reset",
    "git clean",
    "git rm",
    "git mv",
    "git merge",
    "git rebase",
    "git cherry-pick",
    "git revert",
    "git tag",
    "git branch -d",
    "git branch -D",
    "git branch -m",
    "git branch -M",
    "git push",
    "git pull",
    "git stash",
];

fn deny_rules(readonly: bool) -> Vec<String> {
    let extra = if readonly {
        READONLY_DENY_PREFIXES
    } else {
        &[]
    };
    DENY_PREFIXES
        .iter()
        .chain(extra.iter())
        .map(|p| format!("command({p})"))
        .collect()
}

fn permissions_json(existing: &str, readonly: bool) -> String {
    let mut map = serde_json::from_str::<serde_json::Value>(existing)
        .ok()
        .and_then(|v| v.get("permissions").and_then(|p| p.as_object()).cloned())
        .unwrap_or_default();
    map.insert("deny".into(), serde_json::json!(deny_rules(readonly)));
    serde_json::to_string(&serde_json::Value::Object(map)).unwrap_or_default()
}

fn trusted_workspaces_json(existing: &str, cwd: &str) -> String {
    let mut list = serde_json::from_str::<serde_json::Value>(existing)
        .ok()
        .and_then(|v| {
            v.get("trustedWorkspaces")
                .and_then(|t| t.as_array())
                .cloned()
        })
        .unwrap_or_default();
    if !list.iter().any(|entry| entry.as_str() == Some(cwd)) {
        list.push(serde_json::Value::String(cwd.to_string()));
    }
    serde_json::to_string(&serde_json::Value::Array(list)).unwrap_or_default()
}

/// The settings file's bytes: `toolPermission = "always-proceed"`,
/// `permissions.deny`, and (when `trusted_workspace` is `Some`)
/// `trustedWorkspaces` written fresh or spliced byte-exact into an
/// existing object — every other member keeps its whitespace and order.
/// Pure: `trusted_workspace` is written verbatim with no filesystem calls.
#[cfg(test)]
pub(crate) fn settings_content(
    existing: Option<&[u8]>,
    trusted_workspace: Option<&str>,
) -> Result<Vec<u8>, String> {
    settings_content_for(existing, trusted_workspace, false)
}

fn settings_content_for(
    existing: Option<&[u8]>,
    trusted_workspace: Option<&str>,
    readonly: bool,
) -> Result<Vec<u8>, String> {
    let fresh = || {
        let mut obj = serde_json::Map::new();
        obj.insert("toolPermission".into(), serde_json::json!("always-proceed"));
        obj.insert(
            "permissions".into(),
            serde_json::json!({ "deny": deny_rules(readonly) }),
        );
        if let Some(cwd) = trusted_workspace {
            obj.insert("trustedWorkspaces".into(), serde_json::json!([cwd]));
        }
        Ok(format!(
            "{}\n",
            serde_json::to_string(&serde_json::Value::Object(obj)).unwrap_or_default()
        )
        .into_bytes())
    };
    let Some(text) = existing.and_then(|b| std::str::from_utf8(b).ok()) else {
        return fresh();
    };
    let trimmed = text.trim();
    let Some(with_tp) = splice_member(trimmed, "toolPermission", "\"always-proceed\"") else {
        // A file that is not a JSON object is healed, not preserved.
        return fresh();
    };
    let perms = permissions_json(trimmed, readonly);
    let Some(with_perms) = splice_permissions(&with_tp, &perms) else {
        return fresh();
    };
    let Some(cwd) = trusted_workspace else {
        return Ok(format!("{with_perms}\n").into_bytes());
    };
    let tw = trusted_workspaces_json(trimmed, cwd);
    match splice_member(&with_perms, "trustedWorkspaces", &tw) {
        Some(spliced) => Ok(format!("{spliced}\n").into_bytes()),
        None => fresh(),
    }
}

/// Write `<cwd>/.agy-settings.json` before the launch — agy reads
/// it once at startup, so the value must be in place before
/// `terminal.run` and a failure is an attention, never a
/// half-allowed role. Canonicalizes `p.trusted_workspace` after
/// creating the work directory so agy sees its physical cwd. The file
/// lands mode 600 like every stage-written file; always `Ok(None)` —
/// the workdir itself is the boundary and there is no open-dir drift
/// to report.
pub fn prepare(p: &AgyPrep) -> Result<Option<super::PrepNote>, String> {
    let dir = p
        .settings
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let existing = std::fs::read(&p.settings).ok();
    let trusted = p.trusted_workspace.as_deref().map(|ws| {
        let target = if ws.as_os_str().is_empty() {
            Path::new(".")
        } else {
            ws
        };
        std::fs::canonicalize(target)
            .unwrap_or_else(|_| target.to_path_buf())
            .to_string_lossy()
            .into_owned()
    });
    let bytes = settings_content_for(existing.as_deref(), trusted.as_deref(), p.readonly)?;
    write600(&p.settings, &bytes).map_err(|e| format!("{}: {e}", p.settings.display()))?;

    const REL: &str = ".agents/hooks.json";
    let cwd = super::hook_files::work_dir_of(&p.hooks);
    if super::hook_files::is_tracked(cwd, REL) {
        return Ok(Some(super::PrepNote::HooksUnavailable(REL)));
    }
    let was_new = !p.hooks.exists();
    let hdir = p.hooks.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(hdir).map_err(|e| format!("{}: {e}", hdir.display()))?;
    let existing_hooks = std::fs::read(&p.hooks).ok();
    let hook_bytes = super::hook_files::agy_hooks_content(existing_hooks.as_deref(), &p.signals);
    write600(&p.hooks, &hook_bytes).map_err(|e| format!("{}: {e}", p.hooks.display()))?;
    super::hook_files::maybe_exclude_new(cwd, was_new, REL)?;
    Ok(None)
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

/// The id `cache/last_conversations.json` — agy's own cwd → last
/// conversation map, the sibling of `conversations/` — assigns
/// `cwd`, when that conversation is usable: the value is a plain
/// stem (no separator, no `..`), its `<id>.db` exists, was born at
/// or after `started_ms` (up to [`super::BIRTH_SLACK_MS`] before —
/// the same clock the scan reads) and no other role claims it. A
/// missing, unreadable or malformed file is no answer, never an
/// error; agy older than 1.5.14 keeps no map at all.
fn mapped_session(
    conversations: &Path,
    cwd: &str,
    started_ms: u64,
    taken: &BTreeSet<String>,
) -> Option<String> {
    let map = conversations
        .parent()?
        .join("cache/last_conversations.json");
    let text = std::fs::read_to_string(&map).ok()?;
    let ids: BTreeMap<String, String> = serde_json::from_str(&text).ok()?;
    let id = ids.get(cwd)?;
    // A plain stem only: the value names `conversations/<id>.db`,
    // so a separator or a `..` segment is no answer at all.
    if id.is_empty() || id.contains(['/', '\\']) || id.contains("..") || taken.contains(id) {
        return None;
    }
    let db = conversations.join(format!("{id}.db"));
    (!predates_start(file_time_ms(&db)?, started_ms)).then(|| id.clone())
}

/// The session id a live agy role's launch minted: the id agy's
/// own cwd → conversation map names for this cwd when its db is
/// new enough, else the newest `<uuid>.db` under `conversations/`
/// created at or after the role's start (up to
/// [`super::BIRTH_SLACK_MS`] before it) whose bytes name
/// `file://<cwd>` — percent-encoded or verbatim — and no other
/// role already claims.
pub fn session_at(
    conversations: &Path,
    cwd: &str,
    started_ms: u64,
    taken: &BTreeSet<String>,
) -> Option<String> {
    if let Some(id) = mapped_session(conversations, cwd, started_ms, taken) {
        return Some(id);
    }
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
        if predates_start(ms, started_ms) {
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

pub fn plan(identity: &ActorIdentity, ctx: &LaunchCtx) -> Result<Launch, PlanError> {
    let readonly = match identity.permission_profile.as_str() {
        "standard" | "worktree" => false,
        "readonly" => true,
        other => return Err(PlanError::profile(other)),
    };
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
        prep: Some(Prep::Agy(AgyPrep {
            settings,
            hooks: Path::new(ctx.cwd).join(".agents").join("hooks.json"),
            signals: Path::new(ctx.studio_dir)
                .join("roles")
                .join(ctx.role)
                .join("signals.jsonl"),
            trusted_workspace: ctx.answer_trust.then(|| PathBuf::from(ctx.cwd)),
            readonly,
        })),
    })
}
