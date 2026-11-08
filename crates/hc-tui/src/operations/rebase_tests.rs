use super::tests::{edit, hello, next, request, store};
use super::*;
use crate::{
    structure::{Adoption, TabLayout, Tree},
    testcore,
};
use serde_json::json;

#[test]
fn edits_merge_published_members_but_do_not_resurrect_a_concurrent_close() {
    let mut mutation = edit(Change::Focus("t2".into()));
    mutation.fallback = Tree::split(Tree::pane("t1"), Tree::pane("t2"), false, 0.5);
    mutation.incarnations.insert("t2".into(), 2);
    let mut document = Document::default();
    document.tabs.insert(
        mutation.tab.clone(),
        TabLayout {
            root: Some(Tree::pane("t1")),
            ..Default::default()
        },
    );
    mutation.apply(&mut document).unwrap();
    assert_eq!(document.focus.as_deref(), Some("t2"));
    super::edit::hide(&mut document, "t2", 2);
    assert!(mutation.apply(&mut document).is_err());
    mutation.incarnations.insert("t2".into(), 3);
    mutation.apply(&mut document).unwrap();
    assert_eq!(document.focus.as_deref(), Some("t2"));
}

#[test]
fn stale_client_edits_follow_adoption_aliases() {
    let mutation = edit(Change::Name("renamed".into()));
    let mut document = Document::default();
    document.adoptions.insert(
        mutation.space.clone(),
        Adoption {
            space: "chosen".into(),
            tabs: BTreeMap::from([(mutation.tab.clone(), "tab-id".into())]),
            ..Default::default()
        },
    );
    let chosen = "0:/space:6:chosen/tab:6:tab-id";
    document.tabs.insert(
        chosen.into(),
        TabLayout {
            root: Some(Tree::pane("t1")),
            ..Default::default()
        },
    );
    mutation.apply(&mut document).unwrap();
    assert_eq!(document.tabs[chosen].name.as_deref(), Some("renamed"));
    assert!(!document.tabs.contains_key(&mutation.tab));
}

#[test]
fn collapsed_boundary_cannot_receive_an_old_path_ratio() {
    let original = Tree::split(
        Tree::pane("t1"),
        Tree::split(Tree::pane("t2"), Tree::pane("t3"), true, 0.5),
        false,
        0.5,
    );
    let mut mutation = edit(Change::Ratio {
        expected: original.clone(),
        path: vec![],
        ratio: 0.7,
    });
    mutation.fallback = original;
    let mut document = Document::default();
    document.closed.insert("t1".into(), 1);
    assert!(
        mutation
            .apply(&mut document)
            .unwrap_err()
            .contains("topology")
    );
}

#[test]
fn closing_focus_uses_its_own_tab_before_lexically_earlier_tabs() {
    let mut document = Document {
        focus: Some("t2".into()),
        ..Default::default()
    };
    document.tabs.insert(
        "a".into(),
        TabLayout {
            root: Some(Tree::pane("t1")),
            focused: Some("t1".into()),
            ..Default::default()
        },
    );
    document.tabs.insert(
        "z".into(),
        TabLayout {
            root: Some(Tree::split(Tree::pane("t2"), Tree::pane("t3"), false, 0.5)),
            focused: Some("t2".into()),
            ..Default::default()
        },
    );
    super::edit::hide(&mut document, "t2", 2);
    assert_eq!(document.focus.as_deref(), Some("t3"));
}

#[tokio::test]
async fn stale_split_placement_is_rejected_before_terminal_run() {
    let (mut store, mut core) = store();
    let mutation = edit(Change::Split {
        target: "t1".into(),
        terminal: String::new(),
        vertical: false,
        zoomed: false,
    });
    let key = mutation.tab.clone();
    let server = tokio::spawn(async move {
        hello(&mut core, &["doc.get", "doc.set", "terminal.run"]).await;
        let read = next(&mut core, "doc.get").await;
        core.reply(&read["id"],json!({"version":4,"value":{"schema":1,"closed":{"t1":1},"tabs":{key:{"root":{"type":"pane","terminal":"t2"}}}}})).await;
        assert!(core.try_read_frame().await.is_none());
    });
    store
        .submit(request(Action::Create(Box::new(Creation {
            params: Default::default(),
            edit: mutation,
            promotion: None,
            space_adoption: None,
            order: vec![],
        }))))
        .unwrap();
    let result = tokio::time::timeout(testcore::DEADLINE, store.results.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(result.is_err());
    server.await.unwrap();
}

#[test]
fn stale_inventory_preserves_a_replacement_already_in_the_stored_tree() {
    let mut mutation = edit(Change::Name("renamed".into()));
    mutation.incarnations.insert("t2".into(), 2);
    let root = Tree::split(Tree::pane("t1"), Tree::pane("t2"), true, 0.3);
    let mut document = Document::default();
    document.closed.insert("t2".into(), 2);
    document.tabs.insert(
        mutation.tab.clone(),
        TabLayout {
            root: Some(root.clone()),
            ..Default::default()
        },
    );
    mutation.apply(&mut document).unwrap();
    assert_eq!(document.tabs[&mutation.tab].root, Some(root));
}

#[test]
fn older_fallback_cannot_resurrect_a_newer_closed_incarnation() {
    let mut mutation = edit(Change::Focus("t2".into()));
    mutation.fallback = Tree::split(Tree::pane("t1"), Tree::pane("t2"), false, 0.5);
    mutation.incarnations.insert("t2".into(), 2);
    let mut document = Document::default();
    document.closed.insert("t2".into(), 3);
    assert!(mutation.apply(&mut document).is_err());
    mutation.change = Change::Split {
        target: "t2".into(),
        terminal: "t4".into(),
        vertical: false,
        zoomed: false,
    };
    assert!(mutation.apply(&mut document).is_err());
}
