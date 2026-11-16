//! `studio new --interactive` (U2): the same command, but whatever
//! the flags left out is asked for on the terminal — the repository
//! (the current directory when it is a git repository with a
//! commit), the studio id (from the repository's directory name),
//! the title, and the cast, drafted from the runners installed on
//! this machine. The cast is shown and accepted, edited in `$EDITOR`
//! or refused; `studio new`'s own refusals run as the dry run does,
//! then the stage is offered. Nothing is written before the user
//! accepts — a refused cast writes no studio at all.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use crate::args::Common;
use crate::cli::{Env, Fail, check_studio_name};
use crate::commands;
use crate::workareas;

/// Runners the wizard drafts from, best first, with the model the
/// draft assigns — the same check `runners::installed` answers.
/// `cursor`, `devin` and `cmd` carry no effort flag (the cast rule).
const PREFERENCE: &[(&str, &str)] = &[
    ("devin", "swe-2-max"),
    ("claude", "claude-sonnet-4-5"),
    ("codex", "gpt-5.3-codex"),
    ("cursor", "auto"),
    ("opencode", "anthropic/claude-sonnet-4-5"),
    ("cmd", "deepseek/deepseek-v4.1-flash"),
    ("fake", "m"),
];

/// What the wizard still needs after the flags — `None` is asked
/// for. The cast file, when given, skips the draft entirely.
pub(crate) struct WizardArgs {
    pub title: Option<String>,
    pub repo: Option<String>,
    pub cast_file: Option<PathBuf>,
    pub dry_run: bool,
}

/// The `$VISUAL`/`$EDITOR` seam — the edited file stays where it is.
type Edit<'a> = Box<dyn Fn(&Path) -> Result<(), String> + 'a>;

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
    /// `runners::installed(runner, PATH)` — the runner probe.
    pub installed: Box<dyn Fn(&str) -> bool + 'a>,
    /// `$VISUAL`/`$EDITOR <file>` — the edited file stays where it is.
    pub edit: Edit<'a>,
}

/// The real seams: git for the default probe, `runners::installed`
/// for the draft, `$VISUAL`/`$EDITOR` for the edit.
fn real_io<'a>(input: &'a mut dyn BufRead, out: &'a mut dyn Write, env: &'a Env) -> Io<'a> {
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
        installed: Box::new(move |runner| crate::runners::installed(runner, env.get("PATH"))),
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
    let mut io = real_io(&mut input, &mut out, env);
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
            let default = (io.git_repo)(&cwd).then(|| cwd.to_string_lossy().into_owned());
            repo_answer(ask(io, "repository", default.as_deref())?)?
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
    let (cast_file, drafted) = match args.cast_file {
        Some(file) => (file, false),
        None => (cast_draft(io)?, true),
    };
    // Every check `studio new` runs — same messages — before the
    // write is even offered. Nothing exists before this point but
    // the answers and the draft's temporary file.
    let base = crate::cli::base(env, &common)?;
    let written = (|| -> Result<String, Fail> {
        crate::draft::check(&base, &id, &repo, &cast_file, env)?;
        crate::cli::exec_event(
            common.clone(),
            crate::args::Cmd::StudioNew {
                title: Some(title),
                repo: Some(repo),
                cast_file: Some(cast_file.clone()),
                dry_run: false,
                interactive: false,
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
fn ask(io: &mut Io, what: &str, default: Option<&str>) -> Result<String, Fail> {
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

/// The drafted cast: the first installed runner seats every fixed
/// role and both candidates, with its draft model — `cursor`,
/// `devin` and `cmd` take no effort (the cast rule).
fn draft_text(io: &Io) -> Result<String, Fail> {
    let Some((runner, model)) = PREFERENCE.iter().find(|(r, _)| (io.installed)(r)) else {
        return Err(Fail::refused(
            "no supported runner is installed — the wizard cannot draft a cast",
        ));
    };
    let effort = |runner: &str| {
        if matches!(runner, "cursor" | "devin" | "cmd") {
            String::new()
        } else {
            "effort = \"medium\"\n".to_string()
        }
    };
    let role = |runner: &str, model: &str| {
        format!(
            "runner = \"{runner}\"\nmodel = \"{model}\"\n{}\
             permission_profile = \"standard\"",
            effort(runner)
        )
    };
    Ok(format!(
        "# Drafted by `backlot studio new --interactive` — edit or accept as is.\n\
         \n\
         [producer]\n{}\n\n\
         [director]\n{}\n\n\
         [writer]\n{}\n\n\
         [[candidates]]\nname = \"actor-a\"\n{}\n\n\
         [[candidates]]\nname = \"actor-b\"\n{}\n",
        role(runner, model),
        role(runner, model),
        role(runner, model),
        role(runner, model),
        role(runner, model),
    ))
}

/// The draft's file: `backlot-cast-<pid>-<n>.toml` at the first
/// free number, `create_new` so a planted or leaked file is never
/// truncated and a planted symlink never followed — a cast draft is
/// owner-only, mode `0600`.
pub(crate) fn create_draft(dir: &Path, text: &str) -> Result<PathBuf, Fail> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    for n in 0..16u32 {
        let path = dir.join(format!("backlot-cast-{}-{n}.toml", std::process::id()));
        match opts.open(&path) {
            Ok(mut file) => {
                file.write_all(text.as_bytes())
                    .map_err(|e| Fail::refused(format!("draft: {e}")))?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(Fail::refused(format!("draft: {e}"))),
        }
    }
    Err(Fail::refused(
        "draft: no free backlot-cast-* name left in the temp directory",
    ))
}

/// The cast step: draft, show, `accept`/`edit`/`refuse` — the file
/// the checks and the write read is a temporary one; `refuse`
/// writes nothing anywhere.
fn cast_draft(io: &mut Io) -> Result<PathBuf, Fail> {
    let mut text = draft_text(io)?;
    // The file is born only when the draft is accepted or the
    // editor needs it — a refused cast writes nothing anywhere.
    let mut path: Option<PathBuf> = None;
    loop {
        writeln!(io.out, "\n{text}").map_err(io_fail)?;
        match ask(io, "cast [accept/edit/refuse]", Some("a"))?.as_str() {
            a if a.eq_ignore_ascii_case("a") || a.eq_ignore_ascii_case("accept") => {
                return match path {
                    // The editor's file already holds `text`.
                    Some(path) => Ok(path),
                    None => create_draft(&std::env::temp_dir(), &text),
                };
            }
            r if r.eq_ignore_ascii_case("r") || r.eq_ignore_ascii_case("refuse") => {
                return Err(Fail::refused("cast refused — nothing written"));
            }
            _ => {
                if path.is_none() {
                    path = Some(create_draft(&std::env::temp_dir(), &text)?);
                }
                let file = path.as_ref().expect("created above");
                match (io.edit)(file) {
                    Ok(()) => {}
                    Err(e) => {
                        writeln!(io.out, "{e}").map_err(io_fail)?;
                        continue;
                    }
                }
                text = std::fs::read_to_string(file)
                    .map_err(|e| Fail::refused(format!("draft: {e}")))?;
                // An edit that does not parse is shown and asked
                // again — the cast checks are `studio new`'s own.
                if let Err(e) = commands::read_cast(file) {
                    writeln!(io.out, "{}", e.message()).map_err(io_fail)?;
                    continue;
                }
            }
        }
    }
}

fn io_fail(e: std::io::Error) -> Fail {
    Fail::refused(format!("interactive: {e}"))
}
