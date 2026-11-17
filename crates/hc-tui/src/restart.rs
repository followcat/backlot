//! `↵ restart`: `exec` the installed binary with the arguments this
//! process started with. The caller runs it only after the run loop has
//! unwound and the terminal guard has restored the terminal.

use std::ffi::OsString;
use std::io;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

/// Replaces this process with `program`; returns only when `exec` fails.
pub fn exec_installed(program: &Path, argv: impl IntoIterator<Item = OsString>) -> io::Error {
    restart_with(program, argv, |program, argv| {
        Command::new(program).args(argv).exec()
    })
}

/// `exec_installed` with the `exec` call injected, so tests observe the
/// path and the arguments without replacing the test process.
pub fn restart_with(
    program: &Path,
    argv: impl IntoIterator<Item = OsString>,
    exec: impl FnOnce(&Path, Vec<OsString>) -> io::Error,
) -> io::Error {
    exec(program, argv.into_iter().collect())
}

#[cfg(test)]
#[path = "restart_tests.rs"]
mod tests;
