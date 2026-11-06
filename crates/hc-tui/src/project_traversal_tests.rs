//! `agent_traversal` must return the same order the shell resolves and
//! draws: hc-tui layout overrides, then the `herdr-app` document schema,
//! then the label/repository/cwd fallback — flattened in endpoint order.
//! The actor seam proves a resolver is consulted; these prove the UI
//! resolver itself yields the real draw precedence.

use super::*;
use crate::sidebar::fixture::{agent, machine, view};
use hc_proto::watch::{DocKey, Document as WatchedDocument};
use serde_json::json;

fn document(value: serde_json::Value) -> WatchedDocument {
    WatchedDocument {
        value,
        ..Default::default()
    }
}

fn block(terminal: &str) -> serde_json::Value {
    json!({ "type": "block", "content": { "terminal": terminal } })
}

/// The herdr-app schema: one space, one tab, `children` in draw order.
fn app_layout(children: Vec<serde_json::Value>) -> serde_json::Value {
    json!({
        "spaces": [{
            "id": "s1",
            "name": "Space",
            "folder": "/home/user/proj",
            "tabs": [{
                "id": "tab1",
                "name": "Tab",
                "root": { "type": "row", "children": children },
            }],
        }],
    })
}

#[test]
fn the_herdr_app_schema_orders_the_traversal_not_the_inventory() {
    // Inventory lists t1 first; the app document draws t2 first.
    let mut view = view(vec![agent("t1", "idle"), agent("t2", "idle")], vec![]);
    view.documents.insert(
        DocKey {
            machine: None,
            name: "herdr-app".into(),
        },
        document(app_layout(vec![block("t2"), block("t1")])),
    );
    assert_eq!(agent_traversal(&view), vec!["t2", "t1"]);
}

#[test]
fn the_own_layout_override_wins_over_the_herdr_app_schema() {
    // The app schema says t2 then t1; the stored hc-tui tab root reverses it.
    let mut view = view(vec![agent("t1", "idle"), agent("t2", "idle")], vec![]);
    view.documents.insert(
        DocKey {
            machine: None,
            name: "herdr-app".into(),
        },
        document(app_layout(vec![block("t2"), block("t1")])),
    );
    let tab_id = "space:2:s1/tab:4:tab1";
    let root = crate::structure::Tree::split(
        crate::structure::Tree::pane("t1"),
        crate::structure::Tree::pane("t2"),
        false,
        0.5,
    );
    view.documents.insert(
        DocKey {
            machine: None,
            name: "hc-tui".into(),
        },
        document(json!({
            "schema": 1,
            "tabs": {
                crate::project::layout_key(None, tab_id): { "root": root },
            },
        })),
    );
    assert_eq!(agent_traversal(&view), vec!["t1", "t2"]);
}

#[test]
fn without_documents_the_fallback_orders_by_created_then_terminal() {
    // Inventory order t9 then t3 disagrees with the fallback sort
    // (created_ms, term_num, address).
    let mut late_named = agent("t9", "idle");
    late_named.created_ms = 100;
    let mut early_named = agent("t3", "idle");
    early_named.created_ms = 100;
    let view = view(vec![late_named, early_named], vec![]);
    assert_eq!(agent_traversal(&view), vec!["t3", "t9"]);
}

#[test]
fn traversal_flattens_endpoints_in_project_order() {
    // Local first, then remote endpoints sorted by name — not the
    // inventory's key order.
    let view = view(
        vec![
            agent("t1", "idle"),
            agent("host-b/t1", "idle"),
            agent("host-a/t1", "idle"),
        ],
        vec![
            machine("host-a", "connected"),
            machine("host-b", "connected"),
        ],
    );
    assert_eq!(agent_traversal(&view), vec!["t1", "host-a/t1", "host-b/t1"]);
}
