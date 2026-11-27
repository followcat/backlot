//! Single-read stdin materialization. A command whose input source is
//! `-` (or a `--stdin` body) reads the process stream once, writes it
//! to a temporary file, and rewrites the command to use that path, so
//! validation, notes and creation never compete for the same stream.

use std::path::PathBuf;

use crate::args::{CastCmd, Cmd, HollywoodCmd, TextSource};
use crate::cli::Fail;

/// Removes every staged file, including a report's private directory.
pub(crate) struct StagedInput {
    paths: Vec<PathBuf>,
}

impl StagedInput {
    fn push(&mut self, path: PathBuf) {
        self.paths.push(path);
    }
}

impl Drop for StagedInput {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = std::fs::remove_file(path);
            if let Some(dir) = path.parent().filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("backlot-report-"))
            }) {
                let _ = std::fs::remove_dir(dir);
            }
        }
    }
}

fn read_stdin_body(stdin: Option<&str>) -> Result<String, Fail> {
    match stdin {
        Some(text) => Ok(text.to_string()),
        None => std::io::read_to_string(std::io::stdin())
            .map_err(|e| Fail::refused(format!("cannot read stdin: {e}"))),
    }
}

fn is_dash(path: &std::path::Path) -> bool {
    path == std::path::Path::new("-")
}

/// Every stdin use in `cmd`, for the ambiguity check. One process
/// stream cannot mean two independent bodies.
fn stdin_uses(cmd: &Cmd) -> Vec<&'static str> {
    let mut uses = Vec::new();
    match cmd {
        Cmd::Deliver { report, .. } if is_dash(report) => uses.push("--report -"),
        Cmd::StudioNew { cast_file, ask, .. } => {
            if cast_file.as_deref().is_some_and(is_dash) {
                uses.push("--cast -");
            }
            match ask {
                Some(TextSource::File(path)) if is_dash(path) => uses.push("--ask-file -"),
                Some(TextSource::Stdin) => uses.push("--stdin"),
                _ => {}
            }
        }
        Cmd::StudioCast(CastCmd::Set { file, .. }) if is_dash(file) => uses.push("--file -"),
        Cmd::Hollywood(HollywoodCmd::Propose { action, .. }) => match action {
            backlot_model::hollywood::CardAction::StudioNew { cast, .. } if cast == "-" => {
                uses.push("--cast -")
            }
            backlot_model::hollywood::CardAction::Cast { cast, .. } if cast == "-" => {
                uses.push("--file -")
            }
            _ => {}
        },
        Cmd::Tell { source, .. }
        | Cmd::ReqRevise { source, .. }
        | Cmd::Progress { source }
        | Cmd::Design(crate::args::DesignCmd::Note { source, .. })
            if matches!(source, TextSource::Stdin) =>
        {
            uses.push("--stdin")
        }
        _ => {}
    }
    uses
}

/// Materialize every body source the command's execution expects as a
/// file. Stdin is read at most once; two independent `-` inputs refuse
/// before either reader sees an empty stream.
pub(crate) fn materialize(cmd: &mut Cmd, stdin: Option<&str>) -> Result<StagedInput, Fail> {
    let uses = stdin_uses(cmd);
    if uses.len() > 1 {
        return Err(Fail::usage(format!(
            "stdin can supply only one of {}",
            uses.join(" and ")
        )));
    }
    let body = if uses.is_empty() {
        None
    } else {
        Some(read_stdin_body(stdin)?)
    };
    let mut staged = StagedInput { paths: Vec::new() };

    if let Cmd::Deliver { report, .. } = cmd
        && is_dash(report)
    {
        let body = body.as_deref().unwrap_or_default();
        if body.trim().is_empty() {
            return Err(Fail::refused("the report body is empty"));
        }
        let path = stage_body("report", body, true)?;
        *report = path.clone();
        staged.push(path);
    }

    match cmd {
        Cmd::StudioNew { cast_file, ask, .. } => {
            if cast_file.as_deref().is_some_and(is_dash) {
                let path = stage_body("cast", body.as_deref().unwrap_or_default(), false)?;
                *cast_file = Some(path.clone());
                staged.push(path);
            }
            if let Some(source) = ask {
                materialize_text_source(source, body.as_deref(), &mut staged, "the ask is empty")?;
            }
        }
        Cmd::StudioCast(CastCmd::Set { file, .. }) if is_dash(file) => {
            let path = stage_body("cast", body.as_deref().unwrap_or_default(), false)?;
            *file = path.clone();
            staged.push(path);
        }
        Cmd::Hollywood(HollywoodCmd::Propose { action, .. }) => match action {
            backlot_model::hollywood::CardAction::StudioNew { cast, .. } if cast == "-" => {
                let path = stage_body("cast", body.as_deref().unwrap_or_default(), false)?;
                *cast = path.to_string_lossy().into_owned();
                staged.push(path);
            }
            backlot_model::hollywood::CardAction::Cast { cast, .. } if cast == "-" => {
                let path = stage_body("cast", body.as_deref().unwrap_or_default(), false)?;
                *cast = path.to_string_lossy().into_owned();
                staged.push(path);
            }
            _ => {}
        },
        Cmd::Tell { source, .. }
        | Cmd::ReqRevise { source, .. }
        | Cmd::Progress { source }
        | Cmd::Design(crate::args::DesignCmd::Note { source, .. }) => {
            materialize_text_source(
                source,
                body.as_deref(),
                &mut staged,
                "the message body is empty",
            )?;
        }
        _ => {}
    }
    Ok(staged)
}

fn materialize_text_source(
    source: &mut TextSource,
    stdin_body: Option<&str>,
    staged: &mut StagedInput,
    empty: &'static str,
) -> Result<(), Fail> {
    let body = match source {
        TextSource::File(path) if !is_dash(path) => return Ok(()),
        TextSource::File(_) => stdin_body.unwrap_or_default().to_string(),
        TextSource::Text(text) => text.clone(),
        TextSource::Stdin => stdin_body.unwrap_or_default().to_string(),
    };
    if body.trim().is_empty() {
        return Err(Fail::refused(empty));
    }
    let path = stage_body("message", &body, false)?;
    *source = TextSource::File(path.clone());
    staged.push(path);
    Ok(())
}

#[cfg(test)]
thread_local! {
    /// Bodies staged on this thread, so a test checks only the files its own runs staged:
    /// other tests stage bodies in the same temp dir at the same time.
    pub(crate) static STAGED_BODIES: std::cell::RefCell<Vec<PathBuf>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

fn stage_body(label: &str, body: &str, nested_report: bool) -> Result<PathBuf, Fail> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = format!(
        "{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let path = if nested_report {
        let dir = std::env::temp_dir().join(format!("backlot-report-{id}"));
        std::fs::create_dir_all(&dir)
            .map_err(|e| Fail::refused(format!("cannot stage the {label} body: {e}")))?;
        dir.join("report.md")
    } else {
        std::env::temp_dir().join(format!("backlot-body-{id}.txt"))
    };
    std::fs::write(&path, body)
        .map_err(|e| Fail::refused(format!("cannot stage the {label} body: {e}")))?;
    #[cfg(test)]
    STAGED_BODIES.with(|s| s.borrow_mut().push(path.clone()));
    Ok(path)
}
