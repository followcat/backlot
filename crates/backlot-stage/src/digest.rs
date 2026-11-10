//! Merging a role's pending items into one digest (S9d): when two
//! or more items wait on the same send the stage writes
//! `messages/digest-<top seq>.md` and sends the one `[backlot]` line
//! that names it. Each item gets a heading with its label and seq —
//! a stage-written notice inlines the body it generated, a
//! role-written file (`tell`/`ask`/`dispatch`) and a report name
//! their path, and a file-less message shows its own send line.

use std::fmt::Write;
use std::fs;
use std::path::Path;

use backlot_model::state::{Message, ReportRecord};

use crate::notices;

/// One queued thing a role is owed — a queued message or a report.
#[derive(Debug, Clone, PartialEq)]
pub enum OutItem {
    Message(Message),
    Report(ReportRecord),
}

impl OutItem {
    /// The queued event's seq — what `message.delivered` targets.
    pub fn seq(&self) -> u64 {
        match self {
            OutItem::Message(m) => m.seq,
            OutItem::Report(r) => r.seq,
        }
    }

    /// The heading word — the notice's label for a stage-written
    /// file, the message's own purpose, `tell` for a person's file,
    /// `report` for a report.
    pub fn label(&self) -> String {
        match self {
            OutItem::Message(m) => notices::label(m)
                .map(str::to_string)
                .or_else(|| m.purpose.clone())
                .unwrap_or_else(|| "tell".into()),
            OutItem::Report(_) => "report".into(),
        }
    }
}

/// `messages/digest-<top seq>.md` — the file a merged send names,
/// stable across a resend after a crash.
pub fn file_name(items: &[OutItem]) -> String {
    let top = items.iter().map(OutItem::seq).max().unwrap_or(0);
    format!("messages/digest-{top}.md")
}

/// The digest body, items already in send order: one `## <label>
/// (seq <n>)` section each. A stage-written notice's body is inlined;
/// a role-written file and a report are named by path; a file-less
/// message shows the `[backlot]` line it would have sent alone.
pub fn body(items: &[OutItem], dir: &Path) -> String {
    let mut out = String::new();
    for it in items {
        let _ = writeln!(out, "## {} (seq {})\n", it.label(), it.seq());
        match it {
            OutItem::Message(m) => match (m.source_seq.is_some(), m.file.as_deref()) {
                // A stage-written notice: inline the body it wrote.
                (true, Some(f)) => match fs::read_to_string(dir.join(f)) {
                    Ok(text) => {
                        out.push_str(text.trim_end());
                        out.push('\n');
                    }
                    // The file is gone — name it instead of failing.
                    Err(_) => {
                        let _ = writeln!(out, "read {}", dir.join(f).display());
                    }
                },
                // A role-written file is named, never inlined.
                (_, Some(f)) => {
                    let _ = writeln!(out, "read {}", dir.join(f).display());
                }
                // A file-less message shows its own send line.
                _ => {
                    let _ = writeln!(
                        out,
                        "{}",
                        crate::deliver::line(&m.from, &m.to, None, m.purpose.as_deref(), dir)
                    );
                }
            },
            OutItem::Report(r) => {
                let _ = writeln!(out, "read {}", dir.join(&r.file).display());
            }
        }
        out.push('\n');
    }
    out
}
