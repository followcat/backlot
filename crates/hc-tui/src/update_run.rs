//! The in-client installer: the pipeline the update runs (spec 5.1), its
//! process-group spawn and signals, and the post-install `--version`
//! check (spec 5.3). Everything runs as a local `sh -c` on this machine —
//! never through the core and never on a remote endpoint.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;

use crate::app::update_flow::UpdateEvent;
use crate::update_install::shell_quote;

/// backlot's installer, the same script `backlot --upgrade` runs.
pub(crate) const INSTALL_URL: &str = crate::cli::INSTALL_URL;
/// The binary check after a zero exit (spec 5.3).
pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// One installer run, fully resolved before anything is spawned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Plan {
    /// The pipeline handed to `sh -c`.
    pub pipeline: String,
    /// The three variables the pipeline sets before `sh`.
    pub env: Vec<(&'static str, String)>,
    /// The echoed command, wrapped with `\` continuations.
    pub echo: Vec<String>,
    /// The binary the installer writes and the check runs.
    pub bin: PathBuf,
}

/// The pipeline for installing `latest` into `dir`: pinned version,
/// no core, and an explicit directory.
pub(crate) fn plan(dir: &Path, latest: &str) -> Plan {
    let version = format!("v{latest}");
    let quoted_version = shell_quote(&version);
    let dir_text = dir.display().to_string();
    let quoted = shell_quote(&dir_text);
    Plan {
        pipeline: format!(
            "curl -fsSL {INSTALL_URL} | BACKLOT_VERSION={quoted_version} BACKLOT_SKIP_CORE=1 BACKLOT_INSTALL_DIR={quoted} sh"
        ),
        env: vec![
            ("BACKLOT_VERSION", version.clone()),
            ("BACKLOT_SKIP_CORE", "1".to_owned()),
            ("BACKLOT_INSTALL_DIR", dir_text),
        ],
        echo: vec![
            format!("$ curl -fsSL {INSTALL_URL} \\"),
            format!("  | BACKLOT_VERSION={quoted_version} BACKLOT_SKIP_CORE=1 \\"),
            format!("    BACKLOT_INSTALL_DIR={quoted} sh"),
        ],
        bin: dir.join("backlot"),
    }
}

/// Starts `sh -c script` in its own process group and streams its
/// output lines, then one `Exited` event, to `events`. Returns the
/// group id (the child's pid, since the child leads its group).
pub(crate) fn spawn_group(
    script: &str,
    events: mpsc::Sender<UpdateEvent>,
    generation: u64,
) -> std::io::Result<i32> {
    let mut child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(script)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let pgid = child.id().map_or(0, |pid| pid as i32);
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    tokio::spawn(async move {
        let out = tokio::spawn(stream_lines(stdout, events.clone(), generation));
        let err = tokio::spawn(stream_lines(stderr, events.clone(), generation));
        let status = child.wait().await;
        let _ = tokio::join!(out, err);
        let code = status.ok().and_then(|status| status.code());
        let _ = events.send(UpdateEvent::Exited { generation, code }).await;
    });
    Ok(pgid)
}

/// Each line of one pipe becomes an `Output` event until it closes.
async fn stream_lines<R>(pipe: Option<R>, events: mpsc::Sender<UpdateEvent>, generation: u64)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let Some(pipe) = pipe else { return };
    let mut lines = BufReader::new(pipe).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if events
            .send(UpdateEvent::Output { generation, line })
            .await
            .is_err()
        {
            break;
        }
    }
}

/// Sends `signal` to every process in group `pgid`. A group that is
/// already gone is not an error.
pub(crate) fn signal_group(pgid: i32, signal: i32) {
    if pgid > 0 {
        // SAFETY: `kill` takes plain integers; a negative pid addresses a
        // process group and has no memory effects.
        unsafe {
            libc::kill(-pgid, signal);
        }
    }
}

/// Whether group `pgid` still has a live member.
pub(crate) fn group_alive(pgid: i32) -> bool {
    pgid > 0 && unsafe { libc::kill(-pgid, 0) == 0 }
}

/// Runs `<bin> --version` with a three-second bound and returns its
/// trimmed stdout, or `None` if it cannot run, exits badly or times out.
pub(crate) async fn probe_version(bin: PathBuf) -> Option<String> {
    let output = tokio::time::timeout(
        PROBE_TIMEOUT,
        tokio::process::Command::new(&bin)
            .arg("--version")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// After cancelling, waits `grace` and kills the group if it is still
/// alive (spec 5.2). The seams are injected so tests drive it directly.
pub(crate) async fn reap_after(
    pgid: i32,
    grace: Duration,
    alive: impl Fn(i32) -> bool,
    signal: impl Fn(i32, i32),
) {
    tokio::time::sleep(grace).await;
    if alive(pgid) {
        signal(pgid, libc::SIGKILL);
    }
}

#[cfg(test)]
#[path = "update_run_tests.rs"]
mod tests;
