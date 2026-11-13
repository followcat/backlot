//! Command Code (`cmd`): the runner mints the session id — the
//! stage finds it under `~/.commandcode/projects/<slug(cwd)>/` after
//! launch and records `role.session` (S12a). The launch always
//! passes `--trust` (it only initializes the project's slug
//! directory — a non-empty cwd without it deadlocks on the trust
//! dialog the stage never answers), `--skip-onboarding`,
//! `--no-auto-update` and `-m <model>`; a fresh launch also names
//! the session after its terminal (`-n`), which stays a resumable
//! name for `-r`. The first prompt is positional on a fresh launch
//! only — a resume ignores it, so the stage sends the prompt after
//! the composer opens, the same rule devin follows. Effort is never
//! carried: `--effort` writes the global config's
//! `reasoningEffort.<model>` — measured — so the adapter emits no
//! flag at all.
//!
//! Isolation reality: `~/.commandcode` hangs off `HOME` only — no
//! per-role config or auth tree exists. The role boundary is the
//! workdir's own `.commandcode/settings.json`, written before every
//! start and resume with the role's `permissions.allow` and a
//! `deny` that masks the user's global MCP servers — the project
//! file is read once at launch. `NODE_USE_ENV_PROXY=1` rides in the
//! env: without it the model traffic bypasses the company proxy
//! (measured).

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use backlot_model::ActorIdentity;
use backlot_model::event::CheckDecl;
use backlot_model::state::actor_task;

use super::splice::{splice_permissions, write600};
use super::{Launch, LaunchCtx, PlanError, Prep, Session};

/// The attention reason a failed `prepare` raises.
pub const CMD_PREP_FAILED: &str = "cmd prep failed";

/// The per-launch filesystem work: the workdir's settings file and
/// where the user's MCP servers are listed.
#[derive(Debug, Clone, PartialEq)]
pub struct CmdPrep {
    /// `<cwd>/.commandcode/settings.json`.
    pub settings: PathBuf,
    /// The role's `permissions.allow` rules.
    pub allow: Vec<String>,
    /// `<HOME>/.commandcode/mcp.json` — the server names are read,
    /// never the values.
    pub global_mcp: PathBuf,
}

/// Command Code's project slug for a workdir: lowercase, camel
/// boundaries split (`AControl` → `a-control`, `xControl` →
/// `x-control`), non-alphanumeric runs collapsed to one `-`, ends
/// trimmed; empty slugs name `root` — the directory under
/// `~/.commandcode/projects/` a cwd's sessions land in (S12a).
pub fn slug(cwd: &str) -> String {
    let chars: Vec<char> = cwd.chars().collect();
    let mut out = String::with_capacity(cwd.len() + 4);
    for (i, c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            let p = chars[i - 1];
            let boundary = p.is_ascii_lowercase()
                || p.is_ascii_digit()
                || (p.is_ascii_uppercase()
                    && chars.get(i + 1).is_some_and(|n| n.is_ascii_lowercase()));
            if boundary {
                out.push('-');
            }
        }
        out.push(c.to_ascii_lowercase());
    }
    let mut slug = String::with_capacity(out.len());
    for c in out.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() { "root".into() } else { slug }
}

/// A `permissions.allow` entry: `Shell(<spec>:*)` rules in the
/// prefix form the bundle itself writes — `Shell(backlot)` covers
/// `backlot status`, `Shell(git add:*)` reaches `git add -A` — and
/// never a bare command family: `Shell(git:*)` would also allow
/// `git branch -D`, so it is never emitted (S12a).
pub(crate) fn allow_rules(role: &str, checks: &[CheckDecl]) -> Vec<String> {
    let mut rules = vec!["Shell(backlot)".to_string()];
    let is_actor = actor_task(role).is_some();
    if is_actor {
        for cmd in [
            "git add",
            "git commit",
            "git status",
            "git diff",
            "git log",
            "git show",
        ] {
            rules.push(format!("Shell({cmd}:*)"));
        }
    }
    if role == "director" {
        for cmd in ["git status", "git diff", "git log", "git show", "git merge"] {
            rules.push(format!("Shell({cmd}:*)"));
        }
    }
    if is_actor || role == "director" {
        for check in checks {
            if expressible(&check.run) {
                rules.push(format!("Shell({}:*)", check.run));
            }
        }
    }
    rules
}

/// Whether a declared check's `run` can be a `Shell(<run>:*)` rule:
/// `(` `)` are the rule syntax's own, `\` its escape, `*?[]{}` glob
/// characters the matcher would widen, `:` the command/argument
/// split the rule's own `:*` needs to stay unique, and `,` is
/// filtered like cursor — anything holding one prompts at run time
/// instead. A chained `run` is expressible but never matches —
/// cmd resolves each subcommand against the rules (S12a) — so the
/// doc asks for plain single commands.
fn expressible(run: &str) -> bool {
    !run.contains(['(', ')', ',', '*', '?', '[', ']', '{', '}', ':', '\\'])
}

/// The `deny` rules masking the user's global MCP servers:
/// `mcp__<name>` per server the global `mcp.json` declares — only
/// the names are read, never the values (S12a: a project `mcp.json`
/// cannot disable a user server; the permissions deny is the mask).
/// Names sort so the file is stable.
pub(crate) fn mcp_denies(global_mcp: &Path) -> Result<Vec<String>, String> {
    let text = match std::fs::read_to_string(global_mcp) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {e}", global_mcp.display())),
    };
    let v: serde_json::Value = serde_json::from_str(&text)
        .map_err(|_| format!("{} did not parse", global_mcp.display()))?;
    let mut names: Vec<String> = v["mcpServers"]
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    names.sort();
    Ok(names.iter().map(|n| format!("mcp__{n}")).collect())
}

/// The settings file's bytes: `{"permissions":{"allow":..,"deny":..}}`
/// written fresh, or `permissions` spliced byte-exact into an
/// existing object — every other member (the role's own settings)
/// keeps its whitespace and order. `allow` and `deny` entries are
/// serialized by serde — a rule carrying a quote or backslash never
/// reaches the file (`allow_rules` drops those), and deny names
/// come from the JSON server map.
pub(crate) fn settings_content(
    existing: Option<&[u8]>,
    allow: &[String],
    deny: &[String],
) -> Result<Vec<u8>, String> {
    let permissions = serde_json::to_string(&serde_json::json!({
        "allow": allow,
        "deny": deny,
    }))
    .unwrap_or_default();
    let fresh = || Ok(format!("{{\"permissions\":{permissions}}}\n").into_bytes());
    let Some(text) = existing.and_then(|b| std::str::from_utf8(b).ok()) else {
        return fresh();
    };
    match splice_permissions(text.trim(), &permissions) {
        Some(spliced) => Ok(format!("{spliced}\n").into_bytes()),
        // A file that is not a JSON object is healed, not preserved.
        None => fresh(),
    }
}

/// Write `<cwd>/.commandcode/settings.json` before the launch —
/// cmd reads the project file once at startup, so the rules must be
/// in place before `terminal.run` and a failure is an attention,
/// never a half-allowed role. The file lands mode 600 like every
/// stage-written file; always `Ok(false)` — cmd has no open-dir
/// drift to report (the workdir itself is the boundary).
pub fn prepare(p: &CmdPrep) -> Result<bool, String> {
    let dir = p.settings.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    if p.global_mcp.as_os_str().is_empty() {
        return Err("no HOME in the stage's environment — the global mcp.json is unknown".into());
    }
    let deny = mcp_denies(&p.global_mcp)?;
    let existing = std::fs::read(&p.settings).ok();
    let bytes = settings_content(existing.as_deref(), &p.allow, &deny)?;
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

/// The session id a live cmd role's launch minted: the newest
/// `<uuid>.jsonl` under `~/.commandcode/projects/<slug(cwd)>/`
/// created at or after the role's start whose first line —
/// `{"type":"session","id":..,"cwd":..}` — names the role's own
/// cwd, and no other role already claims. `.checkpoints.jsonl`
/// companions and a corrupt first line are skipped.
pub fn session_at(
    projects_root: &Path,
    cwd: &str,
    started_ms: u64,
    taken: &BTreeSet<String>,
) -> Option<String> {
    let dir = projects_root.join(slug(cwd));
    let entries = std::fs::read_dir(dir).ok()?;
    let mut best: Option<(u64, String)> = None;
    for e in entries.filter_map(|e| e.ok()) {
        let path = e.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.ends_with(".jsonl") || name.ends_with(".checkpoints.jsonl") {
            continue;
        }
        let Some(ms) = file_time_ms(&path) else {
            continue;
        };
        if ms < started_ms {
            continue;
        }
        let Ok(file) = std::fs::File::open(&path) else {
            continue;
        };
        let mut first = String::new();
        if BufReader::new(file).read_line(&mut first).is_err() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&first) else {
            continue;
        };
        let (Some(id), Some(file_cwd)) = (v["id"].as_str(), v["cwd"].as_str()) else {
            continue;
        };
        if file_cwd != cwd || taken.contains(id) {
            continue;
        }
        if best.as_ref().is_none_or(|(b, _)| ms > *b) {
            best = Some((ms, id.to_string()));
        }
    }
    best.map(|(_, id)| id)
}

/// The session id a live cmd terminal's argv carries: the value
/// after `-r`/`--resume` (an id or a `-n` name — both resume) or
/// `--session` — how an adopted resumed role recovers its id.
pub fn session_in_argv(argv: &[String]) -> Option<String> {
    let flag = argv
        .iter()
        .position(|a| a == "-r" || a == "--resume" || a == "--session")?;
    argv.get(flag + 1).cloned()
}

pub fn plan(identity: &ActorIdentity, ctx: &LaunchCtx) -> Result<Launch, PlanError> {
    match identity.permission_profile.as_str() {
        "standard" | "worktree" => {}
        other => return Err(PlanError::profile(other)),
    }
    let mut argv = vec!["cmd".to_string()];
    match ctx.resume {
        // `-r` takes the recorded id or the `-n` name; the
        // positional prompt is ignored on resume — the stage sends
        // it after the composer opens instead.
        Some(id) => argv.extend(["-r".into(), id.to_string()]),
        None => argv.extend(["-n".into(), ctx.terminal.to_string()]),
    }
    argv.extend([
        "-m".into(),
        identity.model.clone(),
        "--trust".into(),
        "--skip-onboarding".into(),
        "--no-auto-update".into(),
    ]);
    if ctx.resume.is_none()
        && let Some(p) = ctx.prompt
    {
        argv.push(p.into());
    }
    let mut env = BTreeMap::new();
    env.insert("NODE_USE_ENV_PROXY".into(), "1".into());
    let global_mcp = ctx
        .stage_env
        .get("HOME")
        .map(|h| Path::new(h).join(".commandcode").join("mcp.json"))
        .unwrap_or_default();
    Ok(Launch {
        argv,
        env,
        session: Session::PostLaunch,
        hooks: None,
        prep: Some(Prep::Cmd(CmdPrep {
            settings: Path::new(ctx.cwd).join(".commandcode/settings.json"),
            allow: allow_rules(ctx.role, ctx.checks),
            global_mcp,
        })),
    })
}
