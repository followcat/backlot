//! S3 §2 dialog-model tests — the wording every surface shares and
//! the multi-line body editor Tell/Revise use.

use super::{BodyEditor, StudioDialog};
use crate::studio::StudioKey;

fn key(machine: Option<&str>) -> StudioKey {
    StudioKey {
        machine: machine.map(str::to_owned),
        id: "acme-shop".into(),
    }
}

#[test]
fn place_names_the_machine_or_local() {
    assert_eq!(
        StudioDialog::place(&key(Some("host-b"))),
        "acme-shop · host-b"
    );
    assert_eq!(StudioDialog::place(&key(None)), "acme-shop · Local");
}

#[test]
fn the_dir_override_confirm_uses_the_spec_wording() {
    let StudioDialog::Confirm(d) = StudioDialog::dir_override(&key(None), "alice", true) else {
        panic!("a confirm");
    };
    assert_eq!(d.title, "Answer alice's prompt? · acme-shop · Local");
    assert!(d.body.contains("routed to the director"), "{}", d.body);
    assert!(d.body.contains("allow"), "{}", d.body);
    let StudioDialog::Confirm(deny) = StudioDialog::dir_override(&key(None), "alice", false) else {
        panic!("a confirm");
    };
    assert!(deny.body.contains("deny"), "{}", deny.body);
}

#[test]
fn the_body_editor_edits_and_joins_lines() {
    let mut e = BodyEditor::default();
    e.insert("hello");
    e.insert("\nworld");
    assert_eq!(e.text(), "hello\nworld");
    use crossterm::event::{KeyCode, KeyModifiers};
    // A mid-line Enter splits the row at the cursor.
    e.row = 0;
    e.col = 2;
    e.key(KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(e.text(), "he\nllo\nworld");
    // Backspace at a row's start merges it onto the one above.
    e.row = 1;
    e.col = 0;
    e.key(KeyCode::Backspace, KeyModifiers::NONE);
    assert_eq!(e.text(), "hello\nworld");
    // A prefill seeds the rows verbatim.
    let e = BodyEditor::new("line one\nline two\n");
    assert_eq!(e.lines.len(), 2);
    assert_eq!(e.text(), "line one\nline two");
}

#[test]
fn a_reason_dialog_marks_whether_empty_is_refused() {
    let StudioDialog::Reason(required) = StudioDialog::reason(
        "Pause task t1".into(),
        true,
        super::ReasonFor::TaskVerb {
            key: key(None),
            task: "t1".into(),
            verb: "pause",
        },
    ) else {
        panic!("a reason dialog");
    };
    assert!(required.required, "pause refuses an empty reason");
    let StudioDialog::Reason(optional) = StudioDialog::reason(
        "Finish studio".into(),
        false,
        super::ReasonFor::Finish { key: key(None) },
    ) else {
        panic!("a reason dialog");
    };
    assert!(!optional.required, "finish's reason stays optional");
}
