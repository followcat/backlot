//! Handoff package (`<hdir>/handoffs/<id>.md`) construction, size caps,
//! unanswered-tell detection, mode 0600 persistence, and probation prompt (§2.3.2).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use backlot_model::hollywood::{CardState, HollywoodState};
use backlot_model::state::StudioState;
use backlot_model::summary::open_ask;

use crate::hrender::hollywood_block;
use crate::live::LiveView;
use crate::render::{attention_fact, status as render_studio_status};
use crate::transcript::{ExtractedTail, cap_chars, redact_secrets, trim_newest_bytes};

pub(crate) const MAX_BRIEF_BYTES: usize = 32 * 1024;
pub(crate) const MAX_MEMORY_BYTES: usize = 4 * 1024;

pub(crate) struct HandoffBriefInput<'a> {
    pub id: &'a str,
    pub hdir: &'a Path,
    pub now_ms: u64,
    pub from_actor: &'a str,
    pub to_actor: &'a str,
    pub trigger: &'a str,
    pub studios: &'a BTreeMap<String, StudioState>,
    pub live: &'a LiveView,
    pub hstate: &'a HollywoodState,
    pub tail: &'a ExtractedTail,
}

/// Build the markdown brief written to `<hdir>/handoffs/<id>.md`, capped at 32 KiB.
pub(crate) fn build_handoff_brief(input: &HandoffBriefInput<'_>) -> String {
    let header = format!(
        concat!(
            "# Hollywood Handoff Brief `{}`\n\n",
            "- timestamp_ms: {}\n",
            "- from: `{}`\n",
            "- to: `{}`\n",
            "- trigger: {}\n",
            "- Transcript source: {}\n"
        ),
        input.id,
        input.now_ms,
        input.from_actor,
        input.to_actor,
        input.trigger,
        input.tail.provenance
    );

    let ledger = build_ledger_snapshot(
        input.hdir,
        input.studios,
        input.live,
        input.hstate,
        input.now_ms,
    );
    let cards = build_cards_section(input.hstate);
    let messages = build_messages_section(input.hdir, input.hstate);
    let memory = build_memory_section(input.hdir);

    let mut fixed = String::new();
    fixed.push_str(&header);
    fixed.push_str("\n## Studio Ledger Snapshot (`status --all`)\n\n");
    fixed.push_str(&ledger);
    fixed.push_str("\n## Open & Applied Cards\n\n");
    fixed.push_str(&cards);
    fixed.push_str("\n## Queued & Unanswered Messages\n\n");
    fixed.push_str(&messages);
    fixed.push_str("\n## Memory (`memory.md`)\n\n");
    fixed.push_str(&memory);

    let tail_header = "\n## Old Conversation Tail\n\n";
    let overhead = fixed.len() + tail_header.len();
    if overhead >= MAX_BRIEF_BYTES {
        return cap_bytes(&fixed, MAX_BRIEF_BYTES);
    }
    let tail_budget = MAX_BRIEF_BYTES - overhead;
    let tail_body = trim_newest_bytes(&input.tail.markdown, tail_budget);
    cap_bytes(
        &format!("{fixed}{tail_header}{tail_body}\n"),
        MAX_BRIEF_BYTES,
    )
}

fn cap_bytes(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

fn build_ledger_snapshot(
    hdir: &Path,
    studios: &BTreeMap<String, StudioState>,
    live: &LiveView,
    hstate: &HollywoodState,
    now_ms: u64,
) -> String {
    let mut out = String::new();
    if studios.is_empty() {
        out.push_str("_No active studios._\n");
    } else {
        for (name, st) in studios {
            let empty_failed = BTreeMap::new();
            out.push_str("```text\n");
            out.push_str(&render_studio_status(name, st, &empty_failed, now_ms, live));
            out.push_str("```\n");
            for a in st.open_attention() {
                out.push_str(&format!(
                    "- attention `{}` on `{}` (`{}`): {} ({})\n",
                    a.reason,
                    name,
                    a.role,
                    cap_chars(&redact_secrets(&a.detail), 240),
                    attention_fact(a, now_ms)
                ));
            }
            for role in st.roles.keys() {
                if let Some(ask) = open_ask(st, role) {
                    let body = ask
                        .file
                        .as_deref()
                        .and_then(|rel| {
                            let sdir = hdir.parent().map(|b| crate::store::studio_dir(b, name))?;
                            fs::read_to_string(sdir.join(rel)).ok()
                        })
                        .or_else(|| ask.file.clone())
                        .unwrap_or_else(|| "(ask body unavailable)".into());
                    out.push_str(&format!(
                        "- unanswered `tell user --ask` from `{name}/{role}` (seq {}): {}\n",
                        ask.seq,
                        cap_chars(&redact_secrets(body.trim()), 240)
                    ));
                }
            }
        }
    }
    out.push_str("```text\n");
    out.push_str(&hollywood_block(hstate, &Ok(None)));
    out.push_str("```\n");
    out
}

fn build_cards_section(hstate: &HollywoodState) -> String {
    let active: Vec<_> = hstate
        .cards
        .values()
        .filter(|c| matches!(c.state, CardState::Open | CardState::Applied))
        .collect();
    if active.is_empty() {
        return "_No open or applied cards._\n".to_string();
    }
    let mut out = String::new();
    for c in active {
        out.push_str(&format!(
            "- **c{}** [{}] studio=`{}` — {} (why: {})\n",
            c.seq,
            c.state.as_str(),
            c.action.studio(),
            c.action.label(),
            cap_chars(&redact_secrets(&c.why), 240)
        ));
    }
    out
}

fn build_messages_section(hdir: &Path, hstate: &HollywoodState) -> String {
    let mut lines = Vec::new();
    for m in &hstate.messages {
        if m.to != "hollywood" {
            continue;
        }
        let queued = !m.delivered && !m.dropped && !m.seen;
        if queued && (m.from == "user" || m.from == "stage") {
            let body = read_message_excerpt(hdir, m);
            lines.push(format!(
                "- [seq {} from `{}`] {} (queued — stage will deliver after handoff commits)",
                m.seq, m.from, body
            ));
            continue;
        }
        if m.from == "user" && m.delivered {
            let answered = hstate.reports.iter().any(|r| r.seq > m.seq)
                || hstate.cards.values().any(|c| c.seq > m.seq);
            if !answered {
                let body = read_message_excerpt(hdir, m);
                lines.push(format!(
                    "- [seq {} from `user`] {} (sent to previous agent without a reply — verify and answer once promoted)",
                    m.seq, body
                ));
            }
        }
    }
    if lines.is_empty() {
        "_No queued or unanswered user messages._\n".to_string()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

fn read_message_excerpt(hdir: &Path, m: &backlot_model::hollywood::HMessage) -> String {
    if let Some(rel) = m.file.as_deref()
        && let Ok(text) = fs::read_to_string(hdir.join(rel))
    {
        return cap_chars(&redact_secrets(text.trim()), 400);
    }
    if let Some(text) = m.text.as_deref() {
        return cap_chars(&redact_secrets(text.trim()), 400);
    }
    "(empty body)".to_string()
}

fn build_memory_section(hdir: &Path) -> String {
    let path = hdir.join("memory.md");
    let Ok(raw) = fs::read_to_string(&path) else {
        return format!("Path: `{}` (not yet created).\n", path.display());
    };
    if raw.len() <= MAX_MEMORY_BYTES {
        format!(
            "Path: `{}`\n\n```markdown\n{}\n```\n",
            path.display(),
            raw.trim_end()
        )
    } else {
        let tail = trim_newest_bytes(&raw, MAX_MEMORY_BYTES.saturating_sub(64));
        format!(
            "Path: `{}` (showing tail <= 4 KiB; read full file if needed)\n\n```markdown\n{}\n```\n",
            path.display(),
            tail.trim_end()
        )
    }
}

/// Write `<hdir>/handoffs/<id>.md` atomically with mode `0600`.
pub(crate) fn write_handoff_brief(hdir: &Path, id: &str, body: &str) -> Result<PathBuf, String> {
    let dir = hdir.join("handoffs");
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
    }
    let path = dir.join(format!("{id}.md"));
    let tmp = dir.join(format!(".{id}.md.tmp"));
    write_private(&tmp, body.as_bytes()).map_err(|e| format!("{}: {e}", tmp.display()))?;
    fs::rename(&tmp, &path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })?;
    Ok(path)
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    fs::write(path, bytes)
}

/// First prompt injected into the candidate Hollywood terminal during probation (§2.3.2).
pub(crate) fn candidate_first_prompt(
    hdir: &Path,
    id: &str,
    old_runner: &str,
    old_model: &str,
    new_runner: &str,
    new_model: &str,
) -> String {
    let agents = hdir.join("AGENTS.md");
    let brief = hdir.join("handoffs").join(format!("{id}.md"));
    format!(
        concat!(
            "Read {} and the handoff brief at {}.\n",
            "You are taking over as Hollywood from {}/{}.\n",
            "FIRST: verify you can read the brief and run `backlot status --all`, then confirm\n",
            "readiness with:\n",
            "  backlot hollywood report \"Handoff ready — switched to {}/{}\"\n",
            "Do NOT propose cards or tell studio producers until you have sent that readiness report.\n",
            "Once ready, any queued user messages will be delivered to you automatically."
        ),
        agents.display(),
        brief.display(),
        old_runner,
        old_model,
        new_runner,
        new_model
    )
}
