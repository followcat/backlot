//! Pure helpers for architectural decision messages (`purpose = "decision"`):
//! `AD-<n>` parsing, normalized shared body formatting, deduplication across
//! `--notify` recipients, numeric sorting, supersession folding into History,
//! and rendering `decisions/current.md`.

use std::collections::BTreeMap;

use crate::state::StudioState;

/// Metadata extracted from a normalized decision message file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionMeta {
    pub number: u32,
    pub supersedes: Option<u32>,
    pub title: Option<String>,
    pub body: String,
}

/// One deduplicated architectural decision anchored at its authoritative `#<seq>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionEntry {
    pub number: u32,
    pub seq: u64,
    pub from: String,
    pub supersedes: Option<u32>,
    pub title: Option<String>,
    pub body: String,
}

/// Parse an architectural decision identifier of the form `AD-<n>` (`n >= 1`,
/// no leading zeros).
pub fn parse_decision_id(raw: &str) -> Option<u32> {
    let num = raw.strip_prefix("AD-")?;
    if num.is_empty() || num.starts_with('0') || !num.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    num.parse::<u32>().ok().filter(|&n| n >= 1)
}

/// Format an `AD-<n>` identifier.
pub fn format_id(number: u32) -> String {
    format!("AD-{number}")
}

/// Extract `(number, optional title)` if `line` begins with optional `#` heading
/// markers followed by `AD-<n>`.
fn parse_id_heading(line: &str) -> Option<(u32, Option<String>)> {
    let trimmed = line.trim().trim_start_matches('#').trim();
    let rest = trimmed.strip_prefix("AD-")?;
    let digits_len = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits_len == 0 {
        return None;
    }
    let (digits, after) = rest.split_at(digits_len);
    let number = parse_decision_id(&format!("AD-{digits}"))?;
    if !after.is_empty()
        && !after.starts_with([':', '-', ' ', '\t'])
        && !after.starts_with('—')
        && !after.starts_with('–')
    {
        return None;
    }
    let title = after
        .trim_start_matches([':', ' ', '\t', '-'])
        .trim_start_matches('—')
        .trim_start_matches('–')
        .trim()
        .to_string();
    let title = (!title.is_empty()).then_some(title);
    Some((number, title))
}

fn parse_supersedes_line(line: &str) -> Option<Result<u32, String>> {
    let trimmed = line.trim();
    let lower = trimmed.to_ascii_lowercase();
    let rest = lower.strip_prefix("supersedes:")?;
    let offset = trimmed.len() - rest.len();
    let value = trimmed[offset..].trim();
    match parse_decision_id(value) {
        Some(n) => Some(Ok(n)),
        None => Some(Err(format!(
            "invalid Supersedes header '{value}': expected AD-<n>"
        ))),
    }
}

/// Parse an `AD-<n>` header, optional `Supersedes: AD-<m>` line, and body.
pub fn parse_decision_body(raw: &str) -> Option<DecisionMeta> {
    let mut lines = raw.lines().peekable();
    while lines.peek().is_some_and(|l| l.trim().is_empty()) {
        lines.next();
    }
    let first = lines.next()?;
    let (number, title) = parse_id_heading(first)?;

    while lines.peek().is_some_and(|l| l.trim().is_empty()) {
        lines.next();
    }
    let mut supersedes = None;
    if let Some(next_line) = lines.peek()
        && let Some(parsed) = parse_supersedes_line(next_line)
    {
        supersedes = parsed.ok();
        lines.next();
    }
    while lines.peek().is_some_and(|l| l.trim().is_empty()) {
        lines.next();
    }
    let rest: Vec<&str> = lines.collect();
    let body = rest.join("\n").trim().to_string();
    Some(DecisionMeta {
        number,
        supersedes,
        title,
        body,
    })
}

/// Validate `--id` / `--supersedes` and ensure the shared decision message
/// begins with the canonical `AD-<n>` header (and `Supersedes: AD-<m>` if
/// given) so all notified copies refer to the same decision.
pub fn prepare_decision_body(
    id: &str,
    supersedes: Option<&str>,
    raw_body: &str,
) -> Result<String, String> {
    let number = parse_decision_id(id)
        .ok_or_else(|| format!("invalid --id '{id}': expected AD-<n> (positive integer)"))?;
    let sup_num = match supersedes {
        Some(raw) => {
            let m = parse_decision_id(raw).ok_or_else(|| {
                format!("invalid --supersedes '{raw}': expected AD-<m> (positive integer)")
            })?;
            if m == number {
                return Err(format!("{id} cannot supersede itself"));
            }
            Some(m)
        }
        None => None,
    };
    if raw_body.trim().is_empty() {
        return Err("decision file is empty".to_string());
    }

    let normalized_newlines = raw_body.replace("\r\n", "\n");
    let mut lines: Vec<&str> = normalized_newlines.lines().collect();
    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }

    let Some(&first) = lines.first() else {
        return Err("decision file is empty".to_string());
    };
    if let Some((found_num, _)) = parse_id_heading(first) {
        if found_num != number {
            return Err(format!(
                "file starts with AD-{found_num}, which conflicts with --id {id}"
            ));
        }
        let body_after_head = &lines[1..];
        let mut idx = 0;
        while idx < body_after_head.len() && body_after_head[idx].trim().is_empty() {
            idx += 1;
        }
        let mut out = String::new();
        out.push_str(first);
        out.push('\n');
        if let Some(line) = body_after_head.get(idx)
            && let Some(parsed) = parse_supersedes_line(line)
        {
            let found_sup = parsed?;
            if let Some(want) = sup_num
                && found_sup != want
            {
                return Err(format!(
                    "file says Supersedes: AD-{found_sup}, which conflicts with --supersedes AD-{want}"
                ));
            }
            out.push_str(line);
            out.push('\n');
            idx += 1;
        } else if let Some(want) = sup_num {
            out.push_str(&format!("Supersedes: AD-{want}\n"));
        }
        while idx < body_after_head.len() && body_after_head[idx].trim().is_empty() {
            idx += 1;
        }
        if idx < body_after_head.len() {
            out.push('\n');
            out.push_str(&body_after_head[idx..].join("\n"));
            out.push('\n');
        }
        return Ok(out);
    }

    let mut out = format!("# {id}\n");
    if let Some(want) = sup_num {
        out.push_str(&format!("Supersedes: AD-{want}\n"));
    }
    out.push('\n');
    out.push_str(normalized_newlines.trim());
    out.push('\n');
    Ok(out)
}

/// Derive deduplicated entries and supersession map from studio state.
pub fn collect_decisions(
    state: &StudioState,
    read_file: impl Fn(&str) -> Option<String>,
) -> (BTreeMap<u32, DecisionEntry>, BTreeMap<u32, (u32, u64)>) {
    let mut entries: BTreeMap<u32, DecisionEntry> = BTreeMap::new();
    let mut superseded_by: BTreeMap<u32, (u32, u64)> = BTreeMap::new();

    for m in &state.messages {
        if m.purpose.as_deref() != Some("decision") {
            continue;
        }
        let Some(rel) = m.file.as_deref() else {
            continue;
        };
        let Some(raw) = read_file(rel) else {
            continue;
        };
        let Some(meta) = parse_decision_body(&raw) else {
            continue;
        };
        if entries.contains_key(&meta.number) {
            continue;
        }
        if let Some(old) = meta.supersedes {
            superseded_by.entry(old).or_insert((meta.number, m.seq));
        }
        entries.insert(
            meta.number,
            DecisionEntry {
                number: meta.number,
                seq: m.seq,
                from: m.from.clone(),
                supersedes: meta.supersedes,
                title: meta.title,
                body: meta.body,
            },
        );
    }

    (entries, superseded_by)
}

fn summary_line(entry: &DecisionEntry) -> String {
    if let Some(title) = &entry.title {
        return title.clone();
    }
    let line = entry
        .body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let mut out: String = line.chars().take(100).collect();
    if line.chars().count() > 100 {
        out.push('…');
    }
    out
}

/// Render `decisions/current.md` with Current decisions sorted by numeric `AD-<n>`
/// and superseded decisions folded into History.
pub fn render_current_md(
    state: &StudioState,
    read_file: impl Fn(&str) -> Option<String>,
) -> String {
    let (entries, superseded_by) = collect_decisions(state, read_file);
    let mut out = String::from("# Architecture Decisions\n\n## Current\n\n");

    let active: Vec<&DecisionEntry> = entries
        .values()
        .filter(|e| !superseded_by.contains_key(&e.number))
        .collect();
    let history: Vec<&DecisionEntry> = entries
        .values()
        .filter(|e| superseded_by.contains_key(&e.number))
        .collect();

    if active.is_empty() {
        out.push_str("- *(none)*\n");
    } else {
        for (i, entry) in active.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            match &entry.title {
                Some(title) => out.push_str(&format!(
                    "### AD-{}: {} (#{} · {})\n",
                    entry.number, title, entry.seq, entry.from
                )),
                None => out.push_str(&format!(
                    "### AD-{} (#{} · {})\n",
                    entry.number, entry.seq, entry.from
                )),
            }
            if let Some(sup) = entry.supersedes {
                out.push_str(&format!("\n_Supersedes: AD-{sup}_\n"));
            }
            if !entry.body.is_empty() {
                out.push('\n');
                out.push_str(&entry.body);
                out.push('\n');
            }
        }
    }

    if !history.is_empty() {
        out.push_str("\n## History\n\n");
        for entry in history {
            let (by_num, by_seq) = superseded_by[&entry.number];
            let sum = summary_line(entry);
            if sum.is_empty() {
                out.push_str(&format!(
                    "- **AD-{}** (#{} · {}) — superseded by AD-{by_num} (#{by_seq})\n",
                    entry.number, entry.seq, entry.from
                ));
            } else {
                out.push_str(&format!(
                    "- **AD-{}** (#{} · {}) — superseded by AD-{by_num} (#{by_seq}): {sum}\n",
                    entry.number, entry.seq, entry.from
                ));
            }
        }
    }

    out
}
