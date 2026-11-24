//! D5 one-line progress & active command types and helpers, shared
//! between events, reduced state, summaries, `backlot status`, and
//! the panel.

use serde::{Deserialize, Serialize};

/// Maximum Unicode scalar characters kept in one `role.progress` text.
pub const MAX_PROGRESS_CHARS: usize = 200;

/// Maximum Unicode scalar characters kept in one `RoleSummary.doing` command.
pub const MAX_DOING_CHARS: usize = 60;

/// The role's latest `role.progress` report from the ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleProgress {
    pub text: String,
    pub ts_ms: u64,
}

/// The deepest descendant command currently running under a working role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleDoing {
    #[serde(alias = "text")]
    pub cmd: String,
    #[serde(default)]
    pub since_ms: u64,
}

/// Normalize progress text: fold line/tab separators into spaces, strip
/// other control characters, collapse whitespace runs to single spaces,
/// and cap at [`MAX_PROGRESS_CHARS`].
pub fn normalize_progress_text(raw: &str) -> String {
    let stripped: String = raw
        .chars()
        .filter_map(|c| match c {
            '\n' | '\r' | '\t' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect();
    let one_line = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
    one_line
        .chars()
        .take(MAX_PROGRESS_CHARS)
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// Cap a joined process argv string for summary publication (`doing.cmd`).
pub fn cap_doing(cmd: &str) -> String {
    cmd.chars().take(MAX_DOING_CHARS).collect()
}

/// Cap a progress text read from ledger state to [`MAX_PROGRESS_CHARS`].
pub(crate) fn cap_progress(text: &str) -> String {
    text.chars().take(MAX_PROGRESS_CHARS).collect()
}

/// Append-time check for stored `role.progress` text.
pub(crate) fn valid_progress_text(text: &str) -> Result<(), &'static str> {
    if text.is_empty() {
        return Err("progress text must be nonempty");
    }
    if text.chars().count() > MAX_PROGRESS_CHARS {
        return Err("progress text exceeds 200 characters");
    }
    if text.chars().any(char::is_control) {
        return Err("progress text must be a single line without control characters");
    }
    Ok(())
}

/// `2h`, `3m`, `12s` — compact duration/age shared across the summary
/// views, `backlot status`, and stage stall diagnostics.
pub fn age_short(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}h", s / 3600)
    } else if s >= 60 {
        format!("{}m", s / 60)
    } else {
        format!("{s}s")
    }
}
