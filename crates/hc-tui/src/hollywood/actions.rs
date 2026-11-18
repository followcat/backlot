//! Layer actions — `backlot card apply|cancel|show <id>` and
//! `backlot tell hollywood "<line>"` spawned off the event loop
//! through `std::env::current_exe()`, like the panel's runner. The
//! runner is a boxed async call so tests inject a recorder and never
//! spawn. Ok carries stdout: `card show` output is what the "Show
//! me" view draws.

use std::path::PathBuf;
use std::sync::Arc;

use crate::app::BoxFut;

/// One request: the argv tail after `backlot` (`card apply c3 …`,
/// `tell hollywood …`), already carrying `--as` and the endpoint
/// flags. The future resolves `Ok(stdout)` on exit 0 or
/// `Err(first stderr line)` otherwise.
pub(crate) type HollywoodRunner =
    Arc<dyn Fn(Vec<String>) -> BoxFut<Result<String, String>> + Send + Sync>;

/// A layer action the user asked for — the message a result notice
/// names. `view` means a successful result opens the "Show me" read
/// instead of being discarded.
#[derive(Debug, Clone)]
pub(crate) struct HollywoodAction {
    /// For the notice: `card apply c3`, `tell hollywood`.
    pub label: String,
    /// The argv tail handed to the runner.
    pub argv: Vec<String>,
    /// True only for `card show`: stdout becomes the view's text.
    pub view: bool,
    /// The machine the command belongs on — `None` is this core.
    /// Remote prompt, stage and finish actions run through
    /// `RemoteRunner`; cards and `tell` stay local.
    pub machine: Option<String>,
}

/// The real runner: spawn `exe <argv>` detached from the UI thread
/// and report the exit. `current_exe()` is the backlot binary itself
/// — the stage commands dispatch before TUI parsing.
pub(crate) fn spawn_runner(exe: PathBuf) -> HollywoodRunner {
    Arc::new(move |argv| {
        let exe = exe.clone();
        Box::pin(async move {
            let output = match tokio::process::Command::new(&exe)
                .args(&argv)
                .stdin(std::process::Stdio::null())
                .output()
                .await
            {
                Ok(output) => output,
                Err(e) => return Err(format!("cannot run backlot: {e}")),
            };
            match output.status.success() {
                true => Ok(String::from_utf8_lossy(&output.stdout).into_owned()),
                false => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    Err(stderr
                        .lines()
                        .find(|l| !l.trim().is_empty())
                        .map(str::trim)
                        .unwrap_or("backlot command failed")
                        .to_owned())
                }
            }
        })
    })
}
