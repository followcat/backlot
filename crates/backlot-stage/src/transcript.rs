//! Runner-generic conversation tail extraction, ANSI cleanup,
//! secret redaction, tool collapsing, and universal fallback (§2.3.2–§2.3.3).

use std::time::{Duration, Instant};

use crate::discover::Exec;

pub(crate) const MAX_TAIL_BYTES: usize = 16 * 1024;
pub(crate) const MAX_TURNS: usize = 30;
pub(crate) const MAX_TOOL_SUMMARY_CHARS: usize = 300;
pub(crate) const EXTRACT_TIMEOUT: Duration = Duration::from_secs(3);

/// Where a role's transcript lives so any runner extractor can read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranscriptTarget<'a> {
    pub cwd: &'a str,
    pub runner: &'a str,
    pub session_id: Option<&'a str>,
    pub role_dir: &'a std::path::Path,
    pub home: Option<&'a std::path::Path>,
}

/// Whether the tail came from a runner session store or from terminal+ledger fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TailKind {
    RunnerSession,
    TerminalFallback,
}

/// Extracted conversation tail ready for inclusion in a handoff brief.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtractedTail {
    pub kind: TailKind,
    pub provenance: String,
    pub markdown: String,
}

/// One normalized conversation turn (tool calls already collapsed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Turn {
    pub who: String,
    pub text: String,
}

/// Extract conversation tail from the runner's session storage, read-only and
/// bounded by [`EXTRACT_TIMEOUT`]. Returns `None` when the session is absent,
/// unsupported, or yields no usable turns so callers fall back cleanly.
pub(crate) fn extract_runner_tail(
    target: &TranscriptTarget<'_>,
    exec: &Exec,
) -> Option<ExtractedTail> {
    let sid = target.session_id?.trim();
    if sid.is_empty() || sid.contains(['/', '\\', '\0']) || sid.contains("..") {
        return None;
    }
    let deadline = Instant::now() + EXTRACT_TIMEOUT;
    let (provenance, turns) = crate::transcript_runners::read_turns(target, sid, exec, deadline)?;
    if turns.is_empty() || Instant::now() > deadline {
        return None;
    }
    let markdown = format_turns(&turns);
    if markdown.trim().is_empty() {
        return None;
    }
    Some(ExtractedTail {
        kind: TailKind::RunnerSession,
        provenance,
        markdown,
    })
}

/// Universal fallback when no runner session transcript can be read:
/// ANSI-stripped terminal scrollback plus recent ledger entries.
pub(crate) fn format_fallback_tail(
    screen_scrollback: Option<&str>,
    ledger_lines: &[String],
) -> ExtractedTail {
    let mut parts = Vec::new();
    if let Some(screen) = screen_scrollback {
        let clean = redact_secrets(&strip_ansi_clean(screen));
        let trimmed = clean.trim();
        if !trimmed.is_empty() {
            parts.push(format!(
                "#### Old terminal scrollback (last lines)\n\n```text\n{trimmed}\n```"
            ));
        }
    }
    if !ledger_lines.is_empty() {
        let body = redact_secrets(&ledger_lines.join("\n"));
        parts.push(format!(
            "#### Recent ledger exchange\n\n```text\n{body}\n```"
        ));
    }
    let markdown = if parts.is_empty() {
        "_No prior scrollback or ledger entries available._\n".to_string()
    } else {
        trim_newest_bytes(&parts.join("\n\n"), MAX_TAIL_BYTES)
    };
    ExtractedTail {
        kind: TailKind::TerminalFallback,
        provenance: "terminal scrollback + ledger exchange (runner transcript unavailable)".into(),
        markdown,
    }
}

/// Format up to [`MAX_TURNS`] newest turns capped at [`MAX_TAIL_BYTES`], dropping
/// oldest turns first when oversized.
pub(crate) fn format_turns(turns: &[Turn]) -> String {
    let start = turns.len().saturating_sub(MAX_TURNS);
    let mut blocks: Vec<String> = turns[start..]
        .iter()
        .filter_map(|t| {
            let text = redact_secrets(t.text.trim());
            if text.is_empty() {
                return None;
            }
            let heading = match t.who.as_str() {
                "user" => "User",
                "assistant" => "Assistant",
                other => other,
            };
            Some(format!("#### {heading}\n\n{text}"))
        })
        .collect();
    while blocks.len() > 1 && blocks.join("\n\n").len() > MAX_TAIL_BYTES {
        blocks.remove(0);
    }
    let joined = blocks.join("\n\n");
    if joined.len() > MAX_TAIL_BYTES {
        trim_newest_bytes(&joined, MAX_TAIL_BYTES)
    } else {
        joined
    }
}

/// Collapse one tool invocation/result into a single line up to [`MAX_TOOL_SUMMARY_CHARS`].
pub(crate) fn collapse_tool(name: &str, detail: &str) -> String {
    let flat: String = detail.split_whitespace().collect::<Vec<_>>().join(" ");
    let prefix = format!("[tool: {name}] ");
    let budget = MAX_TOOL_SUMMARY_CHARS.saturating_sub(prefix.chars().count());
    let truncated = cap_chars(&flat, budget);
    redact_secrets(&format!("{prefix}{truncated}"))
}

pub(crate) fn cap_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let keep: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{keep}…")
}

/// Trim `s` to the newest UTF-8 slice at or below `max_bytes`.
pub(crate) fn trim_newest_bytes(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut idx = s.len().saturating_sub(max_bytes);
    while idx < s.len() && !s.is_char_boundary(idx) {
        idx += 1;
    }
    // Prefer starting on a newline so we do not break the middle of a line.
    if let Some(nl) = s[idx..].find('\n') {
        let next = idx + nl + 1;
        if next < s.len() {
            idx = next;
        }
    }
    format!("… [truncated]\n{}", &s[idx..])
}

/// Strip ANSI escape sequences and control noise cleanly for readable scrollback.
pub(crate) fn strip_ansi_clean(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.peek().copied() {
                Some('[') => {
                    chars.next();
                    for ch in chars.by_ref() {
                        if ('@'..='~').contains(&ch) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(ch) = chars.next() {
                        if ch == '\x07' || (ch == '\x1b' && chars.peek() == Some(&'\\')) {
                            if ch == '\x1b' {
                                chars.next();
                            }
                            break;
                        }
                    }
                }
                Some(_) => {
                    chars.next();
                }
                None => break,
            }
        } else if c == '\r' || (c.is_control() && c != '\n' && c != '\t') {
            continue;
        } else {
            out.push(c);
        }
    }
    out
}

/// Redact bearer tokens, API/VCS keys, `*_TOKEN`/`*_KEY`/`*_SECRET`/`*_PASSWORD` env
/// assignments, and URL credentials from transcript excerpts.
pub(crate) fn redact_secrets(text: &str) -> String {
    let mut out = redact_bearer_and_prefixes(text);
    out = redact_env_assignments(&out);
    redact_url_userinfo(&out)
}

fn is_token_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'+' | b'/' | b'=')
}

fn is_env_char(b: u8) -> bool {
    b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'
}

fn redact_bearer_and_prefixes(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].len() > 7 && bytes[i..i + 7].eq_ignore_ascii_case(b"bearer ") {
            let start = i + 7;
            let mut end = start;
            while end < bytes.len() && is_token_char(bytes[end]) {
                end += 1;
            }
            if end > start + 3 {
                out.push_str(&input[i..start]);
                out.push_str("[REDACTED]");
                i = end;
                continue;
            }
        }
        let prefix = ["sk-", "ghp_", "gho_", "github_pat_", "AIza"]
            .into_iter()
            .find(|p| input[i..].starts_with(p));
        if let Some(p) = prefix {
            let boundary = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
            let start = i + p.len();
            let mut end = start;
            while end < bytes.len()
                && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'-' || bytes[end] == b'_')
            {
                end += 1;
            }
            if boundary && end.saturating_sub(start) >= 8 {
                out.push_str(p);
                out.push_str("[REDACTED]");
                i = end;
                continue;
            }
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn redact_env_assignments(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        let boundary = i == 0 || !is_env_char(bytes[i - 1]);
        if boundary && bytes[i].is_ascii_uppercase() {
            let mut k_end = i;
            while k_end < bytes.len() && is_env_char(bytes[k_end]) {
                k_end += 1;
            }
            let key = &input[i..k_end];
            if (key.ends_with("_TOKEN")
                || key.ends_with("_KEY")
                || key.ends_with("_SECRET")
                || key.ends_with("_PASSWORD"))
                && k_end < bytes.len()
                && bytes[k_end] == b'='
            {
                let mut v_start = k_end + 1;
                let quote = if v_start < bytes.len() && matches!(bytes[v_start], b'"' | b'\'') {
                    let q = bytes[v_start];
                    v_start += 1;
                    Some(q)
                } else {
                    None
                };
                let mut v_end = v_start;
                while v_end < bytes.len() {
                    if let Some(q) = quote {
                        if bytes[v_end] == q {
                            v_end += 1;
                            break;
                        }
                    } else if bytes[v_end].is_ascii_whitespace() {
                        break;
                    }
                    v_end += 1;
                }
                out.push_str(&input[i..=k_end]);
                out.push_str("[REDACTED]");
                i = v_end;
                continue;
            }
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn redact_url_userinfo(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(pos) = rest.find("://") {
        let after = &rest[pos + 3..];
        let limit = after
            .find(|c: char| c.is_ascii_whitespace() || c == '/' || c == '"' || c == '\'')
            .unwrap_or(after.len());
        if let Some(at) = after[..limit].rfind('@') {
            out.push_str(&rest[..pos + 3]);
            out.push_str("[REDACTED]");
            rest = &after[at..];
            continue;
        }
        out.push_str(&rest[..pos + 3]);
        rest = after;
    }
    out.push_str(rest);
    out
}
