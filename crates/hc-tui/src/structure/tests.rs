use super::*;
use serde_json::json;

fn terminal(id: &str) -> hc_proto::terminal::Terminal {
    crate::sidebar::fixture::term(id)
}

#[test]
fn foreign_versions_and_mod_nodes_keep_known_membership() {
    let a = terminal("t1");
    let b = terminal("t2");
    let c = terminal("t3");
    let value = json!({"version":999,"spaces":[{"id":"s","name":"work","folder":"/work","tabs":[{"id":"tab","name":"task","root":{"type":"row","children":[{"type":"block","content":{"mod":"clock"}},{"type":"block","content":{"terminal":"t2"}},{"type":"column","children":[{"type":"block","content":{"terminal":"t1"}},{"type":"block","content":{"terminal":"t2"}}]}]}}]}]});
    let spaces = read(&[&a, &b, &c], &value, None);
    assert_eq!(spaces.len(), 2);
    assert_eq!(spaces[0].name, "work");
    assert_eq!(spaces[0].tabs[0].root.terminals(), ["t2", "t1"]);
    assert_eq!(spaces[1].tabs[0].root.terminals(), ["t3"]);
}

#[test]
fn machine_local_ids_do_not_claim_another_machines_terminals() {
    let local = terminal("t1");
    let remote = terminal("host-a/t1");
    let value = json!({"spaces":[{"id":"s","folder":"/work/app","tabs":[{"id":"tab","root":{"type":"block","content":{"terminal":"t1"}}}]}]});
    let spaces = read(&[&remote], &value, Some("host-a"));
    assert_eq!(spaces[0].name, "app");
    assert_eq!(spaces[0].tabs[0].root.terminals(), ["host-a/t1"]);
    assert_eq!(
        read(&[&local], &value, None)[0].tabs[0].root.terminals(),
        ["t1"]
    );
}

#[test]
fn layout_rejects_unknown_schema_and_duplicate_or_invalid_trees() {
    assert!(Document::decode(json!({"schema":2})).is_err());
    let duplicate = Tree::split(Tree::pane("t1"), Tree::pane("t1"), false, 0.5);
    let invalid = Tree::split(Tree::pane("t1"), Tree::pane("t2"), false, 1.0);
    for root in [duplicate, invalid] {
        assert!(Document::decode(json!({"schema":1,"tabs":{"t":{"root":root}}})).is_err());
    }
    let value = json!({"schema":1,"future":{"keep":true},"tabs":{"t":{"future":[1,2,3]}}});
    assert_eq!(
        serde_json::to_value(Document::decode(value.clone()).unwrap()).unwrap()["future"],
        value["future"]
    );
    assert_eq!(
        serde_json::to_value(Document::decode(value.clone()).unwrap()).unwrap()["tabs"]["t"]["future"],
        value["tabs"]["t"]["future"]
    );
}

#[test]
fn pruned_tree_keeps_geometry_and_surviving_ratios() {
    let tree = Tree::split(
        Tree::pane("t1"),
        Tree::split(Tree::pane("t2"), Tree::pane("t3"), true, 0.25),
        false,
        0.6,
    );
    let kept = tree.retain(&|id| id != "t2").unwrap();
    let (layout, ids) = kept.layout(Some("t3"));
    assert_eq!(Tree::from_layout(&layout, &ids).unwrap(), kept);
    let panes = layout.panes(ratatui::layout::Rect::new(0, 0, 100, 40));
    assert_eq!(panes[0].rect.width, 60);
    assert_eq!(panes[1].rect.width, 40);
    assert!(panes[1].is_focused);
}

#[test]
fn newly_labelled_terminal_joins_document_space_and_tab_before_app_adoption() {
    let mut a = terminal("t1");
    let mut b = terminal("t2");
    for term in [&mut a, &mut b] {
        term.labels.insert("herdr.space".into(), "s".into());
        term.labels.insert("herdr.tab".into(), "tab".into());
    }
    let value = json!({"spaces":[{"id":"s","name":"renamed","tabs":[{"id":"tab","name":"task","root":{"type":"block","content":{"terminal":"t1"}}}]}]});
    let spaces = read(&[&a, &b], &value, None);
    assert_eq!(spaces.len(), 1);
    assert_eq!(spaces[0].name, "renamed");
    assert_eq!(spaces[0].tabs.len(), 1);
    assert_eq!(spaces[0].tabs[0].root.terminals(), ["t1", "t2"]);
}

#[test]
fn restored_ids_remain_unique_through_split_and_close() {
    let (mut layout, mut ids) = Tree::pane("t1").layout(None);
    let first = layout.focused();
    let second = layout
        .split_pane(first, ratatui::layout::Direction::Horizontal, 0.5)
        .unwrap();
    assert_ne!(first, second);
    ids.insert(second, "t2".into());
    assert_eq!(
        Tree::from_layout(&layout, &ids).unwrap().terminals(),
        ["t1", "t2"]
    );
    assert!(layout.close_pane(first));
    assert_eq!(
        Tree::from_layout(&layout, &ids).unwrap().terminals(),
        ["t2"]
    );
}

#[test]
fn unknown_owned_node_fields_block_writes_without_losing_them() {
    assert!(
        Document::decode(
            json!({"schema":1,"tabs":{"t":{"root":{"type":"pane","terminal":"t1","future":42}}}})
        )
        .is_err()
    );
}

#[test]
fn large_rows_and_label_fallbacks_produce_bounded_valid_trees() {
    let terms = (1..=512)
        .map(|n| terminal(&format!("t{n}")))
        .collect::<Vec<_>>();
    let refs = terms.iter().collect::<Vec<_>>();
    let children = terms
        .iter()
        .map(|t| json!({"type":"block","content":{"terminal":t.address},"size":{"weight":1e308}}))
        .collect::<Vec<_>>();
    let doc = json!({"spaces":[{"id":"s","tabs":[{"id":"tab","root":{"type":"row","children":children}}]}]});
    for value in [doc, Value::Null] {
        let spaces = read(&refs, &value, None);
        let root = &spaces[0].tabs[0].root;
        assert_eq!(root.terminals().len(), 512);
        assert!(root.valid());
        let (layout, _) = root.layout(None);
        assert_eq!(
            layout.panes(ratatui::layout::Rect::new(0, 0, 512, 40))[0]
                .rect
                .width,
            1
        );
    }
    let (mut layout, _) = Tree::split(Tree::pane("t1"), Tree::pane("t2"), false, 0.5).layout(None);
    assert!(!layout.set_ratio_at(&[], f32::NAN));
    assert_eq!(
        layout.panes(ratatui::layout::Rect::new(0, 0, 100, 40))[0]
            .rect
            .width,
        50
    );
}

#[test]
fn more_than_4096_fallback_terminals_and_underflow_weights_remain_valid() {
    let terms = (1..=4097)
        .map(|n| terminal(&format!("t{n}")))
        .collect::<Vec<_>>();
    let refs = terms.iter().collect::<Vec<_>>();
    let spaces = read(&refs, &Value::Null, None);
    assert!(spaces[0].tabs[0].root.valid());
    assert_eq!(spaces[0].tabs[0].root.terminals().len(), 4097);
    let children = terms[..4]
        .iter()
        .zip([1e-308, 1e-308, 1e308, 1e308])
        .map(|(t, w)| json!({"type":"block","content":{"terminal":t.address},"size":{"weight":w}}))
        .collect::<Vec<_>>();
    let doc = json!({"spaces":[{"id":"s","tabs":[{"id":"tab","root":{"type":"row","children":children}}]}]});
    let spaces = read(&refs[..4], &doc, None);
    let root = &spaces[0].tabs[0].root;
    assert!(root.valid());
    let (layout, _) = root.layout(None);
    let panes = layout.panes(ratatui::layout::Rect::new(0, 0, 1000, 40));
    assert!(panes[0].rect.width <= 1);
    assert!(panes[2].rect.width >= 499);
}
