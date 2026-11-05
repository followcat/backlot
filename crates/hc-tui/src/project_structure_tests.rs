use super::*;
use crate::sidebar::fixture::{term, view};
use serde_json::json;

fn doc(view: &mut View, name: &str, machine: Option<&str>, value: serde_json::Value) {
    view.documents.insert(
        hc_proto::watch::DocKey {
            machine: machine.map(str::to_owned),
            name: name.into(),
        },
        hc_proto::watch::Document {
            value,
            version: 1,
            ..Default::default()
        },
    );
}

#[test]
fn app_document_wins_labels_and_remote_ids_are_qualified_once() {
    let mut a = term("t1");
    a.labels.insert("herdr.space".into(), "wrong".into());
    let mut b = term("host-a/t1");
    b.labels.insert("herdr.space".into(), "wrong".into());
    let mut v = view(vec![a, b], vec![]);
    let structure = json!({"future_version":123,"spaces":[{"id":"s","name":"work","folder":"/work","tabs":[{"id":"tab","name":"task","root":{"type":"block","content":{"terminal":"t1"}}}]}]});
    doc(&mut v, "herdr-app", None, structure.clone());
    doc(&mut v, "herdr-app", Some("host-a"), structure);
    for selected in ["t1", "host-a/t1"] {
        let projected = project(&v, Some(selected));
        assert_eq!(projected.snapshot.workspaces.len(), 1);
        assert_eq!(projected.snapshot.workspaces[0].label, "work");
        assert_eq!(projected.snapshot.tabs[0].label, "task");
        assert_eq!(projected.snapshot.panes[0].pane_id, selected);
    }
}

#[test]
fn own_layout_preserves_new_members_and_prunes_cross_tab_or_stale_leaves() {
    let mut terms = vec![term("t1"), term("t2"), term("t3")];
    for t in &mut terms {
        t.labels.insert("herdr.space".into(), "s".into());
        t.labels.insert(
            "herdr.tab".into(),
            if t.address == "t3" { "other" } else { "tab" }.into(),
        );
    }
    let mut v = view(terms, vec![]);
    let base = project(&v, Some("t1"));
    let tab = base.snapshot.focused_tab_id.unwrap();
    let root = crate::structure::Tree::split(
        crate::structure::Tree::pane("t3"),
        crate::structure::Tree::pane("t1"),
        true,
        0.25,
    );
    let mut own = crate::structure::Document::default();
    own.tabs.insert(
        layout_key(None, &tab),
        crate::structure::TabLayout {
            root: Some(root),
            focused: Some("t1".into()),
            zoomed: Some("gone".into()),
            name: Some("renamed".into()),
            ..Default::default()
        },
    );
    doc(&mut v, "hc-tui", None, serde_json::to_value(own).unwrap());
    let projected = project(&v, Some("t1"));
    let layout = &projected.layouts[&tab];
    assert_eq!(layout.root.as_ref().unwrap().terminals(), ["t1", "t2"]);
    assert!(layout.zoomed.is_none());
    assert_eq!(
        projected
            .snapshot
            .tabs
            .iter()
            .find(|t| t.tab_id == tab)
            .unwrap()
            .label,
        "renamed"
    );
}

#[test]
fn adding_many_members_to_a_saved_layout_keeps_projection_bounded() {
    let terms = (1..=512)
        .map(|n| term(&format!("t{n}")))
        .collect::<Vec<_>>();
    let mut v = view(terms, vec![]);
    let tab = project(&v, Some("t1")).snapshot.focused_tab_id.unwrap();
    let key = layout_key(None, &tab);
    doc(
        &mut v,
        "hc-tui",
        None,
        json!({"schema":1,"tabs":{key:{"root":{"type":"pane","terminal":"t1"}}}}),
    );
    let projected = project(&v, Some("t1"));
    let root = projected.layouts[&tab].root.as_ref().unwrap();
    assert!(root.valid());
    assert_eq!(root.terminals().len(), 512);
}
