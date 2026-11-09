//! The `backlot` studio commands. `maybe_run` returns `None` unless the
//! first argument is exactly a studio command word; otherwise it parses,
//! executes and maps the result to an exit code: 0 done, 1 refused,
//! 2 usage error.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use backlot_model::EventBy;
use backlot_model::state::StudioState;

use crate::args::{Cmd, Common, parse};
use crate::commands;
use crate::link;
use crate::render;
use crate::store::{self, StoreError};

/// First argument words handled by the studio layer. Anything else is
/// left for the TUI's own argument parsing.
const COMMANDS: &[&str] = &[
    "studio", "req", "plan", "dispatch", "report", "ask", "tell", "task", "cast", "deliver",
    "block", "review", "correct", "status", "records", "finding", "stage", "priority", "reopen",
    "close", "approve",
];

/// Failure with its exit code: `Usage` → 2, `Refused` → 1.
#[derive(Debug)]
pub enum Fail {
    Usage(String),
    Refused(String),
}

impl Fail {
    pub fn usage(msg: impl Into<String>) -> Self {
        Fail::Usage(msg.into())
    }
    pub fn refused(msg: impl Into<String>) -> Self {
        Fail::Refused(msg.into())
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

/// Studio names are `[a-z0-9_-]{1,16}` (terminal names are
/// `bl-<studio>-<role>`, limit 32). Checked before any path is built
/// so a name can never escape the state directory.
pub fn valid_studio_name(name: &str) -> bool {
    (1..=16).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-'))
}

fn check_studio_name(name: &str) -> Result<String, Fail> {
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
fn studio_name(common: &Common, env: &Env) -> Result<String, Fail> {
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

fn load(dir: &Path) -> Result<StudioState, Fail> {
    store::load_state(dir).map_err(|e| match e {
        StoreError::Io(e) => refused(e),
        StoreError::Corrupt(c) => refused(c),
        _ => refused("unexpected error"),
    })
}

/// The state base for one command: the override, else one connection
/// for the core id (`--session`/`--remote` apply as the TUI's do).
fn base(env: &Env, common: &Common) -> Result<PathBuf, Fail> {
    link::state_dir_sync(
        env,
        common.session.as_deref().map(std::ffi::OsStr::new),
        common.remote.as_deref(),
    )
}

/// Run the command; the string is printed on stdout on success.
pub fn run(args: &[String], env: &Env) -> Result<String, Fail> {
    let (common, cmd) = parse(args)?;
    if let Cmd::Help(u) = cmd {
        return Ok(format!("{u}\n{}", crate::args::COMMON));
    }
    match cmd {
        // The stage connects itself; no ledger command needs it here.
        Cmd::StageStart => return crate::stage::start(&common, env),
        Cmd::StageRun => return crate::stage::run(&common, env),
        _ => {}
    }
    let base = base(env, &common)?;
    match cmd {
        Cmd::Status { json } => {
            let name = studio_name(&common, env)?;
            let dir = store::studio_dir(&base, &name);
            if !dir.join("events.jsonl").exists() {
                return Err(Fail::refused(format!("no studio '{name}'")));
            }
            let state = load(&dir)?;
            Ok(if json {
                serde_json::to_string_pretty(&state).map_err(refused)?
            } else {
                render::status(&name, &state)
            })
        }
        Cmd::Records { json } => {
            let mut logs = Vec::new();
            for name in store::studios(&base).map_err(refused)? {
                let (events, _) =
                    store::load_events(&store::studio_dir(&base, &name)).map_err(|e| {
                        let detail = match e {
                            StoreError::Io(e) => e.to_string(),
                            StoreError::Corrupt(c) => c.to_string(),
                            _ => "unexpected error".to_string(),
                        };
                        Fail::refused(format!("studio '{name}': {detail}"))
                    })?;
                logs.push(backlot_model::StudioLog {
                    studio: name,
                    events,
                });
            }
            let recs = backlot_model::records(&logs);
            Ok(if json {
                serde_json::to_string_pretty(&recs).map_err(refused)?
            } else {
                render::records(&recs)
            })
        }
        _ => exec_event(common, cmd, env, &base),
    }
}

fn exec_event(common: Common, cmd: Cmd, env: &Env, base: &Path) -> Result<String, Fail> {
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
    let dir = store::studio_dir(base, &caller.studio);
    // Only `studio new` may create a studio directory; anything else
    // against a studio with no log is refused without touching disk.
    if !matches!(cmd, Cmd::StudioNew { .. }) && !dir.join("events.jsonl").is_file() {
        return Err(Fail::refused(format!("no studio '{}'", caller.studio)));
    }
    let built = commands::build(&caller, cmd)?;
    let event = store::append(&dir, |state, seq| built(state, seq))?;
    Ok(render::confirm(&event))
}

/// Entry point for `main`: `None` unless argv[1] is a studio command.
pub fn maybe_run(args: &[String]) -> Option<ExitCode> {
    let first = args.first()?;
    if !COMMANDS.contains(&first.as_str()) {
        return None;
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
        Err(Fail::Refused(msg)) => {
            eprintln!("{msg}");
            ExitCode::FAILURE
        }
    })
}
