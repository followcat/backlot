use super::tests::*;
use super::*;
use crate::{structure::Tree, testcore};
use serde_json::json;

#[tokio::test]
async fn creation_uses_cas_agreed_adoption_and_preserves_foreign_labels() {
    let (mut store, mut core) = store();
    let old_tab = edit(Change::Name(String::new())).tab;
    let old_space = edit(Change::Name(String::new())).space;
    let chosen_space = "space:6:agreed";
    let chosen_tab = format!("{chosen_space}/tab:6:tab-id");
    let expected_key = tab_key(None, &chosen_tab);
    let chosen = crate::structure::Adoption {
        space: "agreed".into(),
        tabs: BTreeMap::from([(old_tab.clone(), "tab-id".into())]),
        name: "work".into(),
        ..Default::default()
    };
    let initial = json!({"schema":1,"adoptions":{old_space.clone():chosen}});
    let server = tokio::spawn(async move {
        hello(
            &mut core,
            &[
                "doc.get",
                "doc.set",
                "terminal.run",
                "terminal.get",
                "terminal.label",
                "terminal.keep",
            ],
        )
        .await;
        let read = next(&mut core, "doc.get").await;
        core.reply(&read["id"], json!({"value":initial,"version":1}))
            .await;
        let read = next(&mut core, "doc.get").await;
        core.reply(&read["id"], json!({"value":initial,"version":1}))
            .await;
        let write = next(&mut core, "doc.set").await;
        let adopted = write["params"]["value"].clone();
        core.reply(&write["id"], json!({"version":2})).await;
        let get = next(&mut core, "terminal.get").await;
        let mut term = testcore::minimal_terminal();
        term["labels"] = json!({"keep":"value"});
        core.reply(&get["id"], term).await;
        let labels = next(&mut core, "terminal.label").await;
        assert_eq!(
            labels["params"]["labels"],
            json!({"herdr.space":"agreed","herdr.tab":"tab-id"})
        );
        core.reply(&labels["id"], json!({})).await;
        let keep = next(&mut core, "terminal.keep").await;
        assert_eq!(keep["params"]["terminal"], "t1");
        assert_eq!(keep["params"]["keep"], true);
        core.reply(&keep["id"], json!({})).await;
        let read = next(&mut core, "doc.get").await;
        core.reply(&read["id"], json!({"value":adopted,"version":2}))
            .await;
        let run = next(&mut core, "terminal.run").await;
        assert_eq!(run["params"]["labels"]["herdr.space"], "agreed");
        assert_eq!(run["params"]["labels"]["herdr.tab"], "tab-id");
        assert_eq!(run["params"]["labels"]["herdr.seen"], "0");
        let mut terminal = testcore::minimal_terminal();
        terminal["address"] = json!("t2");
        core.reply(&run["id"], terminal).await;
        let read = next(&mut core, "doc.get").await;
        core.reply(&read["id"], json!({"value":adopted,"version":2}))
            .await;
        let write = next(&mut core, "doc.set").await;
        let document = Document::decode(write["params"]["value"].clone()).unwrap();
        assert_eq!(
            document.tabs[&expected_key]
                .root
                .as_ref()
                .unwrap()
                .terminals(),
            ["t1", "t2"]
        );
        core.reply(&write["id"], json!({"version":3})).await;
    });
    let creation = Creation {
        params: RunParams {
            labels: BTreeMap::from([("herdr.seen".into(), "0".into())]),
            ..Default::default()
        },
        edit: edit(Change::Split {
            target: "t1".into(),
            terminal: String::new(),
            vertical: false,
            zoomed: false,
        }),
        space_adoption: None,
        promotion: Some(Promotion {
            key: old_space,
            proposal: crate::structure::Adoption {
                space: "different".into(),
                tabs: BTreeMap::from([(old_tab.clone(), "other-tab".into())]),
                ..Default::default()
            },
            members: vec![Member {
                terminal: "t1".into(),
                tab: old_tab,
                labels: BTreeMap::new(),
            }],
        }),
        order: vec![],
    };
    store
        .submit(request(Action::Create(Box::new(creation))))
        .unwrap();
    let written = tokio::time::timeout(testcore::DEADLINE, store.results.recv())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .unwrap_written();
    assert_eq!(written.version, 3);
    server.await.unwrap();
}

#[test]
fn tab_movement_wraps_and_removes_closed_ids_but_preserves_new_concurrent_tabs() {
    let mut document = Document::default();
    document.order.insert(
        "0:/work".into(),
        vec!["a".into(), "closed".into(), "b".into(), "c".into()],
    );
    document.tabs.insert(
        "0:/new".into(),
        crate::structure::TabLayout {
            root: Some(Tree::pane("t9")),
            ..Default::default()
        },
    );
    document
        .order
        .get_mut("0:/work")
        .unwrap()
        .push("new".into());
    Edit {
        space: "0:/work".into(),
        tab: "0:/a".into(),
        fallback: Tree::pane("t1"),
        incarnations: BTreeMap::new(),
        change: Change::MoveTab {
            tab_id: "a".into(),
            direction: -1,
            fallback: vec!["a".into(), "b".into(), "c".into()],
        },
    }
    .apply(&mut document)
    .unwrap();
    assert_eq!(document.order["0:/work"], ["b", "c", "new", "a"]);
}

#[tokio::test]
async fn replaced_close_target_is_checked_before_kill() {
    let (mut store, mut core) = store();
    let server = tokio::spawn(async move {
        hello(
            &mut core,
            &["doc.get", "doc.set", "terminal.get", "terminal.kill"],
        )
        .await;
        let read = next(&mut core, "doc.get").await;
        core.reply(&read["id"], json!({"value":null,"version":0}))
            .await;
        let get = next(&mut core, "terminal.get").await;
        let mut term = testcore::minimal_terminal();
        term["created_ms"] = json!(9);
        core.reply(&get["id"], term).await;
        assert!(core.try_read_frame().await.is_none());
    });
    store
        .submit(request(Action::Close {
            terminals: vec![("t1".into(), 1)],
            memberships: BTreeMap::new(),
            fallback: BTreeMap::new(),
            next: None,
        }))
        .unwrap();
    let error = tokio::time::timeout(testcore::DEADLINE, store.results.recv())
        .await
        .unwrap()
        .unwrap()
        .err()
        .unwrap();
    assert!(error.contains("replaced"));
    server.await.unwrap();
}

#[tokio::test]
async fn close_rechecks_foreign_membership_before_terminal_kill() {
    let (mut store, mut core) = store();
    let server = tokio::spawn(async move {
        hello(
            &mut core,
            &["doc.get", "doc.set", "terminal.get", "terminal.kill"],
        )
        .await;
        let read = next(&mut core, "doc.get").await;
        core.reply(&read["id"], json!({"value":null,"version":0}))
            .await;
        let get = next(&mut core, "terminal.get").await;
        let mut terminal = testcore::minimal_terminal();
        terminal["address"] = json!("t1");
        terminal["created_ms"] = json!(1);
        terminal["labels"] = json!({"herdr.space":"other","herdr.tab":"tab"});
        core.reply(&get["id"], terminal).await;
        let foreign = core.read_request().await;
        assert_eq!(foreign["method"], "doc.get");
        assert_eq!(foreign["params"]["name"], "herdr-app");
        core.reply(&foreign["id"], json!({"value":null,"version":0}))
            .await;
        assert!(core.try_read_frame().await.is_none());
    });
    store
        .submit(request(Action::Close {
            terminals: vec![("t1".into(), 1)],
            memberships: BTreeMap::from([(
                "t1".into(),
                ("space:4:work".into(), "space:4:work/tab:3:tab".into()),
            )]),
            fallback: BTreeMap::new(),
            next: None,
        }))
        .unwrap();
    let error = tokio::time::timeout(testcore::DEADLINE, store.results.recv())
        .await
        .unwrap()
        .unwrap()
        .err()
        .unwrap();
    assert!(error.contains("moved"));
    server.await.unwrap();
}
