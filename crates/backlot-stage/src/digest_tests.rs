//! The digest file's contents: items in the order `outbox` gave
//! them, one `## <label> (seq <n>)` section each — a stage-written
//! notice's body is inlined, a role-written file and a report name
//! their path, a file-less message shows the line it would have
//! sent on its own.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use backlot_model::state::{Message, ReportRecord};

use crate::digest::{self, OutItem};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn tmp() -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "backlot-digest-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&d).unwrap();
    d
}

fn message(
    seq: u64,
    from: &str,
    file: Option<&str>,
    purpose: Option<&str>,
    source_seq: Option<u64>,
) -> Message {
    Message {
        seq,
        from: from.into(),
        to: "director".into(),
        file: file.map(str::to_string),
        purpose: purpose.map(str::to_string),
        source_seq,
        delivered: false,
        dropped: false,
        ts_ms: 0,
        delivered_ms: None,
    }
}

fn report(seq: u64) -> ReportRecord {
    ReportRecord {
        seq,
        from: "writer".into(),
        to: "director".into(),
        node: "revise".into(),
        file: format!("reports/{seq}-revise.md"),
        delivered: false,
        ts_ms: 0,
        delivered_ms: None,
    }
}

/// The file is `messages/digest-<top seq>.md` — the highest seq in
/// the set, so a resend after a crash names the same path.
#[test]
fn the_digest_name_uses_the_top_seq() {
    let items = vec![
        OutItem::Message(message(4, "stage", Some("messages/3-a.md"), None, Some(3))),
        OutItem::Message(message(9, "stage", Some("messages/8-b.md"), None, Some(8))),
    ];
    assert_eq!(digest::file_name(&items), "messages/digest-9.md");
}

/// Each item's heading carries its label — the notice's own label,
/// a person's `tell`, a purpose word, `report` for a report — and
/// its seq; sections land in the order the items were given.
#[test]
fn digest_sections_carry_label_and_seq_in_order() {
    let dir = tmp();
    let items = vec![
        OutItem::Message(message(
            8,
            "stage",
            Some("messages/4-attention.md"),
            None,
            Some(4),
        )),
        OutItem::Message(message(9, "writer", Some("messages/9-note.md"), None, None)),
        OutItem::Message(message(11, "stage", None, Some("plan"), None)),
        OutItem::Report(report(12)),
    ];
    let body = digest::body(&items, &dir);
    let at = |s: &str| {
        body.find(s)
            .unwrap_or_else(|| panic!("{s} missing from\n{body}"))
    };
    let (a, b, c, d) = (
        at("## attention (seq 8)"),
        at("## tell (seq 9)"),
        at("## plan (seq 11)"),
        at("## report (seq 12)"),
    );
    assert!(a < b && b < c && c < d, "{body}");
}

/// A stage-written notice inlines the body it generated; a
/// person's file and a report name their paths; a file-less
/// message shows its own send line.
#[test]
fn digest_inlines_notices_and_names_files() {
    let dir = tmp();
    fs::create_dir_all(dir.join("messages")).unwrap();
    fs::write(
        dir.join("messages/4-attention.md"),
        "# t2 asks\n\n`t2` needs the person.\n",
    )
    .unwrap();
    let items = vec![
        OutItem::Message(message(
            8,
            "stage",
            Some("messages/4-attention.md"),
            None,
            Some(4),
        )),
        OutItem::Message(message(9, "writer", Some("messages/9-note.md"), None, None)),
        OutItem::Message(message(11, "stage", None, Some("plan"), None)),
        OutItem::Report(report(12)),
    ];
    let body = digest::body(&items, &dir);
    let here = dir.display().to_string();
    assert!(
        body.contains("`t2` needs the person."),
        "the notice body is inlined\n{body}"
    );
    assert!(
        body.contains(&format!("read {here}/messages/9-note.md")),
        "a tell file is named\n{body}"
    );
    assert!(
        body.contains(&format!("read {here}/reports/12-revise.md")),
        "a report is named\n{body}"
    );
    assert!(
        body.contains("a plan and an acceptance list are wanted"),
        "a file-less message shows its line\n{body}"
    );
}

/// A notice whose file vanished falls back to naming the path —
/// a missing body never fails the send.
#[test]
fn a_missing_notice_file_falls_back_to_its_path() {
    let dir = tmp();
    let items = vec![OutItem::Message(message(
        8,
        "stage",
        Some("messages/4-lost.md"),
        None,
        Some(4),
    ))];
    let body = digest::body(&items, &dir);
    assert!(
        body.contains(&format!("read {}/messages/4-lost.md", dir.display())),
        "{body}"
    );
}
