use super::*;
use crate::sidebar::fixture::{term, view};
use hc_proto::watch::{DocKey, Document, View};
use serde_json::json;
use std::sync::Arc;

/// One workspace holding `t1` on `tab-a` and `t2` on `tab-b`.
fn two_tab_view() -> View {
    let mut t1 = term("t1");
    t1.labels.insert("herdr.space".into(), "space-a".into());
    t1.labels.insert("herdr.tab".into(), "tab-a".into());
    let mut t2 = term("t2");
    t2.labels.insert("herdr.space".into(), "space-a".into());
    t2.labels.insert("herdr.tab".into(), "tab-b".into());
    view(vec![t1, t2], vec![])
}

fn snap(view: &View, selected: Option<&str>) -> crate::chrome::snapshot::ClientShellSnapshot {
    crate::project::project(view, selected).snapshot
}

#[test]
fn identical_projections_do_not_change_focus_or_order() {
    let view = two_tab_view();
    let a = snap(&view, Some("t1"));
    let b = snap(&view, Some("t1"));
    assert!(!tab_focus_or_order_changed(&a, &b));
}

#[test]
fn a_focused_tab_switch_is_a_change() {
    let view = two_tab_view();
    let a = snap(&view, Some("t1"));
    let b = snap(&view, Some("t2"));
    assert!(tab_focus_or_order_changed(&a, &b));
}

#[test]
fn a_tab_order_flip_under_a_stable_focus_is_a_change() {
    let mut view = two_tab_view();
    let a = snap(&view, Some("t1"));
    // The shared layout document's `order` flips the tab bar; the
    // focused pane — and its tab — stays put.
    let workspace_id = a.workspaces[0].workspace_id.clone();
    let order: Vec<String> = a.tabs.iter().rev().map(|tab| tab.tab_id.clone()).collect();
    view.documents_mut().insert(
        DocKey {
            machine: None,
            name: "hc-tui".into(),
        },
        Arc::new(Document {
            value: json!({
                "schema": 1,
                "order": { crate::project::layout_key(None, &workspace_id): order },
            }),
            version: 2,
            ..Default::default()
        }),
    );
    let b = snap(&view, Some("t1"));
    assert_eq!(a.focused_tab_id, b.focused_tab_id);
    assert!(tab_focus_or_order_changed(&a, &b));
}
