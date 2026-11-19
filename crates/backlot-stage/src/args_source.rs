//! G2's `--file`/`--text`/`--stdin` body source: the `TextSource`
//! the `Cmd` variants carry, and the `Source` collector the `tell`
//! and `req revise` parsers share.

use std::path::PathBuf;

use crate::args::{It, eq, usage};
use crate::cli::Fail;

/// A `--file`/`--text`/`--stdin` body source (G2): `tell` and
/// `req revise` take exactly one. `Text` and `Stdin` materialize to
/// a temp file in `cli::run` before the command builds, so
/// `commands` only ever sees `File`.
#[derive(Debug)]
pub enum TextSource {
    File(PathBuf),
    Text(String),
    Stdin,
}

/// The `--file`/`--text`/`--stdin` collector `tell` and `req
/// revise` share (G2). `flag` eats a source flag; `one` enforces
/// exactly one source.
#[derive(Default)]
pub(crate) struct Source {
    pub(crate) file: Option<PathBuf>,
    pub(crate) text: Option<String>,
    pub(crate) stdin: bool,
}

impl Source {
    /// `true` when `arg` was a source flag.
    pub(crate) fn flag(&mut self, it: &mut It, arg: &str, u: &'static str) -> Result<bool, Fail> {
        match arg {
            "--file" => self.file = Some(PathBuf::from(it.value("--file", u)?)),
            _ if eq(arg, "--file").is_some() => self.file = eq(arg, "--file").map(PathBuf::from),
            "--text" => self.text = Some(it.value("--text", u)?),
            _ if eq(arg, "--text").is_some() => self.text = eq(arg, "--text").map(String::from),
            "--stdin" => self.stdin = true,
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Exactly one source; an empty `--text` is refused now, an
    /// empty stdin when `cli::run` reads it.
    pub(crate) fn one(self, u: &'static str) -> Result<TextSource, Fail> {
        let n = self.file.is_some() as usize + self.text.is_some() as usize + self.stdin as usize;
        if n > 1 {
            return Err(usage("pick exactly one of --file, --text, --stdin", u));
        }
        if let Some(text) = &self.text
            && text.trim().is_empty()
        {
            return Err(usage("--text must not be empty", u));
        }
        match self {
            Self { file: Some(f), .. } => Ok(TextSource::File(f)),
            Self { text: Some(t), .. } => Ok(TextSource::Text(t)),
            Self { stdin: true, .. } => Ok(TextSource::Stdin),
            _ => Err(usage(
                "missing body source: --file <f>, --text <msg> or --stdin",
                u,
            )),
        }
    }
}
