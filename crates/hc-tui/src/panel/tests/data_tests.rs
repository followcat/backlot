//! Reading `backlot` out of the view: soft failures only, and the
//! stage's live check.
use super::summary_json;
use crate::panel::{stage_running, summary};
use hc_proto::watch::{DocKey, Document, View};

fn view_with_document(value: serde_json::Value) -> View {
    let mut view = View::default();
    view.documents.insert(
        DocKey {
            machine: None,
            name: "backlot".into(),
        },
        Document {
            value,
            ..Document::default()
        },
    );
    view
}

#[test]
fn a_missing_document_is_no_studios() {
    assert!(summary(&View::default()).is_none());
}

#[test]
fn a_malformed_document_is_no_studios() {
    let view = view_with_document(serde_json::json!({"not": "a summary"}));
    assert!(summary(&view).is_none());
}

#[test]
fn a_wrong_schema_is_no_studios() {
    let mut value = summary_json();
    value["schema"] = "backlot/2".into();
    let view = view_with_document(value);
    assert!(summary(&view).is_none());
}

#[test]
fn a_machine_scoped_document_is_not_this_cores() {
    let mut view = View::default();
    view.documents.insert(
        DocKey {
            machine: Some("host-a".into()),
            name: "backlot".into(),
        },
        Document {
            value: summary_json(),
            ..Document::default()
        },
    );
    assert!(summary(&view).is_none());
}

#[test]
fn an_unknown_work_state_reads_as_waiting() {
    let mut value = summary_json();
    value["studios"][0]["roles"][0]["work"] = "teleporting".into();
    let view = view_with_document(value);
    let summary = summary(&view).expect("the rest still parses");
    let producer = summary.studios[0]
        .roles
        .iter()
        .find(|r| r.role == "producer")
        .unwrap();
    assert_eq!(producer.work, Some(backlot_model::summary::Work::Waiting));
}

#[test]
fn stage_running_needs_a_live_named_terminal() {
    let mut view = View::default();
    assert!(!stage_running(&view));
    let mut stage = crate::sidebar::fixture::term("t9");
    stage.name = Some("backlot-stage".into());
    stage.state = "live".into();
    view.inventory.replace(1, vec![stage.clone()]);
    assert!(stage_running(&view));
    // A finished stage terminal does not count.
    stage.state = "finished".into();
    view.inventory.replace(1, vec![stage]);
    assert!(!stage_running(&view));
}
