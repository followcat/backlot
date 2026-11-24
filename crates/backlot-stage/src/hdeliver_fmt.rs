//! Two-lane formatting helpers and durable passive-context tracking
//! for Hollywood's inbox (`hdeliver.rs`).

use std::collections::BTreeSet;
use std::path::Path;

use backlot_model::hollywood::HMessage;

use crate::cli::Fail;
use crate::hnotice::{self, PassiveItem, PassiveKey};

/// Inline user-lane byte limit before overflowing to `messages/user-<top>.md`.
pub(crate) const PERSON_INLINE_BYTES: usize = 2048;
/// Preview length inside a `> 2 KiB` user-lane overflow line.
pub(crate) const USER_PREVIEW_CHARS: usize = 120;
/// Defer stage notices until the newest pending one is at least 5 s old.
pub(crate) const NOTICE_DEFER_MS: u64 = 5_000;
/// Inline single-notice byte limit before falling back to a digest file.
pub(crate) const NOTICE_INLINE_BYTES: usize = 400;

fn single_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn read_text(m: &HMessage, hdir: &Path) -> String {
    if let Some(t) = &m.text {
        t.clone()
    } else if let Some(f) = &m.file {
        std::fs::read_to_string(hdir.join(f)).unwrap_or_default()
    } else {
        String::new()
    }
}

/// The digest file name — `messages/digest-<top seq>.md` like the
/// studios'.
pub(crate) fn digest_name(items: &[&HMessage]) -> String {
    let top = items.iter().map(|m| m.seq).max().unwrap_or(0);
    format!("messages/digest-{top}.md")
}

/// The digest body: sections by label, each notice's file inlined,
/// a person's `tell` quoted as its line.
pub(crate) fn digest_body(items: &[&HMessage], hdir: &Path) -> String {
    let mut out = String::from("# backlot digest\n\n");
    for m in items {
        let head = m
            .file
            .as_deref()
            .and_then(|_| hnotice::label(m))
            .map(|l| format!("## {l} (seq {})\n\n", m.seq))
            .unwrap_or_else(|| format!("## message from {} (seq {})\n\n", m.from, m.seq));
        out.push_str(&head);
        if let Some(f) = &m.file {
            let text = std::fs::read_to_string(hdir.join(f)).unwrap_or_default();
            out.push_str(text.trim_end());
            out.push_str("\n\n");
        } else if let Some(t) = &m.text {
            out.push_str(&format!("{t}\n\n"));
        }
    }
    out
}

fn passive_tail(passive: &[PassiveItem]) -> String {
    if passive.is_empty() {
        return String::new();
    }
    let items: Vec<&str> = passive.iter().map(|p| p.text.as_str()).collect();
    format!(" · (context: {})", items.join(", "))
}

fn waiting_footer(notices: &[&HMessage]) -> String {
    if notices.is_empty() {
        return String::new();
    }
    let mut labels = Vec::new();
    for m in notices {
        let l = hnotice::label(m).unwrap_or("notice");
        if !labels.contains(&l) {
            labels.push(l);
        }
    }
    format!(
        " (+{} stage notices waiting: {})",
        notices.len(),
        labels.join(", ")
    )
}

/// Build the Person-lane payload (inline ` · `-joined <= 2 KiB, or file
/// overflow `messages/user-<top>.md` + 120-char latest preview), with
/// passive context and waiting notice footer.
pub(crate) fn person_payload(
    msgs: &[&HMessage],
    deferred_notices: &[&HMessage],
    passive: &[PassiveItem],
    hdir: &Path,
) -> Result<String, Fail> {
    let tail = passive_tail(passive);
    let footer = waiting_footer(deferred_notices);
    let inline = if msgs.len() == 1 {
        let body = single_line(&read_text(msgs[0], hdir));
        format!("[backlot] user → hollywood: {body}")
    } else {
        let numbered: Vec<String> = msgs
            .iter()
            .enumerate()
            .map(|(i, m)| format!("{}. {}", i + 1, single_line(&read_text(m, hdir))))
            .collect();
        format!(
            "[backlot] user → hollywood ({} messages, oldest to newest): {}",
            msgs.len(),
            numbered.join(" · ")
        )
    };
    let candidate = format!("{inline}{tail}{footer}");
    if candidate.len() <= PERSON_INLINE_BYTES {
        return Ok(candidate);
    }

    let top = msgs.iter().map(|m| m.seq).max().unwrap_or(0);
    let rel = format!("messages/user-{top}.md");
    let path = hdir.join(&rel);
    std::fs::create_dir_all(hdir.join("messages"))
        .map_err(|e| Fail::refused(format!("user inbox dir: {e}")))?;
    let mut md = format!("# user messages ({}, oldest to newest)\n\n", msgs.len());
    for (i, m) in msgs.iter().enumerate() {
        md.push_str(&format!("{}. {}\n\n", i + 1, read_text(m, hdir).trim_end()));
    }
    std::fs::write(&path, md).map_err(|e| Fail::refused(format!("user inbox file: {e}")))?;
    let latest_line = msgs
        .last()
        .map(|m| single_line(&read_text(m, hdir)))
        .unwrap_or_default();
    let preview: String = latest_line.chars().take(USER_PREVIEW_CHARS).collect();
    let noun = if msgs.len() == 1 {
        "message"
    } else {
        "messages"
    };
    Ok(format!(
        "[backlot] user → hollywood: read {} ({} {noun}; latest: \"{preview}\"){tail}{footer}",
        path.display(),
        msgs.len()
    ))
}

/// Build the Notice-lane payload (single notice inline <= 400 B, else
/// `messages/digest-<top>.md`), appending passive context if accrued.
pub(crate) fn notice_payload(
    notices: &[&HMessage],
    passive: &[PassiveItem],
    hdir: &Path,
) -> Result<String, Fail> {
    let tail = passive_tail(passive);
    if notices.len() == 1 {
        let m = notices[0];
        let l = hnotice::label(m).unwrap_or("notice");
        let raw = read_text(m, hdir);
        let flat = raw
            .lines()
            .map(|line| line.trim().trim_start_matches('#').trim())
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        let inline = format!("[backlot] stage → hollywood ({l}): {flat}");
        if inline.len() <= NOTICE_INLINE_BYTES {
            return Ok(format!("{inline}{tail}"));
        }
    }
    let file = digest_name(notices);
    let path = hdir.join(&file);
    std::fs::create_dir_all(hdir.join("messages"))
        .map_err(|e| Fail::refused(format!("digest dir: {e}")))?;
    std::fs::write(&path, digest_body(notices, hdir))
        .map_err(|e| Fail::refused(format!("digest file: {e}")))?;
    Ok(format!(
        "[backlot] stage → hollywood: read {} ({} items){tail}",
        path.display(),
        notices.len()
    ))
}

fn passive_file(hdir: &Path) -> std::path::PathBuf {
    hdir.join("passive-accounted.json")
}

pub(crate) fn load_passive(hdir: &Path, out: &mut BTreeSet<PassiveKey>) {
    let Ok(bytes) = std::fs::read_to_string(passive_file(hdir)) else {
        return;
    };
    if let Ok(list) = serde_json::from_str::<Vec<PassiveKey>>(&bytes) {
        out.extend(list);
    }
}

pub(crate) fn save_passive(hdir: &Path, keys: &BTreeSet<PassiveKey>) {
    if keys.is_empty() {
        return;
    }
    let _ = std::fs::create_dir_all(hdir);
    let list: Vec<&PassiveKey> = keys.iter().collect();
    if let Ok(data) = serde_json::to_string(&list) {
        let _ = std::fs::write(passive_file(hdir), data);
    }
}
