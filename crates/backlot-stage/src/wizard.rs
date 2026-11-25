//! `studio new --interactive` (U2): the same command, but whatever
//! the flags left out is asked for on the terminal — the repository
//! (the current directory when it is a git repository with a
//! commit), the studio id (from the repository's directory name),
//! the title, and the cast, drafted from the runners installed on
//! this machine. The cast is shown and accepted, edited in `$EDITOR`
//! or refused; `studio new`'s own refusals run as the dry run does,
//! then the stage is offered. Nothing is written before the user
//! accepts — a refused cast writes no studio at all.

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use crate::args::Common;
use crate::cli::{Env, Fail, check_studio_name};
use crate::runners::detect::{self, Found};
use crate::workareas;

/// What the wizard still needs after the flags — `None` is asked
/// for. The cast file, when given, skips the draft entirely; a
/// `--mode` given on the command line skips the mode question.
pub(crate) struct WizardArgs {
    pub title: Option<String>,
    pub repo: Option<String>,
    pub cast_file: Option<PathBuf>,
    pub dry_run: bool,
    pub mode: Option<backlot_model::event::StudioMode>,
}

/// The `$VISUAL`/`$EDITOR` seam — the edited file stays where it is.
type Edit<'a> = Box<dyn Fn(&Path) -> Result<(), String> + 'a>;

/// The terminal picker's seam — the current directory in, the picked
/// repository's absolute path out; `Err` the picker's refusal.
type PickRepo<'a> = Box<dyn FnMut(&Path) -> Result<PathBuf, Fail> + 'a>;

/// The runner detection seam (U5) — `detect::find`'s verdict for one
/// runner name, `None` when nothing was found anywhere.
type Detect<'a> = Box<dyn Fn(&str) -> Option<Found> + 'a>;

/// Everything `run` does behind injected seams, so tests script the
/// prompts and never spawn git, an editor or a runner probe.
pub(crate) struct Io<'a> {
    /// The prompt's input lines.
    pub input: &'a mut dyn BufRead,
    /// Prompts, the shown cast and the result line.
    pub out: &'a mut dyn Write,
    /// `<dir>` is a git repository with at least one commit —
    /// `git -C <dir> rev-parse --verify HEAD` live.
    pub git_repo: Box<dyn Fn(&Path) -> bool + 'a>,
    /// `runners::installed(runner, PATH)` widened by detection —
    /// a runner found anywhere counts, since the wizard writes the
    /// PATH its launch needs (U5).
    pub installed: Box<dyn Fn(&str) -> bool + 'a>,
    /// `detect::find` — the draft's runners block and the
    /// `runner_env` PATH write's verdict per runner.
    pub detect: Detect<'a>,
    /// `$VISUAL`/`$EDITOR <file>` — the edited file stays where it is.
    pub edit: Edit<'a>,
    /// The terminal repository picker — `Some` only when the
    /// wizard's stdin and stdout are both a terminal (U5). The
    /// repository question goes through it instead of `ask`; it
    /// answers the picked absolute path, `Err` the picker's own
    /// refusal — "cancelled" for Esc/Ctrl+C.
    pub pick_repo: Option<PickRepo<'a>>,
}

/// The "repositories in use" group for the picker (U5): each live
/// terminal's cwd walked up to its repository root, deduplicated.
/// The core unreachable, `terminal.list` failing — the group is
/// simply absent, never an error.
pub(crate) fn in_use(env: &Env, common: &Common) -> Vec<PathBuf> {
    let Ok(rt) = crate::link::runtime() else {
        return Vec::new();
    };
    let fetch = async {
        let link = crate::link::connect(
            env,
            common.session.as_deref().map(std::ffi::OsStr::new),
            common.remote.as_deref(),
        )
        .await
        .ok()?;
        let list = link
            .client
            .terminal_list(hc_proto::terminal::ListParams::default())
            .await
            .ok()?;
        // The state base exactly as the stage resolves it: the
        // environment when set, else this core's directory, made real
        // so a terminal's cwd compares against the same path.
        let base = match env.get("BACKLOT_STATE_DIR") {
            Some(dir) => PathBuf::from(dir),
            None => crate::server_lock::effective_base(&env.vars, &link.core_id),
        };
        Some((list, crate::daemon::resolved_base(base)))
    };
    let Some((list, state_dir)) = rt.block_on(fetch) else {
        return Vec::new();
    };
    crate::wizard_picker::in_use::collect(
        list.terminals
            .into_iter()
            .map(|t| PathBuf::from(t.cwd))
            .collect(),
        &state_dir,
        &crate::paths::is_git,
        &crate::wizard_picker::fs::branch,
    )
}

/// The real seams: git for the default probe, `runners::installed`
/// for the draft, `$VISUAL`/`$EDITOR` for the edit.
fn real_io<'a>(
    input: &'a mut dyn BufRead,
    out: &'a mut dyn Write,
    env: &'a Env,
    common: &Common,
) -> Io<'a> {
    let pick_env = Env {
        vars: env.vars.clone(),
    };
    let pick_common = common.clone();
    let detect_scope = detect::scope(env);
    let installed_scope = detect::scope(env);
    Io {
        input,
        out,
        git_repo: Box::new(|dir| {
            workareas::real_git(
                &[
                    "-C",
                    &dir.to_string_lossy(),
                    "rev-parse",
                    "--verify",
                    "HEAD",
                ],
                ".",
            )
            .code
                == 0
        }),
        installed: Box::new(move |runner| {
            crate::runners::installed(runner, env.get("PATH"))
                || detect::find(runner, &installed_scope).is_some()
        }),
        detect: Box::new(move |runner| detect::find(runner, &detect_scope)),
        pick_repo: (std::io::stdin().is_terminal() && std::io::stdout().is_terminal()).then(
            move || -> PickRepo {
                Box::new(move |start| {
                    let in_use = in_use(&pick_env, &pick_common);
                    match crate::wizard_picker::pick(start, in_use) {
                        Ok(Some(path)) => Ok(path),
                        Ok(None) => Err(Fail::refused("cancelled")),
                        Err(e) => Err(Fail::refused(format!("picker: {e}"))),
                    }
                })
            },
        ),
        edit: Box::new(|file| {
            let editor = std::env::var("VISUAL")
                .ok()
                .filter(|e| !e.is_empty())
                .or_else(|| std::env::var("EDITOR").ok().filter(|e| !e.is_empty()))
                .ok_or_else(|| "neither $VISUAL nor $EDITOR is set".to_string())?;
            std::process::Command::new("sh")
                .arg("-c")
                .arg(format!("{editor} \"$1\""))
                .arg("editor")
                .arg(file)
                .status()
                .map_err(|e| format!("{editor}: {e}"))
                .and_then(|s| {
                    s.success()
                        .then_some(())
                        .ok_or_else(|| format!("{editor} exited {s}"))
                })
        }),
    }
}

/// `backlot studio new --interactive` — real stdio and probes.
pub(crate) fn run(common: Common, args: WizardArgs, env: &Env) -> Result<String, Fail> {
    let mut input = std::io::stdin().lock();
    let mut out = std::io::stdout().lock();
    let mut io = real_io(&mut input, &mut out, env, &common);
    run_io(common, args, env, &mut io)
}

/// The prompts, the checks and the write — every external reach is
/// an `Io` seam so a test drives it with lines. `git` and the
/// runner probes stay real inside `draft::check` — the checks are
/// `studio new`'s own, and tests hand it real repositories like the
/// dry-run tests do.
pub(crate) fn run_io(
    mut common: Common,
    args: WizardArgs,
    env: &Env,
    io: &mut Io,
) -> Result<String, Fail> {
    if args.dry_run {
        return Err(Fail::usage(
            "--dry-run and --interactive do not combine: \
             the wizard already runs every check before it writes",
        ));
    }
    // The repository first — the studio id's default is its name.
    let repo = match args.repo {
        Some(repo) => repo,
        None => {
            let cwd = std::env::current_dir().map_err(|e| Fail::refused(e.to_string()))?;
            match io.pick_repo.as_mut() {
                // On a terminal the picker answers the question —
                // the line prompt stays for scripts and pipes.
                Some(pick) => repo_answer(pick(&cwd)?.to_string_lossy().into_owned())?,
                None => {
                    let default = (io.git_repo)(&cwd).then(|| cwd.to_string_lossy().into_owned());
                    repo_answer(ask(io, "repository", default.as_deref())?)?
                }
            }
        }
    };
    let id = match common.studio.clone() {
        Some(id) => {
            check_studio_name(&id)?;
            id
        }
        None => {
            let default = default_id(&repo);
            loop {
                let answer = ask(io, "studio", default.as_deref())?;
                match check_studio_name(&answer) {
                    Ok(id) => break id,
                    Err(e) => writeln!(io.out, "{}", e.message()).map_err(io_fail)?,
                }
            }
        }
    };
    common.studio = Some(id.clone());
    let title = match args.title {
        Some(title) => title,
        None => ask(io, "title", Some(&id))?,
    };
    // R14: `--mode` answers ahead; otherwise the wizard asks —
    // `standard` on a bare Enter, anything else re-asked.
    let mode = match args.mode {
        Some(mode) => mode,
        None => loop {
            match ask(io, "mode [standard|experimental]", Some("standard"))?.as_str() {
                "standard" => break backlot_model::event::StudioMode::Standard,
                "experimental" => break backlot_model::event::StudioMode::Experimental,
                other => {
                    writeln!(io.out, "bad mode '{other}': want standard|experimental")
                        .map_err(io_fail)?;
                }
            }
        },
    };
    let (cast_file, drafted) = match args.cast_file {
        Some(file) => (file, false),
        None => (crate::wizard_cast::cast_draft(io, env, mode)?, true),
    };
    // Every check `studio new` runs — same messages — before the
    // write is even offered. Nothing exists before this point but
    // the answers and the draft's temporary file.
    let base = crate::cli::base(env, &common)?;
    // U5: the accepted draft's runners that only detection could
    // reach get their `[runner_env]` PATH before the draft check —
    // the check counts it as installed like a launch would.
    if drafted {
        crate::wizard_paths::runner_paths(io, &base, &cast_file, env)?;
    }
    // D4: a plain directory defaults to `git init` in the
    // interactive flow — ask before the launch writes it; the
    // explicit no-git mode only exists on the launch plan's final
    // check (the TUI's step 4), so a no names the flags instead.
    let init = matches!(
        crate::paths::classify(std::path::Path::new(&repo)),
        crate::paths::PathShape::Plain
    ) && {
        let yes = ask(io, &format!("git init in {repo}? [Y/n]"), Some("y"))?;
        if !yes.eq_ignore_ascii_case("y") {
            if drafted {
                let _ = std::fs::remove_file(&cast_file);
            }
            return Err(Fail::refused(
                "a plain directory needs --init or --no-git — \
                 rerun `studio new` with one",
            ));
        }
        true
    };
    let written = (|| -> Result<String, Fail> {
        crate::draft::check(&base, &id, &repo, &cast_file, env, init, false)?;
        crate::cli::exec_event(
            common.clone(),
            crate::args::Cmd::StudioNew {
                title: Some(title),
                repo: Some(repo),
                cast_file: Some(cast_file.clone()),
                dry_run: false,
                interactive: false,
                mode: Some(mode),
                init,
                no_git: false,
                ask: None,
            },
            env,
            &base,
        )
    })();
    if drafted {
        // The ledger keeps the cast's own copy of the event; the
        // draft's temporary file is done either way.
        let _ = std::fs::remove_file(&cast_file);
    }
    let out = written?;
    let mut lines = out;
    // The stage offer: `y` starts it like `backlot stage start` —
    // `check_stage_caller` does not apply: the user is the caller.
    if ask(io, "start the stage now? [Y/n]", Some("y"))?.eq_ignore_ascii_case("y") {
        let started = crate::stage::start(&common, env)?;
        if !started.is_empty() {
            lines.push_str(&format!("\n{started}"));
        }
    }
    Ok(lines)
}

/// `name [default]: ` on `out`, the trimmed answer back — the
/// default on an empty line, re-asked while empty with no default;
/// EOF refuses.
pub(crate) fn ask(io: &mut Io, what: &str, default: Option<&str>) -> Result<String, Fail> {
    loop {
        match default {
            Some(d) => write!(io.out, "{what} [{d}]: ").map_err(io_fail)?,
            None => write!(io.out, "{what}: ").map_err(io_fail)?,
        }
        io.out.flush().map_err(io_fail)?;
        let mut line = String::new();
        if io.input.read_line(&mut line).map_err(io_fail)? == 0 {
            return Err(Fail::refused("interactive input ended"));
        }
        let answer = line.trim();
        if !answer.is_empty() {
            return Ok(answer.to_string());
        }
        if let Some(d) = default {
            return Ok(d.to_string());
        }
    }
}

/// The repository answer: absolute like `--repo`, checked against
/// `studio new`'s own `repo_path` so both refuse the same way.
fn repo_answer(answer: String) -> Result<String, Fail> {
    crate::args_studio::repo_path(&answer)
}

/// `acme-shop` — the repository's directory name squeezed into a
/// valid studio id; `None` when nothing valid comes out.
fn default_id(repo: &str) -> Option<String> {
    let base = repo.trim_end_matches('/').rsplit('/').next()?;
    let id: String = base
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let id = id.trim_matches('-');
    crate::cli::valid_studio_name(id).then(|| id.to_string())
}

pub(crate) fn io_fail(e: std::io::Error) -> Fail {
    Fail::refused(format!("interactive: {e}"))
}
