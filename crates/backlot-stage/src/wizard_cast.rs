//! The wizard's cast step: the runners block, the drafted cast
//! file, and the show/`accept`/`edit`/`refuse` loop — everything
//! behind the same `Io` seams `wizard::run_io` drives.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::cli::{Env, Fail};
use crate::commands;
use crate::runners::detect;
use crate::wizard::{Io, ask, io_fail};

/// Runners the wizard drafts from, best first, with the model the
/// draft assigns — the same check `runners::installed` answers.
/// `cursor`, `devin` and `cmd` carry no effort flag (the cast rule).
/// `claude` gets the CLI's own `sonnet` alias so the draft tracks
/// the current sonnet instead of going stale; `gpt-5.3-codex` is the
/// codex id `docs/studio/` documents, and opencode takes the same
/// sonnet through its `provider/model` config.
const PREFERENCE: &[(&str, &str)] = &[
    ("devin", "swe-2-max"),
    ("claude", "sonnet"),
    ("codex", "gpt-5.3-codex"),
    ("cursor", "auto"),
    ("opencode", "anthropic/claude-sonnet-5-5"),
    ("cmd", "deepseek/deepseek-v4.1-flash"),
    ("cline", "cline-pass/deepseek-v4.1-flash-1017"),
    ("fake", "m"),
];

/// The model the draft assigns a runner — `runners --json`'s
/// `models` list source, so the table and the dialog agree.
pub(crate) fn draft_model(runner: &str) -> Option<&'static str> {
    PREFERENCE
        .iter()
        .find(|(r, _)| *r == runner)
        .map(|(_, m)| *m)
}

/// The drafted cast: the first installed runner seats every fixed
/// role and both candidates, with its draft model — `cursor`,
/// `devin` and `cmd` take no effort (the cast rule). An experimental
/// studio's draft shows `mode = "experimental"` up top; a standard
/// draft writes no mode line at all.
fn draft_text(io: &Io, mode: backlot_model::event::StudioMode) -> Result<String, Fail> {
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
    let mode_line = match mode {
        backlot_model::event::StudioMode::Experimental => "mode = \"experimental\"\n\n",
        backlot_model::event::StudioMode::Standard => "",
    };
    Ok(format!(
        "# Drafted by `backlot studio new --interactive` — edit or accept as is.\n\
         \n\
         {mode_line}\
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

/// The draft's runners block (U5): every runner the preference
/// list could seat, with where detection found it and its version —
/// `needs PATH` marks a find the inherited PATH cannot reach, which
/// the accepted cast's `runner_env` write then covers. `fake` has
/// no row: it has no binary anywhere.
fn runner_lines(io: &mut Io, env: &Env) -> Vec<String> {
    let home = env.get("HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
    PREFERENCE
        .iter()
        .filter(|(r, _)| *r != "fake")
        .map(|(r, _)| match (io.detect)(r) {
            Some(f) => {
                let dir = detect::tilde(home.as_deref(), &f.bin_dir());
                let needs = if f.needs_path() { "  needs PATH" } else { "" };
                match f.version {
                    Some(v) => format!("  {r:<9} {v}  ({dir}){needs}"),
                    None => format!("  {r:<9} not working  ({dir}){needs}"),
                }
            }
            None => format!("  {r:<9} not installed"),
        })
        .collect()
}

/// The cast step: draft, show, `accept`/`edit`/`refuse` — the file
/// the checks and the write read is a temporary one; `refuse`
/// writes nothing anywhere.
pub(crate) fn cast_draft(
    io: &mut Io,
    env: &Env,
    mode: backlot_model::event::StudioMode,
) -> Result<PathBuf, Fail> {
    for line in runner_lines(io, env) {
        writeln!(io.out, "{line}").map_err(io_fail)?;
    }
    let mut text = draft_text(io, mode)?;
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
