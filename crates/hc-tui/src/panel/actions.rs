//! Panel actions — `backlot priority|reopen ...` spawned off the
//! event loop through `std::env::current_exe()`. The runner is a
//! boxed async call so tests inject a recorder and never spawn.

use std::path::PathBuf;
use std::sync::Arc;

use crate::app::BoxFut;

/// One request: the argv tail after `backlot` (`priority --level …`
/// or `reopen --person …`, already carrying `--studio`/`--as` and the
/// endpoint flags). The future resolves `Ok(())` on exit 0 or
/// `Err(first stderr line)` otherwise.
pub(crate) type PanelRunner = Arc<dyn Fn(Vec<String>) -> BoxFut<Result<(), String>> + Send + Sync>;

/// A panel action the user asked for — the message a result notice
/// names.
#[derive(Debug, Clone)]
pub(crate) struct PanelAction {
    /// For the notice: `priority high`, `reopen alice`.
    pub label: String,
    /// The argv tail handed to the runner.
    pub argv: Vec<String>,
}

/// The real runner: spawn `exe <argv>` detached from the UI thread
/// and report the exit. `current_exe()` is the backlot binary itself —
/// the studio commands dispatch before TUI parsing.
pub(crate) fn spawn_runner(exe: PathBuf) -> PanelRunner {
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
                true => Ok(()),
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
