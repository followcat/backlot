//! cursor-agent 2026.11.05, measured in S11a. The runner assigns
//! the chat id, so the session is found after launch. Every role
//! runs isolated: `CURSOR_CONFIG_DIR`, `CURSOR_DATA_DIR` and
//! `CURSOR_SANDBOX_POLICY_DIR` point under
//! `roles/<role>/cursor/` in the studio state directory, so a role's
//! config, chats, trust and sandbox stay out of `~/.cursor` — only
//! the global `mcp.json` is still read (hard-coded path; every server
//! it names is disabled per role) and a daily empty
//! `.agent-data-cleanup-<date>` marker lands in its `projects/`.
//! `auth.json` — the login — does not
//! move with `CURSOR_CONFIG_DIR`, it reads `XDG_CONFIG_HOME`; a
//! stage env carrying it would log the role out, so that env is a
//! plan refusal. `XDG_CONFIG_HOME` itself is never set.
//!
//! The per-session files are written before every start and resume
//! ([`prepare`], driven from `Launch::prep`): the role's
//! `cli-config.json` `permissions` — `Shell(...)` rules, the
//! narrowest form the bundle's matcher takes — and
//! `data/projects/<slug>/mcp-disabled.json`, because the global
//! `~/.cursor/mcp.json` is read from a hard-coded path and holds
//! paid tools a role must never see. Directories are mode 700 and
//! every file the stage writes is mode 600 — the agent writes its
//! account `authInfo` into `cli-config.json`, so the studio state
//! directory is private and nothing read back from that file is
//! ever logged.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use backlot_model::ActorIdentity;
use backlot_model::event::CheckDecl;
use backlot_model::state::actor_task;

use super::{Launch, LaunchCtx, PlanError, Prep, Session};

/// The stage env carries `XDG_CONFIG_HOME`: `auth.json` moves with
/// it, so the role would start logged out.
pub const XDG_CONFIG_HOME_SET: &str = "inherited XDG_CONFIG_HOME";
/// `prepare` could not do its work — the role does not start.
pub const CURSOR_PREP_FAILED: &str = "cursor prep failed";

/// The role's own cursor tree: `<studio>/roles/<role>/cursor`.
pub fn role_root(studio_dir: &str, role: &str) -> PathBuf {
    Path::new(studio_dir)
        .join("roles")
        .join(role)
        .join("cursor")
}

/// `<config>/chats/` — where the role's chat dirs live.
pub fn chats_root(role_root: &Path) -> PathBuf {
    role_root.join("config").join("chats")
}

/// cursor's project slug from `cursor-config/dist/paths.js` →
/// `workspace-paths.js` `r_`: non-alphanumeric runs collapse to one
/// `-`, leading and trailing `-` trimmed. The 83-char + sha256 cut
/// is `kL`, a different dir (worker state), not `projects/<slug>`.
pub fn slug(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut dash = true; // leading dashes are skipped, never emitted
    for c in path.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
}

/// The `permissions.allow` rules the role's `cli-config.json`
/// carries — `Shell(<cmd>)` matches the bare command and the
/// command with arguments (the bundle's matcher accepts
/// `cmd == pat || cmd starts with pat + ' ' || base matches`), so
/// one rule per command is the narrow form; unlike claude there is
/// no starred variant. The base is `backlot`; an actor gets the git
/// commands its loop drives, the director `git merge`, and both get
/// every declared check's `run`. A `run` holding `()` `,` `*?[]{}:`
/// or `\` is not expressible — `,` splits rules, the rest are glob
/// or syntax characters — and is simply never pre-allowed.
pub fn allow_rules(role: &str, checks: &[CheckDecl]) -> Vec<String> {
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
            rules.push(format!("Shell({cmd})"));
        }
    }
    if role == "director" {
        rules.push("Shell(git merge)".into());
    }
    if is_actor || role == "director" {
        for check in checks {
            if expressible(&check.run) {
                rules.push(format!("Shell({})", check.run));
            }
        }
    }
    rules
}

/// Whether `run` survives a `Shell(<run>)` round trip: `(` `)` and
/// `,` are the rule syntax's own, `*?[]{}` are glob characters the
/// matcher would expand, `:` splits the command from an argument
/// pattern, and `\` is the escape — any of them would make the
/// written rule match more or less than the command.
fn expressible(run: &str) -> bool {
    !run.contains(['(', ')', ',', '*', '?', '[', ']', '{', '}', ':', '\\'])
}

/// What [`super::prepare`] writes for one cursor launch — computed
/// in `plan`, where paths are known, executed at launch time.
#[derive(Debug, Clone, PartialEq)]
pub struct CursorPrep {
    /// `roles/<role>/cursor` — `config/`, `data/` and `sandbox/`
    /// are created under it, mode 700.
    pub root: PathBuf,
    /// The workdir's `slug` — `data/projects/<slug>/`.
    pub project: String,
    /// The `Shell(...)` rules for `config/cli-config.json`.
    pub allow: Vec<String>,
    /// The global `~/.cursor/mcp.json` — read for server names only.
    pub global_mcp: PathBuf,
}

/// The env the terminal gets — the three isolated roots. Never
/// `XDG_CONFIG_HOME`.
fn env(root: &Path) -> BTreeMap<String, String> {
    let v = |p: PathBuf| p.to_string_lossy().into_owned();
    BTreeMap::from([
        ("CURSOR_CONFIG_DIR".into(), v(root.join("config"))),
        ("CURSOR_DATA_DIR".into(), v(root.join("data"))),
        ("CURSOR_SANDBOX_POLICY_DIR".into(), v(root.join("sandbox"))),
    ])
}

pub fn plan(identity: &ActorIdentity, ctx: &LaunchCtx) -> Result<Launch, PlanError> {
    match identity.permission_profile.as_str() {
        "standard" | "worktree" => {}
        other => return Err(PlanError::profile(other)),
    }
    // An empty value is the XDG convention for unset — auth stays at
    // the default `~/.config`; only a real redirect moves it.
    if ctx
        .stage_env
        .get("XDG_CONFIG_HOME")
        .is_some_and(|v| !v.is_empty())
    {
        return Err(PlanError {
            reason: XDG_CONFIG_HOME_SET,
            detail: "the stage's environment carries XDG_CONFIG_HOME; cursor's auth.json \
                     moves with it and the role would start logged out — unset it in the \
                     stage's environment"
                .into(),
        });
    }
    let mut argv = vec!["cursor-agent".into()];
    if let Some(id) = ctx.resume {
        argv.extend(["--resume".into(), id.to_string()]);
    }
    argv.extend(["--trust".into(), "--model".into(), identity.model.clone()]);
    if let Some(p) = ctx.prompt {
        argv.push(p.into());
    }
    let root = role_root(ctx.studio_dir, ctx.role);
    let global_mcp = ctx
        .stage_env
        .get("HOME")
        .map(|h| Path::new(h).join(".cursor").join("mcp.json"))
        .unwrap_or_default();
    Ok(Launch {
        argv,
        env: env(&root),
        session: Session::PostLaunch,
        hooks: None,
        prep: Some(Prep::Cursor(CursorPrep {
            project: slug(ctx.cwd),
            root,
            allow: allow_rules(ctx.role, ctx.checks),
            global_mcp,
        })),
    })
}

/// Create `dir` mode 700 — existing dirs are chmodded back, so a
/// drifted mode heals on the next launch.
#[cfg(unix)]
fn dir700(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn dir700(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}

/// A stage-written file, mode 600 — `cli-config.json` will hold the
/// account's `authInfo` once the agent has run.
#[cfg(unix)]
fn write600(path: &Path, content: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, content)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn write600(path: &Path, content: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, content)
}

/// The `permissions` value the file carries — the role's `allow`
/// rules and the mandatory empty `deny` (the schema requires both).
fn permissions_value(allow: &[String]) -> serde_json::Value {
    serde_json::json!({"allow": allow, "deny": []})
}

/// The file's content when none exists yet (or one exists that is
/// not a JSON object — a corrupt file is healed, not preserved):
/// the three required keys, matching what cursor writes itself.
fn minimal_config(allow: &[String]) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "version": 1,
        "editor": {"vimMode": false},
        "permissions": permissions_value(allow),
    }))
    .unwrap_or_default()
        + "\n"
}

/// A JSON string literal in `text` starting at `at` (which must be
/// the opening quote): its end offset, or `None` when unterminated.
fn string_end(text: &str, at: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = at + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

/// The end offset of the JSON value starting at `at` — object,
/// array, string, or a scalar literal; nesting and strings handled.
/// A scalar runs to the next `,` `}` `]` or the text's end.
fn value_end(text: &str, at: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut i = at;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => i = string_end(text, i)?,
            b'{' | b'[' => {
                depth += 1;
                i += 1;
            }
            b'}' | b']' => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
                i += 1;
            }
            b',' if depth == 0 => return Some(i),
            _ => i += 1,
        }
        if depth == 0 && i > at && matches!(bytes[at], b'{' | b'[' | b'"') {
            // A delimited value ended at its own close.
            return Some(i);
        }
    }
    if depth == 0 && i > at { Some(i) } else { None }
}

/// `text`'s top-level `"permissions"` value replaced by `value`,
/// spliced byte-exact so every other key — `authInfo` included,
/// never parsed here — keeps its bytes. When the object has no
/// `permissions` key the member is appended; a text that is not a
/// JSON object at all yields `None` and the caller writes fresh.
fn splice_permissions(text: &str, value: &str) -> Option<String> {
    let bytes = text.as_bytes();
    if bytes.first() != Some(&b'{') {
        return None;
    }
    let mut members: Vec<(usize, usize, usize)> = Vec::new(); // (key start, value start, value end)
    let mut i = 1;
    let mut close = None;
    while i < bytes.len() {
        i += text[i..]
            .bytes()
            .take_while(|b| b.is_ascii_whitespace() || *b == b',')
            .count();
        match bytes.get(i) {
            Some(&b'}') => {
                close = Some(i);
                break;
            }
            Some(&b'"') => {
                let key_start = i;
                i = string_end(text, i)?;
                i += text[i..]
                    .bytes()
                    .take_while(|b| b.is_ascii_whitespace())
                    .count();
                if bytes.get(i) != Some(&b':') {
                    return None;
                }
                i += 1;
                i += text[i..]
                    .bytes()
                    .take_while(|b| b.is_ascii_whitespace())
                    .count();
                let vstart = i;
                i = value_end(text, i)?;
                members.push((key_start, vstart, i));
            }
            _ => return None,
        }
    }
    let close = close?;
    for (ks, vs, ve) in &members {
        if text[*ks..string_end(text, *ks)?] == *"\"permissions\"" {
            return Some(format!("{}{}{}", &text[..*vs], value, &text[*ve..]));
        }
    }
    // No permissions key: append before the closing brace.
    let insertion = if members.is_empty() {
        format!("\n  \"permissions\": {value}\n")
    } else {
        format!(",\n  \"permissions\": {value}\n")
    };
    Some(format!("{}{}{}", &text[..close], insertion, &text[close..]))
}

/// The JSON content `config/cli-config.json` holds after prepare.
pub(crate) fn config_content(existing: Option<&[u8]>, allow: &[String]) -> Vec<u8> {
    let value = serde_json::to_string(&permissions_value(allow)).unwrap_or_default();
    if let Some(text) = existing.and_then(|b| std::str::from_utf8(b).ok()) {
        let text = text.trim();
        if let Some(spliced) = splice_permissions(text, &value) {
            return format!("{spliced}\n").into_bytes();
        }
    }
    minimal_config(allow).into_bytes()
}

/// The server names the global `mcp.json` declares — keys of
/// `mcpServers` only, the values are never looked at.
fn mcp_server_names(global_mcp: &Path) -> Result<Vec<String>, String> {
    let text = match std::fs::read_to_string(global_mcp) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {e}", global_mcp.display())),
    };
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", global_mcp.display()))?;
    Ok(v.get("mcpServers")
        .and_then(|m| m.as_object())
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default())
}

/// `prepare`'s work for one launch: the isolated tree, the
/// permissions file and the MCP-off list. Any failure is the
/// caller's attention — a role never starts half-configured. The
/// existing `cli-config.json` is spliced by byte range; its other
/// keys' contents — `authInfo` above all — are never parsed into
/// the process, logged or returned in an error.
pub(crate) fn prepare(prep: &CursorPrep) -> Result<(), String> {
    let io = |what: &'static str| move |e: std::io::Error| format!("{what}: {e}");
    let config_dir = prep.root.join("config");
    let project_dir = prep.root.join("data").join("projects").join(&prep.project);
    for dir in [&config_dir, &project_dir, &prep.root.join("sandbox")] {
        dir700(dir).map_err(io("cursor dirs"))?;
    }
    let cli_config = config_dir.join("cli-config.json");
    let existing = std::fs::read(&cli_config).ok();
    write600(
        &cli_config,
        &config_content(existing.as_deref(), &prep.allow),
    )
    .map_err(io("cli-config.json"))?;
    if prep.global_mcp.as_os_str().is_empty() {
        return Err("no HOME in the stage's environment — the global mcp.json is unknown".into());
    }
    let names = mcp_server_names(&prep.global_mcp)?;
    let disabled = serde_json::to_string(&names).unwrap_or_default() + "\n";
    write600(&project_dir.join("mcp-disabled.json"), disabled.as_bytes())
        .map_err(io("mcp-disabled.json"))?;
    Ok(())
}

/// The newest chat a fresh launch could have created: a uuid dir
/// under `chats/` whose `meta.json`'s `createdAtMs` is at or after
/// `started_ms`, whose `cwd` is the role's and whose id is not
/// taken. `meta.json` is the authority for both timestamps — dir
/// mtimes move as the chat grows.
pub(crate) fn session_at(
    chats: &Path,
    cwd: &str,
    started_ms: u64,
    taken: &BTreeSet<String>,
) -> Option<String> {
    let mut best: Option<(u64, String)> = None;
    for hash_dir in fs_dirs(chats) {
        for chat in fs_dirs(&hash_dir) {
            let Some(id) = chat.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !is_uuid(id) || taken.contains(id) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(chat.join("meta.json")) else {
                continue;
            };
            let Ok(meta) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            let (Some(at), Some(chat_cwd)) = (meta["createdAtMs"].as_u64(), meta["cwd"].as_str())
            else {
                continue;
            };
            if at < started_ms || chat_cwd != cwd {
                continue;
            }
            if best.as_ref().is_none_or(|(b, _)| at > *b) {
                best = Some((at, id.to_string()));
            }
        }
    }
    best.map(|(_, id)| id)
}

/// The subdirectories of `dir`, in no guaranteed order.
fn fs_dirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect()
}

/// A `<uuid>` chat dir name: 8-4-4-4-12 lowercase-or-uppercase hex.
fn is_uuid(name: &str) -> bool {
    name.len() == 36
        && name.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}
