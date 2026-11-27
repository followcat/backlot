//! Pure helpers for machine-local, repo-scoped operator notes
//! (`<state_dir>/repo-notes/<sha256(canonical_repo)[..16]>.md`):
//! slug validation, secret/DSN rejection, markdown section parsing,
//! idempotent `<!-- card c<seq> -->` upserts, and section digests.

use std::collections::BTreeMap;

use crate::hollywood_snapshot::sha256_hex;

/// Maximum UTF-8 byte length of a single repo note entry text.
pub const MAX_NOTE_BYTES: usize = 1024;
/// Maximum slug length for `entry_id`.
pub const MAX_ENTRY_ID_LEN: usize = 40;

/// Parsed `## <entry_id>` section from a repo-notes markdown file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoNoteSection {
    pub entry_id: String,
    pub card_seq: Option<u64>,
    pub body: String,
}

/// True when `slug` matches `[a-z0-9_-]{1,40}`.
pub fn valid_entry_id(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= MAX_ENTRY_ID_LEN
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// First 16 lowercase hex chars of `sha256(canonical_repo)`.
pub fn repo_hash_prefix(canonical_repo: &str) -> String {
    let hex = sha256_hex(canonical_repo.as_bytes());
    hex[..16].to_string()
}

fn contains_uri_credentials(text: &str) -> bool {
    let mut rest = text;
    while let Some(idx) = rest.find("://") {
        let after = &rest[idx + 3..];
        let auth_end = after
            .find(['/', '?', '#', ' ', '\t', '\n', '\r', '"', '\''])
            .unwrap_or(after.len());
        let authority = &after[..auth_end];
        if let Some((userinfo, host)) = authority.split_once('@')
            && !userinfo.is_empty()
            && !host.is_empty()
        {
            return true;
        }
        rest = after;
    }
    false
}

fn has_token_prefix(text: &str) -> bool {
    for prefix in [
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "ghr_",
        "github_pat_",
        "sk-ant-",
        "sk-proj-",
        "sk-live-",
        "sk-test-",
        "xoxb-",
        "xoxp-",
        "xoxa-",
        "AIza",
    ] {
        let mut rest = text;
        while let Some(idx) = rest.find(prefix) {
            let tail = &rest[idx + prefix.len()..];
            let token_len = tail
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
                .count();
            if token_len >= 10 {
                return true;
            }
            rest = tail;
        }
    }
    // Generic `sk-<long alphanumeric>` key.
    let mut rest = text;
    while let Some(idx) = rest.find("sk-") {
        let boundary_ok = idx == 0 || !rest.as_bytes()[idx - 1].is_ascii_alphanumeric();
        let tail = &rest[idx + 3..];
        if boundary_ok
            && tail
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
                .count()
                >= 16
        {
            return true;
        }
        rest = tail;
    }
    // AWS access key ID (`AKIA...` / `ASIA...`, 20 uppercase alnum).
    for prefix in ["AKIA", "ASIA"] {
        let mut r = text;
        while let Some(idx) = r.find(prefix) {
            let tail = &r[idx + 4..];
            if tail
                .bytes()
                .take_while(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
                .count()
                >= 16
            {
                return true;
            }
            r = tail;
        }
    }
    false
}

fn contains_kv_secret(lower: &str) -> bool {
    for key in ["password=", "passwd=", "secret=", "api_key=", "auth_token="] {
        if let Some(idx) = lower.find(key) {
            let rhs = lower[idx + key.len()..].trim_start();
            if !rhs.is_empty() && !rhs.starts_with([' ', '\n', '\t']) {
                return true;
            }
        }
    }
    false
}

fn contains_bearer_or_pem(text: &str) -> bool {
    if text.contains("-----BEGIN ") && text.contains("PRIVATE KEY-----") {
        return true;
    }
    let lower = text.to_ascii_lowercase();
    let mut rest = lower.as_str();
    while let Some(idx) = rest.find("bearer ") {
        let tail = rest[idx + 7..].trim_start();
        let token_len = tail
            .bytes()
            .take_while(|b| !b.is_ascii_whitespace())
            .count();
        if token_len >= 12 {
            return true;
        }
        rest = tail;
    }
    false
}

/// Static safety check for repo notes: non-empty, <= 1024 bytes, printable text,
/// and free of obvious secrets, private keys, or DSNs with embedded credentials.
pub fn check_text_safe(text: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("repo-note text must not be empty".into());
    }
    if text.len() > MAX_NOTE_BYTES {
        return Err(format!(
            "repo-note text exceeds {MAX_NOTE_BYTES} bytes (got {})",
            text.len()
        ));
    }
    if text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err("repo-note text cannot contain control characters".into());
    }
    if text.contains("<!--") {
        return Err(
            "repo-note text rejected: cannot contain HTML/card marker comments (`<!--`)".into(),
        );
    }
    if text.lines().any(|line| line.trim_start().starts_with('#')) {
        return Err(
            "repo-note text rejected: lines cannot start with `#` (would forge markdown sections)"
                .into(),
        );
    }
    let lower = text.to_ascii_lowercase();
    if contains_uri_credentials(text)
        || has_token_prefix(text)
        || contains_kv_secret(&lower)
        || contains_bearer_or_pem(text)
    {
        return Err(
            "repo-note text rejected: appears to contain a secret, token, or DSN with credentials"
                .into(),
        );
    }
    Ok(())
}

fn parse_card_marker(line: &str) -> Option<u64> {
    let trimmed = line.trim();
    let inner = trimmed.strip_prefix("<!--")?.strip_suffix("-->")?.trim();
    let num = inner.strip_prefix("card c")?.trim();
    num.parse::<u64>().ok()
}

/// Parse `## <entry_id>` sections in order of `entry_id`.
pub fn parse_sections(doc: &str) -> BTreeMap<String, RepoNoteSection> {
    let mut out = BTreeMap::new();
    let mut current_id: Option<String> = None;
    let mut current_seq: Option<u64> = None;
    let mut current_lines: Vec<String> = Vec::new();

    let flush = |id: Option<String>,
                 seq: Option<u64>,
                 lines: Vec<String>,
                 out: &mut BTreeMap<String, RepoNoteSection>| {
        let Some(entry_id) = id else { return };
        let body = lines.join("\n").trim().to_string();
        out.insert(
            entry_id.clone(),
            RepoNoteSection {
                entry_id,
                card_seq: seq,
                body,
            },
        );
    };

    for line in doc.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            let slug = rest.trim().to_string();
            if valid_entry_id(&slug) {
                flush(
                    current_id.take(),
                    current_seq.take(),
                    std::mem::take(&mut current_lines),
                    &mut out,
                );
                current_id = Some(slug);
                continue;
            }
        }
        if current_id.is_none() {
            continue;
        }
        if current_seq.is_none()
            && current_lines.is_empty()
            && let Some(seq) = parse_card_marker(line)
        {
            current_seq = Some(seq);
            continue;
        }
        current_lines.push(line.to_string());
    }
    flush(current_id, current_seq, current_lines, &mut out);
    out
}

/// Canonical SHA-256 digest of a section (`None` when absent).
pub fn entry_sha256(section: Option<&RepoNoteSection>) -> Option<String> {
    let sec = section?;
    let canonical = format!("{}\n{}", sec.card_seq.unwrap_or(0), sec.body.trim());
    Some(sha256_hex(canonical.as_bytes()))
}

/// True if `doc` already carries `<!-- card c<card_seq> -->` under `## <entry_id>`.
pub fn has_card_marker(doc: &str, entry_id: &str, card_seq: u64) -> bool {
    parse_sections(doc)
        .get(entry_id)
        .is_some_and(|sec| sec.card_seq == Some(card_seq))
}

fn render_doc(canonical_repo: &str, sections: &BTreeMap<String, RepoNoteSection>) -> String {
    let mut out = format!("# Repo Notes: {canonical_repo}\n");
    for sec in sections.values() {
        out.push_str(&format!("\n## {}\n", sec.entry_id));
        if let Some(seq) = sec.card_seq {
            out.push_str(&format!("<!-- card c{seq} -->\n"));
        }
        if !sec.body.is_empty() {
            out.push_str(sec.body.trim());
            out.push('\n');
        }
    }
    out
}

/// Insert or replace `## <entry_id>` with `<!-- card c<card_seq> -->` + `text`.
pub fn upsert_section(
    existing_doc: Option<&str>,
    canonical_repo: &str,
    entry_id: &str,
    card_seq: u64,
    text: &str,
) -> String {
    let mut sections = existing_doc.map(parse_sections).unwrap_or_default();
    sections.insert(
        entry_id.to_string(),
        RepoNoteSection {
            entry_id: entry_id.to_string(),
            card_seq: Some(card_seq),
            body: text.trim().to_string(),
        },
    );
    render_doc(canonical_repo, &sections)
}

/// Remove `## <entry_id>` from `existing_doc`, returning `Ok(None)` when the last
/// entry was removed (so the file itself can be deleted).
pub fn remove_section(
    existing_doc: &str,
    canonical_repo: &str,
    entry_id: &str,
) -> Result<Option<String>, String> {
    let mut sections = parse_sections(existing_doc);
    if sections.remove(entry_id).is_none() {
        return Err(format!(
            "no repo-note entry '{entry_id}' for '{canonical_repo}'"
        ));
    }
    if sections.is_empty() {
        Ok(None)
    } else {
        Ok(Some(render_doc(canonical_repo, &sections)))
    }
}
