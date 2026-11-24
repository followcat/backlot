//! Claude Code: the id is a v4 UUID the adapter chooses at launch, so
//! `role.started` carries it. A Claude Code started under another
//! Claude Code inherits markers — `CLAUDE_CODE_CHILD_SESSION` turns
//! off transcript saving and nothing could be read back — so argv is
//! prefixed with `env -u <name>` for each one the stage's own env has.
//! `HC_*` is hc's per-terminal set and stays.

use std::collections::BTreeMap;

use backlot_model::ActorIdentity;
use backlot_model::event::CheckDecl;
use backlot_model::state::actor_task;

use super::{HookSetup, Launch, LaunchCtx, PlanError, Session, uuid_v4};

/// The marker names a parent Claude Code leaves in its
/// environment — exactly these: a `CLAUDE_CODE_*` prefix would also
/// cover settings a user sets on purpose, like
/// `CLAUDE_CODE_OAUTH_TOKEN`, and those stay.
const MARKERS: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_EFFORT",
    "CLAUDE_PID",
    "AI_AGENT",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_SESSION_ID",
];

fn is_marker(name: &str) -> bool {
    MARKERS.contains(&name)
}

/// The models `claude --model` is documented to take: the aliases
/// its `--help` names (`fable`, `opus`, `sonnet` — `haiku` is the
/// same family) plus the full ids `docs/studio/guide.md` casts.
/// A newer id still goes in as free text.
pub const MODELS: &[&str] = &[
    "sonnet",
    "opus",
    "haiku",
    "fable",
    "claude-sonnet-5-5",
    "claude-haiku-5-5",
];

/// `--permission-mode`'s value for the profile.
fn permission_mode(profile: &str) -> Result<&'static str, PlanError> {
    match profile {
        "standard" => Ok("manual"),
        "worktree" => Ok("acceptEdits"),
        other => Err(PlanError::profile(other)),
    }
}

/// The commands a Hollywood launch pre-allows — one list, so the
/// generated CLI reference and the launch argv can never disagree.
pub fn hollywood_commands() -> &'static [&'static str] {
    // D17a: exactly the overseer's set — anything else stops at the
    // prompt, which is the user's to answer, never the stage's.
    &[
        "backlot status",
        "backlot records",
        "backlot tell producer",
        "backlot hollywood propose",
        "backlot hollywood report",
        "backlot studio new --dry-run",
        "backlot --help",
        "backlot help",
    ]
}

/// The per-session `--allowedTools` value: `backlot` commands and
/// reads under the studio's state directory, where the prompts,
/// messages and requirement files live — plus, per role, the
/// commands its job needs. An actor gets the six git commands its
/// loop drives, bare and with arguments, and every declared check's
/// `run` verbatim; the director reads what it reviews, merges and
/// runs the same checks — never `git branch`, which would also allow
/// the deleting `git branch -D`.
/// Nothing is written to settings: the rules live only in this
/// argv. `//` is Claude Code's rule syntax for an absolute path; a
/// single leading `/` would anchor at the working directory and
/// match nothing.
pub(crate) fn allowed_tools(studio_dir: &str, role: &str, checks: &[CheckDecl]) -> String {
    let dir = studio_dir.strip_prefix('/').unwrap_or(studio_dir);
    // One command in both forms Claude's rule syntax takes: bare and
    // with arguments.
    let command = |rules: &mut Vec<String>, cmd: &str| {
        rules.push(format!("Bash({cmd})"));
        rules.push(format!("Bash({cmd} *)"));
    };
    if role == "hollywood" {
        let mut rules = vec![format!("Read(//{dir}/**)")];
        for cmd in hollywood_commands() {
            command(&mut rules, cmd);
        }
        return rules.join(",");
    }
    let mut rules = vec!["Bash(backlot *)".to_string(), format!("Read(//{dir}/**)")];
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
            command(&mut rules, cmd);
        }
    }
    if role == "director" {
        for cmd in ["git status", "git diff", "git log", "git show", "git merge"] {
            command(&mut rules, cmd);
        }
    }
    if is_actor || role == "director" {
        for check in checks {
            // `(` and `,` are the rule syntax's own — a `run`
            // holding either is never pre-allowed (the cast still
            // accepted it; `studio new` refuses the words that would
            // change who the command runs as).
            if !check.run.contains(['(', ')', ',']) {
                rules.push(format!("Bash({})", check.run));
            }
        }
    }
    rules.join(",")
}

/// POSIX single-quoting for the settings file's command line: `'` is
/// written `'\''`. A studio directory can hold spaces or quotes.
pub(crate) fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The per-session Claude settings file (S9f, R7b): `hooks` and a
/// `permissions.deny` entry blocking `AskUserQuestion` — no env —
/// pointing every hook event at
/// `backlot hook --file <role's signals.jsonl>` with a five second
/// timeout. `--settings <file>` is one session's extra layer, so
/// the user's own hooks keep running and no user settings file is
/// touched. The command is the running binary's absolute path —
/// both paths shell-quoted.
pub(crate) fn hook_setup(ctx: &LaunchCtx) -> HookSetup {
    let role_dir = std::path::Path::new(ctx.studio_dir)
        .join("roles")
        .join(ctx.role);
    let settings = role_dir.join("claude-settings.json");
    let signals = role_dir.join("signals.jsonl");
    let exe = std::env::current_exe()
        .map(|p| live_exe(p, |q| q.exists()).to_string_lossy().into_owned())
        .unwrap_or_else(|_| "backlot".into());
    let command = format!(
        "{} hook --file {}",
        sh_quote(&exe),
        sh_quote(&signals.to_string_lossy())
    );
    let hook = serde_json::json!({"type": "command", "command": command, "timeout": 5});
    // Every event the caps name — a `command` hook running the
    // binary's own `hook` subcommand, no matcher so all of them fire
    // (S9f).
    let events: serde_json::Map<String, serde_json::Value> = backlot_model::caps::CLAUDE
        .hooks
        .iter()
        .map(|e| {
            (
                (*e).to_string(),
                serde_json::json!([{"hooks": [hook.clone()]}]),
            )
        })
        .collect();
    let content = serde_json::to_string_pretty(&serde_json::json!({
        "permissions": {"deny": ["AskUserQuestion"]},
        "hooks": events,
    }))
    .unwrap_or_default()
        + "\n";
    HookSetup {
        settings,
        signals,
        content,
        command,
    }
}

/// `current_exe` names the file the process was exec'd from — a
/// binary replaced on disk while it runs (a rebuild, an upgrade)
/// reads `<path> (deleted)` on Linux, and a hook command built on
/// that points nowhere: the role's hooks go quiet and the screen
/// takes over. The name without the marker is the replacement —
/// use it when it exists.
pub(crate) fn live_exe(
    path: std::path::PathBuf,
    exists: impl Fn(&std::path::Path) -> bool,
) -> std::path::PathBuf {
    const DELETED: &str = " (deleted)";
    let Some(s) = path.to_str() else {
        return path;
    };
    let Some(base) = s.strip_suffix(DELETED) else {
        return path;
    };
    let stripped = std::path::PathBuf::from(base);
    if exists(&stripped) { stripped } else { path }
}

pub fn plan(identity: &ActorIdentity, ctx: &LaunchCtx) -> Result<Launch, PlanError> {
    let mode = permission_mode(&identity.permission_profile)?;
    let mut argv: Vec<String> = Vec::new();
    let markers: Vec<&String> = ctx.stage_env.keys().filter(|k| is_marker(k)).collect();
    if !markers.is_empty() {
        argv.push("env".into());
        for name in markers {
            argv.push("-u".into());
            argv.push(name.clone());
        }
    }
    argv.push("claude".into());
    let session = match ctx.resume {
        Some(id) => {
            argv.extend(["-r".into(), id.to_string()]);
            id.to_string()
        }
        None => uuid_v4(),
    };
    // The `--allowedTools` list is variadic, so the pair goes before
    // `--model`: a `--flag` must follow the value or the positional
    // prompt would be swallowed as a tool name.
    let hooks = hook_setup(ctx);
    argv.extend([
        "--allowedTools".into(),
        allowed_tools(ctx.studio_dir, ctx.role, ctx.checks),
        "--settings".into(),
        hooks.settings.to_string_lossy().into_owned(),
    ]);
    argv.extend([
        "--model".into(),
        identity.model.clone(),
        "--effort".into(),
        identity.effort.clone().unwrap_or_default(),
        "--permission-mode".into(),
        mode.into(),
    ]);
    if ctx.resume.is_none() {
        argv.extend(["--session-id".into(), session.clone()]);
    }
    if let Some(p) = ctx.prompt {
        argv.push(p.into());
    }
    Ok(Launch {
        argv,
        env: BTreeMap::new(),
        session: Session::Chosen(session),
        hooks: Some(hooks),
        prep: None,
    })
}
