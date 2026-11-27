//! Environment sanitization, bounded output tail/log helpers, and
//! `outward.completed` ledger recording for `CardAction::Outward`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use backlot_model::event::{EventKind, OutwardOutcome};
use backlot_model::{EventBy, NewEvent};

use crate::checkrun::LOG_CAP;
use crate::cli::Env;
use crate::guard_prep::strip_shim_dirs;
use crate::store::{self, Pending};

const TAIL_BYTES: usize = 2048;
const TAIL_LINES: usize = 30;

/// Minimal clean environment for `stage`-owned read-only git/gh inspection.
pub(crate) fn stage_probe_env() -> Env {
    let mut vars = BTreeMap::new();
    for k in [
        "PATH",
        "HOME",
        "USER",
        "LOGNAME",
        "LANG",
        "LC_ALL",
        "TERM",
        "TMPDIR",
        "SSH_AUTH_SOCK",
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "GH_HOST",
    ] {
        if let Ok(v) = std::env::var(k) {
            vars.insert(k.into(), v);
        }
    }
    vars.insert("GIT_CONFIG_COUNT".into(), "0".into());
    vars.insert("GIT_TERMINAL_PROMPT".into(), "0".into());
    vars.insert("GH_PROMPT_DISABLED".into(), "1".into());
    Env { vars }
}

/// Build the clean person environment used to spawn approved outward commands:
/// keeps the user's session/credential env, strips all `BACKLOT_*` and
/// `GIT_CONFIG_*` variables, drops `<role>/bin` and any backlot guard shim dir
/// from `PATH`, and disables interactive prompts.
pub(crate) fn person_exec_env(
    stage_env: &BTreeMap<String, String>,
    role_bin: Option<&Path>,
) -> Vec<(String, String)> {
    let mut out = Vec::with_capacity(stage_env.len() + 2);
    for (k, v) in stage_env {
        if k.starts_with("BACKLOT_")
            || k.starts_with("GIT_CONFIG_")
            || k == "GIT_TERMINAL_PROMPT"
            || k == "GH_PROMPT_DISABLED"
        {
            continue;
        }
        if k == "PATH" {
            out.push(("PATH".into(), strip_shim_dirs(v, role_bin)));
            continue;
        }
        out.push((k.clone(), v.clone()));
    }
    out.push(("GIT_TERMINAL_PROMPT".into(), "0".into()));
    out.push(("GH_PROMPT_DISABLED".into(), "1".into()));
    out
}

/// Cap completion tail to at most 30 lines and 2 KiB at a UTF-8 char boundary.
pub(crate) fn cap_tail(bytes: &[u8]) -> String {
    let lossy = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = lossy.lines().collect();
    let start = lines.len().saturating_sub(TAIL_LINES);
    let joined = lines[start..].join("\n");
    if joined.len() <= TAIL_BYTES {
        return joined;
    }
    let mut idx = joined.len() - TAIL_BYTES;
    while idx < joined.len() && !joined.is_char_boundary(idx) {
        idx += 1;
    }
    joined[idx..].to_string()
}

pub(crate) fn write_outward_log(sdir: &Path, card: u64, output: &[u8]) -> Option<String> {
    let rel = format!("outward/c{card}.log");
    let path = sdir.join(&rel);
    if let Some(parent) = path.parent()
        && fs::create_dir_all(parent).is_err()
    {
        return None;
    }
    let capped = &output[..output.len().min(LOG_CAP)];
    fs::write(&path, capped).ok().map(|()| rel)
}

pub(crate) fn record_outward_completed(
    sdir: &Path,
    card: u64,
    source_seq: u64,
    outcome: OutwardOutcome,
    exit_code: Option<i32>,
    output_tail: String,
    log_file: Option<String>,
) {
    let kind = EventKind::OutwardCompleted {
        card,
        source_seq,
        outcome,
        exit_code,
        output_tail,
        log_file,
    };
    let _ = store::append(sdir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(EventBy::role("stage"), kind),
            files: Vec::new(),
        })
    });
}

pub(crate) fn finish_reason(outcome: OutwardOutcome, code: Option<i32>, tail: &str) -> String {
    let last_line = tail
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(str::trim)
        .unwrap_or("");
    match outcome {
        OutwardOutcome::TimedOut => "outward command timed out".into(),
        OutwardOutcome::Failed | OutwardOutcome::Denied | OutwardOutcome::Stale => {
            let prefix = match code {
                Some(c) => format!("exit {c}"),
                None => "outward command failed".into(),
            };
            if last_line.is_empty() {
                prefix
            } else {
                format!("{prefix}: {last_line}")
            }
        }
        OutwardOutcome::Succeeded => String::new(),
    }
}
