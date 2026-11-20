//! Reading `backlot` out of the view: soft failures only, the
//! per-machine aggregate and the stage's live check.
use super::summary_json;
use crate::panel::data::{local_summary, stage_running, summaries};
use hc_proto::watch::{DocKey, Document, View};

fn view_with_document(value: serde_json::Value) -> View {
    let mut view = View::default();
    view.documents_mut().insert(
        DocKey {
            machine: None,
            name: "backlot".into(),
        },
        Document {
            value,
            ..Document::default()
        }
        .into(),
    );
    view
}

#[test]
fn a_missing_document_is_no_studios() {
    assert!(summaries(&View::default()).is_empty());
}

#[test]
fn an_empty_machines_answer_reads_as_no_added_machines() {
    // 0.1.21 and later always send `terminal.list`'s `machines`; an
    // empty list means "no machines", not "no information". The
    // studio's list is the local document either way, and an absent
    // list (an older core) reads the same.
    let mut answered = view_with_document(summary_json());
    answered.machines = Some(Vec::new());
    let machines = summaries(&answered);
    assert_eq!(machines.len(), 1);
    assert_eq!(machines[0].machine, None);
    let unanswered = view_with_document(summary_json());
    assert_eq!(summaries(&unanswered).len(), 1);
    // The panel's rows say "no studios" only when no document parsed —
    // never an error row for the empty answer.
    let rows = crate::panel::rows::rows(
        &machines,
        |_| true,
        |_| true,
        &crate::panel::Panel::new(),
        crate::panel::PanelMode::Collapsed,
        None,
        24,
    );
    assert!(
        rows.iter().all(|row| matches!(
            row,
            crate::panel::rows::Row::Title
                | crate::panel::rows::Row::Strip { .. }
                | crate::panel::rows::Row::StripRole { .. }
        )),
        "{rows:?}"
    );
}

#[test]
fn a_malformed_document_is_no_studios() {
    let view = view_with_document(serde_json::json!({"not": "a summary"}));
    assert!(summaries(&view).is_empty());
}

#[test]
fn a_wrong_schema_is_no_studios() {
    let mut value = summary_json();
    value["schema"] = "backlot/2".into();
    let view = view_with_document(value);
    assert!(summaries(&view).is_empty());
}

#[test]
fn a_machine_scoped_document_is_not_this_cores() {
    let mut view = View::default();
    view.documents_mut().insert(
        DocKey {
            machine: Some("host-a".into()),
            name: "backlot".into(),
        },
        Document {
            value: summary_json(),
            ..Document::default()
        }
        .into(),
    );
    // The document never counts as this core's — it lands as the
    // machine's own row even without a `machine.list` entry.
    assert!(local_summary(&view).is_none());
    let machines = summaries(&view);
    assert_eq!(machines.len(), 1);
    assert_eq!(machines[0].machine.as_deref(), Some("host-a"));
}

#[test]
fn an_unknown_work_state_reads_as_waiting() {
    let mut value = summary_json();
    value["studios"][0]["roles"][0]["work"] = "teleporting".into();
    let view = view_with_document(value);
    let summary = local_summary(&view).expect("the rest still parses");
    let producer = summary.studios[0]
        .roles
        .iter()
        .find(|r| r.role == "producer")
        .unwrap();
    assert_eq!(producer.work, Some(backlot_model::summary::Work::Waiting));
}

#[test]
fn a_remote_document_qualifies_its_role_terminals() {
    let mut view = View::default();
    view.documents_mut().insert(
        DocKey {
            machine: Some("host-b".into()),
            name: "backlot".into(),
        },
        Document {
            value: summary_json(),
            ..Document::default()
        }
        .into(),
    );
    let machines = summaries(&view);
    let producer = &machines[0].summary.studios[0].roles[0];
    assert_eq!(producer.terminal.as_deref(), Some("host-b/t1"));
}

#[test]
fn stage_running_needs_a_live_named_terminal() {
    let mut view = View::default();
    assert!(!stage_running(&view, None));
    let mut stage = crate::sidebar::fixture::term("t9");
    stage.name = Some("backlot-stage".into());
    stage.state = "live".into();
    view.inventory_mut().replace(1, vec![stage.clone()]);
    assert!(stage_running(&view, None));
    // A finished stage terminal does not count.
    stage.state = "finished".into();
    view.inventory_mut().replace(1, vec![stage]);
    assert!(!stage_running(&view, None));
}

#[test]
fn stage_running_is_per_machine() {
    let mut view = View::default();
    let mut remote = crate::sidebar::fixture::term("host-b/t9");
    remote.name = Some("backlot-stage".into());
    remote.state = "live".into();
    view.inventory_mut().replace(1, vec![remote]);
    assert!(stage_running(&view, Some("host-b")));
    assert!(!stage_running(&view, None));
    assert!(!stage_running(&view, Some("host-a")));
}
