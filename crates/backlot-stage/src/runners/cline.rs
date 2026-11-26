//! Cline CLI 3.0.74 (S14a), measured 2026-11-17 on the ClinePass
//! provider. `cline -i` runs its session in a shared hub daemon by
//! default, and the hub's environment is what a role's commands get —
//! `BACKLOT_*` and `PATH` would never reach the actor, so identity
//! pinning breaks. `--yolo` is the one switch that picks the local
//! backend, and every argv the adapter builds carries it. `--yolo`
//! also forces act mode and auto-approve on; the permission layer is
//! then the stage-written `<cwd>/.cline/hooks/PreToolUse.sh`, not a
//! prompt ([`super::cline_hook`]).
//!
//! The id is cline's own: a fresh launch's manifest lands under
//! `~/.cline/data/sessions/`, found after launch; `--id <id>` resumes
//! one and ignores a positional prompt, so a resume's prompt goes in
//! after the composer opens (the devin and cmd rule). Login, settings
//! and the session store are the user's — there is no per-role tree,
//! and the launch flags may rewrite `providers.json`'s per-provider
//! `settings.model`/`settings.reasoning` (the cursor `--model`
//! class of side effect, documented not restored).

use std::path::{Path, PathBuf};

use backlot_model::ActorIdentity;
use backlot_model::event::CheckDecl;
use backlot_model::state::actor_task;

use super::cline_hook::{self, Spec};
use super::splice::write600;
use super::{Launch, LaunchCtx, PlanError, Prep, Session, predates_start};

/// The attention reason a failed `prepare` raises.
pub const CLINE_PREP_FAILED: &str = "cline prep failed";
/// The model names a provider the adapter does not know.
pub const MODEL_NOT_SUPPORTED: &str = "model not supported";
/// The cast effort is not one cline's `--thinking` takes.
pub const EFFORT_NOT_SUPPORTED: &str = "effort not supported";

/// The `cline-pass/<model>` ids the ClinePass provider listed for
/// the measured account — the only ones [`provider`] accepts; any
/// other provider's id is a plan refusal, not a catalog entry.
/// New ClinePass models still go in as free text.
pub const MODELS: &[&str] = &[
    "cline-pass/deepseek-v4.1-flash-1017",
    "cline-pass/deepseek-v4.1-flash",
    "cline-pass/deepseek-v4-pro",
    "cline-pass/glm-5.3",
    "cline-pass/glm-5.3-flash",
    "cline-pass/kimi-k3",
    "cline-pass/kimi-k3.1-pro",
    "cline-pass/mimo-v2.6-flash",
    "cline-pass/mimo-v2.6-pro",
    "cline-pass/minimax-m3",
    "cline-pass/minimax-m3.1-flash",
    "cline-pass/muse-spark-1.3-contributor",
    "cline-pass/qwen3.7-max",
    "cline-pass/qwen3.7-plus",
    "cline-pass/qwen3.8-max",
];

/// The provider a model id names, when the adapter knows one: cline
/// takes `-P <provider>` beside `-m <model>`, and only the
/// `cline-pass/<model>` ids were measured. Any other model would
/// need an explicit provider, and the only per-runner config channel
/// (`[runner_env]` in `config.toml`) carries environment, not argv —
/// so such a model is a refusal, not a guess.
pub fn provider(model: &str) -> Option<&'static str> {
    let (head, rest) = model.split_once('/')?;
    (head == "cline-pass" && !rest.is_empty()).then_some("cline-pass")
}

/// The `--thinking` value for a cast effort: one of the runner's caps'
/// levels, or nothing when the cast declares none. Anything else is a
/// refusal naming the effort — the flag would be rejected by cline, or
/// worse, read as something else.
pub fn thinking(effort: Option<&str>) -> Result<Option<&'static str>, PlanError> {
    let Some(effort) = effort else {
        return Ok(None);
    };
    let levels = backlot_model::caps::CLINE.effort.levels();
    levels
        .iter()
        .find(|e| **e == effort)
        .copied()
        .map(Some)
        .ok_or_else(|| PlanError {
            reason: EFFORT_NOT_SUPPORTED,
            detail: format!(
                "effort '{effort}' is not one cline's --thinking takes ({})",
                levels.join(", ")
            ),
        })
}

/// `~/.cline/data/sessions` — cline's store: one directory per
/// session holding `<id>.json` (the manifest) and
/// `<id>.messages.json` (the whole conversation, which the stage
/// never reads).
pub fn sessions_root(home: &Path) -> PathBuf {
    home.join(".cline").join("data").join("sessions")
}

/// A process's parent pid, read from `/proc` — Linux only, `None`
/// elsewhere. Cline's manifest names the `.cline` engine's pid; the
/// terminal's pid is the `node` wrapper that started it, so the pid
/// test in [`session_at`] compares a manifest pid's parent with the
/// terminal's.
pub fn parent_pid(pid: u32) -> Option<u32> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // `pid (comm) state ppid …` — comm may hold spaces and brackets,
    // so the fields are counted from the last `)`.
    let rest = text.rsplit_once(')')?.1;
    rest.split_whitespace().nth(1)?.parse().ok()
}

/// A file's birth time, or its mtime where birth time is
/// unsupported — a manifest is written when its session starts.
fn file_time_ms(path: &Path) -> Option<u64> {
    let md = std::fs::metadata(path).ok()?;
    let t = md.created().or_else(|_| md.modified()).ok()?;
    Some(
        t.duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    )
}

/// The session a fresh launch created (S14a section 3): a manifest
/// `<id>/<id>.json` under `sessions_root` whose `cwd` (or
/// `workspace_root`) is the role's, whose file is at or after the
/// launch (up to [`super::BIRTH_SLACK_MS`] before it), whose
/// `source` is `cli` when it names one, and whose `pid`
/// belongs to the role's own `.cline` process — its parent is the
/// terminal's pid. Dirs with no manifest, corrupt JSON and ids
/// another role claims are skipped; the newest qualified one wins.
///
/// `owner_pid` is the terminal's own pid and `parent_of` the pid
/// lookup, injected so a fixture can name a process tree. Without an
/// owner pid there is nothing to compare, and a manifest whose parent
/// is unknown (`None`) is not rejected on a guess.
pub fn session_at(
    sessions_root: &Path,
    cwd: &str,
    started_ms: u64,
    taken: &std::collections::BTreeSet<String>,
    owner_pid: Option<u32>,
    parent_of: &dyn Fn(u32) -> Option<u32>,
) -> Option<String> {
    let entries = std::fs::read_dir(sessions_root).ok()?;
    let mut best: Option<(u64, String)> = None;
    for entry in entries.filter_map(|e| e.ok()) {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let Some(id) = dir.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if taken.contains(id) {
            continue;
        }
        let manifest = dir.join(format!("{id}.json"));
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let manifest_cwd = v["cwd"].as_str().or_else(|| v["workspace_root"].as_str());
        if manifest_cwd != Some(cwd) {
            continue;
        }
        if matches!(v["session_id"].as_str(), Some(sid) if sid != id) {
            continue;
        }
        if matches!(v["source"].as_str(), Some(src) if src != "cli") {
            continue;
        }
        let Some(pid) = v["pid"].as_u64() else {
            continue;
        };
        if let Some(owner) = owner_pid
            && matches!(parent_of(pid as u32), Some(parent) if parent != owner)
        {
            continue;
        }
        let Some(ms) = file_time_ms(&manifest) else {
            continue;
        };
        if predates_start(ms, started_ms) {
            continue;
        }
        if best.as_ref().is_none_or(|(b, _)| ms > *b) {
            best = Some((ms, id.to_string()));
        }
    }
    best.map(|(_, id)| id)
}

/// The per-launch filesystem work: the permission hook in the work
/// area, the role's MCP mask and its signals file.
#[derive(Debug, Clone, PartialEq)]
pub struct ClinePrep {
    /// `<cwd>/.cline/hooks/PreToolUse.sh` — written mode 0755.
    pub hook: PathBuf,
    /// The hook's bytes, generated from the role's rules.
    pub hook_content: String,
    /// `roles/<role>/cline-mcp.json` — `CLINE_MCP_SETTINGS_PATH`'s
    /// value.
    pub mcp: PathBuf,
    /// `roles/<role>/cline-signals.jsonl` — one line per decision.
    pub signals: PathBuf,
    /// The role's work area: the only folder an edit may land in, and
    /// the folder whose `info/exclude` gains `/.cline/`.
    pub cwd: PathBuf,
}

/// The commands a role may run: `backlot` for everyone, plus the
/// shared read-only set ([`super::READ_ONLY`]); an actor adds
/// `git add`/`git commit`, the director `git merge`; and both, each
/// declared check's `run` verbatim. Only `git branch`'s two listing
/// forms are allowed, never the bare family. A rule a single-quoted
/// shell string cannot hold — one with a quote or a newline — is
/// dropped rather than mangled.
pub fn allow_commands(role: &str, checks: &[CheckDecl]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let add = |out: &mut Vec<String>, c: &str| {
        if !c.is_empty() && !c.contains(['\'', '\n']) {
            out.push(c.to_string());
        }
    };
    if role == "hollywood" {
        // D17a: exactly the overseer's set.
        for cmd in [
            "backlot status",
            "backlot records",
            "backlot tell producer",
            "backlot progress",
            "backlot hollywood propose",
            "backlot hollywood report",
            "backlot studio new --dry-run",
            // Read-only, compiled into the binary: the docs, no disk
            // search.
            "backlot guide",
        ] {
            add(&mut out, cmd);
        }
        return out;
    }
    add(&mut out, "backlot");
    for cmd in super::READ_ONLY {
        add(&mut out, cmd);
    }
    let is_actor = actor_task(role).is_some();
    if is_actor {
        for cmd in ["git add", "git commit"] {
            add(&mut out, cmd);
        }
    }
    if role == "director" {
        add(&mut out, "git merge");
    }
    if is_actor || role == "director" {
        for check in checks {
            add(&mut out, &check.run);
        }
    }
    out
}

/// Write the hook, the MCP mask and the role's signals file, and keep
/// `/.cline/` out of the worktree's git status — before
/// `terminal.run`, so a fresh session's first tool call already goes
/// through the hook. A failure is the caller's attention: without the
/// hook the role would run every tool unguarded. Never an
/// informational `true`.
pub(crate) fn prepare(prep: &ClinePrep) -> Result<bool, String> {
    let io = |what: &'static str| move |e: std::io::Error| format!("{what}: {e}");
    let cline = prep.cwd.join(".cline");
    std::fs::create_dir_all(cline.join("hooks")).map_err(io(".cline dirs"))?;
    write755(&prep.hook, prep.hook_content.as_bytes()).map_err(io("PreToolUse.sh"))?;
    if let Some(parent) = prep.mcp.parent() {
        std::fs::create_dir_all(parent).map_err(io("cline-mcp.json"))?;
    }
    write600(&prep.mcp, cline_hook::MCP_MASK.as_bytes()).map_err(io("cline-mcp.json"))?;
    if let Some(parent) = prep.signals.parent() {
        std::fs::create_dir_all(parent).map_err(io("cline-signals.jsonl"))?;
    }
    std::fs::write(&prep.signals, "").map_err(io("cline-signals.jsonl"))?;
    if let Some(exclude) = super::hook_files::exclude_path(&prep.cwd) {
        super::hook_files::exclude_add(&exclude, "/.cline/")?;
    }
    Ok(false)
}

/// An executable stage-written file, mode 0755 — cline runs the hook
/// itself, so the write and the mode are one step.
#[cfg(unix)]
fn write755(path: &Path, content: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, content)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn write755(path: &Path, content: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, content)
}

pub fn plan(identity: &ActorIdentity, ctx: &LaunchCtx) -> Result<Launch, PlanError> {
    match identity.permission_profile.as_str() {
        "standard" | "worktree" => {}
        other => return Err(PlanError::profile(other)),
    }
    let provider = provider(&identity.model).ok_or_else(|| PlanError {
        reason: MODEL_NOT_SUPPORTED,
        detail: format!(
            "model '{}' names no provider — cline needs a 'cline-pass/<model>' id",
            identity.model
        ),
    })?;
    let thinking = thinking(identity.effort.as_deref())?;
    let mut argv: Vec<String> = vec!["cline".into(), "-i".into(), "--yolo".into()];
    // The measured resume shape names the id right after `--yolo`
    // (S14a section 3).
    if let Some(id) = ctx.resume {
        argv.extend(["--id".into(), id.to_string()]);
    }
    argv.extend([
        "-P".into(),
        provider.into(),
        "-m".into(),
        identity.model.clone(),
    ]);
    if let Some(effort) = thinking {
        argv.extend(["--thinking".into(), effort.into()]);
    }
    // A positional prompt is the first message on a fresh launch; a
    // resume ignores it, so the stage sends the text after the
    // composer opens instead.
    if ctx.resume.is_none()
        && let Some(p) = ctx.prompt
    {
        argv.push(p.into());
    }
    // The role's own files: the permission hook in the work area and
    // the MCP mask beside the studio's role directory.
    let role_dir = Path::new(ctx.studio_dir).join("roles").join(ctx.role);
    let signals = role_dir.join("cline-signals.jsonl");
    let mcp = role_dir.join("cline-mcp.json");
    let hook = cline_hook::hook_path(Path::new(ctx.cwd));
    let hook_content = cline_hook::script(&Spec {
        worktree: ctx.cwd,
        signals: &signals,
        allowed: &allow_commands(ctx.role, ctx.checks),
        role: ctx.role,
    });
    Ok(Launch {
        argv,
        env: std::collections::BTreeMap::from([(
            "CLINE_MCP_SETTINGS_PATH".into(),
            mcp.to_string_lossy().into_owned(),
        )]),
        session: Session::PostLaunch,
        hooks: None,
        prep: Some(Prep::Cline(ClinePrep {
            hook,
            hook_content,
            mcp,
            signals,
            cwd: Path::new(ctx.cwd).to_path_buf(),
        })),
    })
}
