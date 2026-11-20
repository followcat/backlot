//! The `backlot` studio commands. `maybe_run` returns `None` unless the
//! first argument is exactly a studio command word; otherwise it parses,
//! executes and maps the result to an exit code: 0 done, 1 refused,
//! 2 usage error.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use backlot_model::EventBy;
use backlot_model::state::StudioState;
use hc_proto::client::CallError;

use crate::args::{Cmd, Common};
use crate::commands;
use crate::link;
use crate::render;
use crate::store::{self, StoreError};

/// First argument words handled by the studio layer. Anything else is
/// left for the TUI's own argument parsing.
const COMMANDS: &[&str] = &[
    "studio",
    "req",
    "plan",
    "dispatch",
    "report",
    "ask",
    "tell",
    "task",
    "cast",
    "deliver",
    "block",
    "review",
    "correct",
    "status",
    "records",
    "finding",
    "stage",
    "priority",
    "reopen",
    "close",
    "approve",
    "attention",
    "hook",
    "hollywood",
    "card",
    "runners",
    "repos",
];

/// Failure with its exit code: `Usage` → 2, `Refused`/`Core` → 1.
/// `Core` marks a lost hc connection — inside a stage tick it is the
/// one failure that still ends the stage; everything else is a
/// per-studio error the tick logs and walks past.
#[derive(Debug)]
pub enum Fail {
    Usage(String),
    Refused(String),
    Core(String),
}

impl Fail {
    pub fn usage(msg: impl Into<String>) -> Self {
        Fail::Usage(msg.into())
    }
    pub fn refused(msg: impl Into<String>) -> Self {
        Fail::Refused(msg.into())
    }
    pub fn core(msg: impl Into<String>) -> Self {
        Fail::Core(msg.into())
    }
    /// The message for logging or reporting.
    pub fn message(&self) -> &str {
        match self {
            Fail::Usage(m) | Fail::Refused(m) | Fail::Core(m) => m,
        }
    }
    /// A failed core call into the right failure: a dead wire —
    /// `Closed` or `Protocol` — is `Core`, the one error a stage tick
    /// must not walk past; anything else is one studio's `Refused`,
    /// logged once and retried on a later pass. `Rpc` verdicts are
    /// usually filtered before this and mean `Refused` here too.
    pub fn call(e: CallError) -> Self {
        match e {
            CallError::Closed | CallError::Protocol(_) => {
                Fail::core(format!("core call failed: {e}"))
            }
            _ => Fail::refused(format!("core call failed: {e}")),
        }
    }
}

/// Environment for the command; the real one is `std::env`, tests pass
/// a map (process env is global and racy under parallel tests).
#[derive(Debug, Default)]
pub struct Env {
    pub vars: BTreeMap<String, String>,
}

impl Env {
    pub fn process() -> Self {
        Env {
            vars: std::env::vars().collect(),
        }
    }
    pub fn get(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(String::as_str)
    }
}

/// Studio names are `[a-z0-9_-]{1,16}` starting alphanumeric
/// (terminal names are `bl-<studio>-<role>`, limit 32). Checked
/// before any path is built so a name can never escape the state
/// directory or spell an option downstream (D17a).
pub fn valid_studio_name(name: &str) -> bool {
    (1..=16).contains(&name.len())
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-'))
}

pub(crate) fn check_studio_name(name: &str) -> Result<String, Fail> {
    if valid_studio_name(name) {
        Ok(name.to_string())
    } else {
        Err(Fail::usage(format!(
            "bad studio name '{name}': want [a-z0-9_-]{{1,16}}"
        )))
    }
}

/// The resolved caller: `BACKLOT_STUDIO`, `BACKLOT_ROLE`,
/// `BACKLOT_TASK`, overridable by `--studio`, `--as`, `--task`.
pub struct Caller {
    pub studio: String,
    pub role: String,
    pub task: Option<String>,
    pub session: Option<String>,
}

impl Caller {
    pub fn by(&self) -> EventBy {
        EventBy {
            role: self.role.clone(),
            session: self.session.clone(),
        }
    }
}

/// A pinned identity cannot be claimed away: once `BACKLOT_ROLE`,
/// `BACKLOT_STUDIO` or `BACKLOT_TASK` is set — every role terminal
/// gets its own — the matching `--as`, `--studio` or `--task` flag
/// must be absent or equal. With the variable unset the flag works
/// as it always has: the user's own shell and the panel pass `--as`
/// with no `BACKLOT_ROLE`.
pub(crate) fn check_identity(common: &Common, env: &Env) -> Result<(), Fail> {
    for (flag, arg, var) in [
        ("--as", &common.role, "BACKLOT_ROLE"),
        ("--studio", &common.studio, "BACKLOT_STUDIO"),
        ("--task", &common.task, "BACKLOT_TASK"),
    ] {
        if let (Some(flag_value), Some(pinned)) = (arg.as_deref(), env.get(var))
            && flag_value != pinned
        {
            return Err(Fail::refused(format!(
                "{flag} '{flag_value}' disagrees with {var} '{pinned}'"
            )));
        }
    }
    Ok(())
}

fn caller(common: &Common, env: &Env, need_task: bool) -> Result<Caller, Fail> {
    let studio = check_studio_name(
        &common
            .studio
            .clone()
            .or_else(|| env.get("BACKLOT_STUDIO").map(String::from))
            .ok_or_else(|| Fail::usage("missing studio: set BACKLOT_STUDIO or pass --studio"))?,
    )?;
    let role = common
        .role
        .clone()
        .or_else(|| env.get("BACKLOT_ROLE").map(String::from))
        .ok_or_else(|| Fail::usage("missing role: set BACKLOT_ROLE or pass --as"))?;
    let task = common
        .task
        .clone()
        .or_else(|| env.get("BACKLOT_TASK").map(String::from));
    if need_task && task.is_none() {
        return Err(Fail::usage("missing task: set BACKLOT_TASK or pass --task"));
    }
    Ok(Caller {
        studio,
        role,
        task,
        session: env.get("HC_TERMINAL_ID").map(String::from),
    })
}

/// Studio name only (for `studio new` and `status`).
pub(crate) fn studio_name(common: &Common, env: &Env) -> Result<String, Fail> {
    check_studio_name(
        &common
            .studio
            .clone()
            .or_else(|| env.get("BACKLOT_STUDIO").map(String::from))
            .ok_or_else(|| Fail::usage("missing studio: set BACKLOT_STUDIO or pass --studio"))?,
    )
}

fn refused<E: ToString>(e: E) -> Fail {
    Fail::refused(e.to_string())
}

/// `stage start` and `stage run` are a person's: an actor or writer
/// key never launches or is the stage (S9b).
pub(crate) fn check_stage_caller(common: &Common, env: &Env) -> Result<(), Fail> {
    let role = common
        .role
        .clone()
        .or_else(|| env.get("BACKLOT_ROLE").map(String::from));
    // Hollywood is an overseer, never the stage (D13).
    if let Some(role) = role
        && (role == "writer"
            || role == "hollywood"
            || backlot_model::state::actor_task(&role).is_some())
    {
        return Err(Fail::refused(format!(
            "role '{role}' cannot do this; the user, the producer or the director can"
        )));
    }
    Ok(())
}

impl From<StoreError<Fail>> for Fail {
    fn from(e: StoreError<Fail>) -> Fail {
        match e {
            StoreError::Io(e) => refused(e),
            StoreError::Corrupt(c) => refused(c),
            StoreError::Refused(r) => refused(r),
            StoreError::Build(f) => f,
        }
    }
}

impl From<StoreError<Fail, backlot_model::hollywood::HRefusal>> for Fail {
    fn from(e: StoreError<Fail, backlot_model::hollywood::HRefusal>) -> Fail {
        match e {
            StoreError::Io(e) => refused(e),
            StoreError::Corrupt(c) => refused(c),
            StoreError::Refused(r) => refused(r),
            StoreError::Build(f) => f,
        }
    }
}

fn load(dir: &Path) -> Result<StudioState, Fail> {
    store::load_state(dir).map_err(|e| match e {
        StoreError::Io(e) => refused(e),
        StoreError::Corrupt(c) => refused(c),
        _ => refused("unexpected error"),
    })
}

/// The state base for one command: the override, else one connection
/// for the core id (`--session`/`--remote` apply as the TUI's do).
pub(crate) fn base(env: &Env, common: &Common) -> Result<PathBuf, Fail> {
    link::state_dir_sync(
        env,
        common.session.as_deref().map(std::ffi::OsStr::new),
        common.remote.as_deref(),
    )
}

/// The base the named studio actually lives under: `base` itself when
/// its log sits there, else the one sibling core directory that has
/// it — a studio made under another hc session resolves without the
/// user knowing which core id it was. `BACKLOT_STATE_DIR`, `--session`
/// and `--remote` pin the base and skip the scan; several sibling hits
/// refuse with the core ids so `--session` can disambiguate. `name`
/// must already be a checked studio name.
pub(crate) fn resolve_base(
    env: &Env,
    common: &Common,
    base: &Path,
    name: &str,
) -> Result<PathBuf, Fail> {
    if store::studio_dir(base, name).join("events.jsonl").is_file() {
        return Ok(base.to_path_buf());
    }
    if env.get("BACKLOT_STATE_DIR").is_some() || common.session.is_some() || common.remote.is_some()
    {
        return Err(Fail::refused(format!("no studio '{name}'")));
    }
    let root = store::root_dir(&env.vars);
    let mut cores: Vec<String> = std::fs::read_dir(&root)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|core| {
                    store::studio_dir(&root.join(core), name)
                        .join("events.jsonl")
                        .is_file()
                })
                .collect()
        })
        .unwrap_or_default();
    cores.sort();
    match cores.as_slice() {
        [] => Err(Fail::refused(format!("no studio '{name}'"))),
        [core] => Ok(root.join(core)),
        _ => Err(Fail::refused(format!(
            "studio '{name}' exists under several cores ({}) — pick one with --session",
            cores.join(", ")
        ))),
    }
}

/// Run the command; the string is printed on stdout on success.
pub fn run(args: &[String], env: &Env) -> Result<String, Fail> {
    run_stdin(args, env, None)
}

pub(crate) use crate::cli_run::run_stdin;

pub(crate) fn exec_event(
    common: Common,
    mut cmd: Cmd,
    env: &Env,
    base: &Path,
) -> Result<String, Fail> {
    let needs_task = matches!(
        cmd,
        Cmd::Deliver { .. }
            | Cmd::Block { .. }
            | Cmd::Cast { .. }
            | Cmd::Review { .. }
            | Cmd::Finding { .. }
            | Cmd::TaskState { .. }
            | Cmd::Priority { .. }
    );
    let caller = match &cmd {
        // `studio new` runs before any role terminal exists; the user
        // records it (`--as`/`BACKLOT_ROLE` may still override).
        Cmd::StudioNew { .. } => Caller {
            studio: studio_name(&common, env)?,
            role: common
                .role
                .clone()
                .or_else(|| env.get("BACKLOT_ROLE").map(String::from))
                .unwrap_or_else(|| "user".to_string()),
            task: None,
            session: env.get("HC_TERMINAL_ID").map(String::from),
        },
        _ => caller(&common, env, needs_task)?,
    };
    // Hollywood's one write into a studio is `tell <studio>
    // producer` (R26). `ask` records the identical event, so the
    // command — not the ledger's role gate — is where the rest is
    // refused, ahead of any state read or file copy.
    if caller.role == "hollywood" && !matches!(&cmd, Cmd::Tell { role, .. } if role == "producer") {
        return Err(Fail::refused(
            "role 'hollywood' cannot do this; in a studio it may only 'tell <studio> producer'",
        ));
    }
    // Only `studio new` may create a studio directory; anything else
    // resolves the studio's real base — this core's, or the one
    // sibling core that holds it — and refuses without touching disk.
    let resolved;
    let base = if matches!(cmd, Cmd::StudioNew { .. }) {
        base
    } else {
        resolved = resolve_base(env, &common, base, &caller.studio)?;
        &resolved
    };
    let dir = store::studio_dir(base, &caller.studio);
    // `deliver` checks the caller's right to record before git runs
    // — a refused role hears "cannot do this", not a git error — then
    // the task and cast, then resolves the range in the task's work
    // area. The ledger records the resolved `sha..sha`; the model
    // re-checks the role under the append lock.
    if let Cmd::Deliver { commits, .. } = &mut cmd {
        let task = caller.task.clone().unwrap_or_default();
        let state = load(&dir)?;
        backlot_model::roles::may_deliver(&state, &caller.role, &task).map_err(refused)?;
        let t = state
            .tasks
            .get(&task)
            .ok_or_else(|| Fail::refused(format!("no task '{task}'")))?;
        if t.cast.is_none() {
            return Err(Fail::refused(format!("task '{task}' has no cast")));
        }
        *commits = crate::range::check_range(&dir, &caller.studio, &task, &state, commits)?;
    }
    // `cast` checks each accepted dependency's delivery already sits
    // on the integration branch — a cast before the merge would cut
    // the task's work area without its dependency's code. A working
    // role never reaches git: the append's role gate refuses it.
    if let Cmd::Cast { .. } = &cmd
        && backlot_model::state::actor_task(&caller.role).is_none()
        && caller.role != "writer"
    {
        let task = caller.task.clone().unwrap_or_default();
        let state = load(&dir)?;
        crate::range::check_dependencies(&dir, &caller.studio, &state, &task)?;
    }
    // Hollywood's own ledger keeps its copy of a message it sent a
    // studio's producer (R26) — the `message.queued` just recorded is
    // the delivery record. Captured before `build` consumes the cmd.
    let h_mirror = crate::hwood::mirror_for(&caller, &cmd);
    let built = commands::build(&caller, cmd)?;
    let event = store::append(&dir, |state, seq| built(state, seq))?;
    let mut out = render::confirm(&event);
    if let Some(file) = h_mirror {
        out.push_str(&format!(
            "\n{}",
            crate::hwood::mirror_tell(base, &caller.by(), &caller.studio, &file)?
        ));
    }
    Ok(out)
}

/// Entry point for `main`: `None` unless argv[1] is a studio command.
pub fn maybe_run(args: &[String]) -> Option<ExitCode> {
    let first = args.first()?;
    if !COMMANDS.contains(&first.as_str()) {
        return None;
    }
    // `hook` is the runner hook's own command (S9f): no studio, no
    // ledger, always silent, always 0 — it must never get in its
    // runner's way.
    if first == "hook" {
        return Some(crate::hook::run(&args[1..]));
    }
    let env = Env::process();
    Some(match run(args, &env) {
        Ok(out) => {
            if !out.is_empty() {
                println!("{out}");
            }
            ExitCode::SUCCESS
        }
        Err(Fail::Usage(msg)) => {
            eprintln!("{msg}");
            ExitCode::from(2)
        }
        Err(Fail::Refused(msg) | Fail::Core(msg)) => {
            eprintln!("{msg}");
            ExitCode::FAILURE
        }
    })
}
