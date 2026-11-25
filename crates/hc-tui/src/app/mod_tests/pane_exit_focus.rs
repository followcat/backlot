//! A pane's program exits: focus stays in its tab, on a sibling pane.

use super::*;

fn pane(address: &str, space: &str) -> hc_proto::terminal::Terminal {
    let mut t = crate::sidebar::fixture::term(address);
    t.labels.insert("herdr.space".into(), space.into());
    t.labels.insert("herdr.tab".into(), "tab".into());
    t
}

fn set_layout(app: &mut App, second_tab: serde_json::Value, version: u64) {
    let block = serde_json::json!({"type": "block", "content": {"terminal": "t0"}});
    let value = serde_json::json!({"spaces": [
        {"id": "first", "name": "first", "folder": "/a",
         "tabs": [{"id": "tab", "name": "tab", "root": block}]},
        {"id": "second", "name": "second", "folder": "/b",
         "tabs": [{"id": "tab", "name": "tab", "root": second_tab}]},
    ]});
    let key = hc_proto::watch::DocKey {
        machine: None,
        name: "herdr-app".into(),
    };
    let doc = hc_proto::watch::Document {
        value,
        version,
        ..Default::default()
    };
    app.view.documents_mut().insert(key, Arc::new(doc));
}

#[tokio::test]
async fn an_exited_pane_hands_focus_to_its_sibling() {
    let block = |t: &str| serde_json::json!({"type": "block", "content": {"terminal": t}});
    let mut app = app_with_terms(vec![
        pane("t0", "first"),
        pane("t1", "second"),
        pane("t2", "second"),
    ]);
    set_layout(
        &mut app,
        serde_json::json!({"type": "row", "children": [block("t1"), block("t2")]}),
        1,
    );
    app.focused = Some("t2".into());
    app.refresh_projection();

    // t2's shell exits: the layout drops it, the inventory keeps it as finished.
    let mut finished = pane("t2", "second");
    finished.state = "finished".into();
    app.view.inventory_mut().replace(
        10,
        vec![pane("t0", "first"), pane("t1", "second"), finished],
    );
    set_layout(&mut app, block("t1"), 2);
    app.mode = Mode::Normal;
    app.reconcile_published_view();

    assert_eq!(app.focused.as_deref(), Some("t1"));
}
