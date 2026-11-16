use super::tests::*;
use super::*;
use crate::testcore;
use serde_json::json;

/// A JSON-RPC error reply for `request` (its id is reused).
async fn reply_error(core: &mut testcore::FakeCore, request: &serde_json::Value, kind: &str) {
    let error = json!({"jsonrpc":"2.0","id":request["id"],
        "error":{"code":-32000,"message":kind,"data":{"kind":kind}}});
    core.send(&hc_proto::frame::Frame::data(0, serde_json::to_vec(&error).unwrap()).encode())
        .await;
}

#[tokio::test]
async fn rename_space_writes_the_adoption_name_through_cas() {
    let (mut store, mut core) = store();
    let key = "0:/space:4:work";
    let server = tokio::spawn(async move {
        hello(&mut core, &["doc.get", "doc.set"]).await;
        let read = next(&mut core, "doc.get").await;
        core.reply(
            &read["id"],
            json!({"version":3,"value":{
                "schema":1,
                "future":{"preserve":true},
                "adoptions":{key:{
                    "space":"work","machine":null,
                    "tabs":{"0:/space:4:work/tab:3:tab":"tab"},
                    "name":"old","custom_label":false,"pinned":true
                }}
            }}),
        )
        .await;
        let write = next(&mut core, "doc.set").await;
        assert_eq!(write["params"]["version"], 3);
        let adoption = &write["params"]["value"]["adoptions"][key];
        assert_eq!(adoption["name"], "new name");
        assert_eq!(adoption["custom_label"], true);
        assert_eq!(adoption["space"], "work");
        assert_eq!(adoption["pinned"], true);
        assert_eq!(write["params"]["value"]["future"]["preserve"], true);
        core.reply(&write["id"], json!({"version":4})).await;
    });
    store
        .submit(request(Action::RenameSpace {
            key: key.into(),
            proposal: crate::structure::Adoption {
                space: "work".into(),
                name: "new name".into(),
                custom_label: true,
                ..Default::default()
            },
        }))
        .unwrap();
    let written = tokio::time::timeout(testcore::DEADLINE, store.results.recv())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .unwrap_written();
    assert_eq!(written.version, 4);
    server.await.unwrap();
}

#[tokio::test]
async fn rename_space_records_an_adoption_for_a_cwd_workspace() {
    let (mut store, mut core) = store();
    let key = "0:/cwd:10:/home/user";
    let server = tokio::spawn(async move {
        hello(&mut core, &["doc.get", "doc.set"]).await;
        let read = next(&mut core, "doc.get").await;
        core.reply(&read["id"], json!({"version":1,"value":null}))
            .await;
        let write = next(&mut core, "doc.set").await;
        let adoption = &write["params"]["value"]["adoptions"][key];
        assert_eq!(adoption["space"], "/home/user");
        assert_eq!(adoption["name"], "code");
        core.reply(&write["id"], json!({"version":2})).await;
    });
    store
        .submit(request(Action::RenameSpace {
            key: key.into(),
            proposal: crate::structure::Adoption {
                space: "/home/user".into(),
                name: "code".into(),
                custom_label: true,
                ..Default::default()
            },
        }))
        .unwrap();
    tokio::time::timeout(testcore::DEADLINE, store.results.recv())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .unwrap_written();
    server.await.unwrap();
}

#[tokio::test]
async fn workspace_creation_runs_a_kept_labelled_terminal_and_records_its_adoption() {
    let (mut store, mut core) = store();
    let space_key = "0:/space:6:fresh1";
    let tab_key = "0:/space:6:fresh1/tab:6:fresht";
    let server = tokio::spawn(async move {
        hello(&mut core, &["doc.get", "doc.set", "terminal.run"]).await;
        for _ in 0..2 {
            let read = next(&mut core, "doc.get").await;
            core.reply(&read["id"], json!({"value":null,"version":0}))
                .await;
        }
        let run = next(&mut core, "terminal.run").await;
        assert_eq!(run["params"]["keep"], true);
        assert_eq!(run["params"]["no_parent"], true);
        assert_eq!(run["params"]["no_notify"], true);
        assert_eq!(run["params"]["labels"]["herdr.space"], "fresh1");
        assert_eq!(run["params"]["labels"]["herdr.tab"], "fresht");
        assert_eq!(run["params"]["labels"]["herdr.seen"], "0");
        assert_eq!(run["params"]["cwd"], "/home/user/work");
        let mut terminal = testcore::minimal_terminal();
        terminal["address"] = json!("t9");
        core.reply(&run["id"], terminal).await;
        let read = next(&mut core, "doc.get").await;
        core.reply(&read["id"], json!({"value":null,"version":0}))
            .await;
        let write = next(&mut core, "doc.set").await;
        let document = Document::decode(write["params"]["value"].clone()).unwrap();
        let adoption = &document.adoptions[space_key];
        assert_eq!(adoption.space, "fresh1");
        assert_eq!(adoption.name, "work");
        assert!(!adoption.custom_label);
        assert_eq!(
            document.tabs[tab_key].root.as_ref().unwrap().terminals(),
            ["t9"]
        );
        assert_eq!(document.order[space_key], ["space:6:fresh1/tab:6:fresht"]);
        assert_eq!(document.focus.as_deref(), Some("t9"));
        core.reply(&write["id"], json!({"version":1})).await;
    });
    store
        .submit(request(Action::Create(Box::new(Creation {
            params: RunParams {
                cwd: Some("/home/user/work".into()),
                no_parent: true,
                no_notify: true,
                keep: true,
                labels: BTreeMap::from([
                    ("herdr.space".into(), "fresh1".into()),
                    ("herdr.tab".into(), "fresht".into()),
                    ("herdr.seen".into(), "0".into()),
                ]),
                ..Default::default()
            },
            edit: Edit {
                tab: tab_key.into(),
                space: space_key.into(),
                fallback: crate::structure::Tree::pane("pending-created-terminal"),
                incarnations: BTreeMap::new(),
                change: Change::NewTab {
                    terminal: String::new(),
                    name: String::new(),
                },
            },
            promotion: None,
            space_adoption: Some(crate::structure::Adoption {
                space: "fresh1".into(),
                tabs: BTreeMap::from([(tab_key.into(), "fresht".into())]),
                name: "work".into(),
                ..Default::default()
            }),
            report_created: false,
            order: vec![],
        }))))
        .unwrap();
    let written = tokio::time::timeout(testcore::DEADLINE, store.results.recv())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .unwrap_written();
    assert_eq!(written.version, 1);
    server.await.unwrap();
}

#[tokio::test]
async fn workspace_close_kills_each_member_and_persists_after_every_kill() {
    let (mut store, mut core) = store();
    let membership = || ("space:4:work".into(), "space:4:work/tab:3:tab".into());
    let server = tokio::spawn(async move {
        hello(
            &mut core,
            &["doc.get", "doc.set", "terminal.get", "terminal.kill"],
        )
        .await;
        let read = next(&mut core, "doc.get").await;
        core.reply(&read["id"], json!({"value":null,"version":0}))
            .await;
        let mut written = serde_json::Value::Null;
        let mut version = 0u64;
        for (terminal, created_ms) in [("t1", 1u64), ("t2", 2u64)] {
            let get = next(&mut core, "terminal.get").await;
            assert_eq!(get["params"]["terminal"], terminal);
            let mut record = testcore::minimal_terminal();
            record["address"] = json!(terminal);
            record["created_ms"] = json!(created_ms);
            record["labels"] = json!({"herdr.space":"work","herdr.tab":"tab"});
            core.reply(&get["id"], record.clone()).await;
            let foreign = core.read_request().await;
            assert_eq!(foreign["method"], "doc.get");
            assert_eq!(foreign["params"]["name"], "herdr-app");
            core.reply(&foreign["id"], json!({"value":null,"version":0}))
                .await;
            let kill = next(&mut core, "terminal.kill").await;
            assert_eq!(kill["params"]["terminal"], terminal);
            core.reply(&kill["id"], record).await;
            let read = next(&mut core, "doc.get").await;
            core.reply(&read["id"], json!({"value":written,"version":version}))
                .await;
            let write = next(&mut core, "doc.set").await;
            let document = Document::decode(write["params"]["value"].clone()).unwrap();
            assert_eq!(document.closed[terminal], created_ms);
            written = write["params"]["value"].clone();
            version += 1;
            core.reply(&write["id"], json!({"version":version})).await;
        }
    });
    store
        .submit(request(Action::Close {
            terminals: vec![("t1".into(), 1), ("t2".into(), 2)],
            memberships: BTreeMap::from([("t1".into(), membership()), ("t2".into(), membership())]),
            fallback: BTreeMap::new(),
            next: Some("t3".into()),
        }))
        .unwrap();
    let written = tokio::time::timeout(testcore::DEADLINE, store.results.recv())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .unwrap_written();
    assert_eq!(written.version, 2);
    assert_eq!(written.document.closed.len(), 2);
    assert_eq!(written.document.focus.as_deref(), Some("t3"));
    server.await.unwrap();
}

#[tokio::test]
async fn close_removes_the_record_after_kill_when_rm_is_offered() {
    let (mut store, mut core) = store();
    let membership = || ("space:4:work".into(), "space:4:work/tab:3:tab".into());
    let server = tokio::spawn(async move {
        hello(
            &mut core,
            &[
                "doc.get",
                "doc.set",
                "terminal.get",
                "terminal.kill",
                "terminal.rm",
            ],
        )
        .await;
        let read = next(&mut core, "doc.get").await;
        core.reply(&read["id"], json!({"value":null,"version":0}))
            .await;
        let get = next(&mut core, "terminal.get").await;
        let mut record = testcore::minimal_terminal();
        record["address"] = json!("t1");
        record["created_ms"] = json!(1);
        record["labels"] = json!({"herdr.space":"work","herdr.tab":"tab"});
        core.reply(&get["id"], record.clone()).await;
        let foreign = core.read_request().await;
        assert_eq!(foreign["params"]["name"], "herdr-app");
        core.reply(&foreign["id"], json!({"value":null,"version":0}))
            .await;
        let kill = next(&mut core, "terminal.kill").await;
        core.reply(&kill["id"], record).await;
        let rm = next(&mut core, "terminal.rm").await;
        assert_eq!(rm["params"], json!({"terminal": "t1"}));
        core.reply(&rm["id"], json!({})).await;
        let read = next(&mut core, "doc.get").await;
        core.reply(&read["id"], json!({"value":null,"version":0}))
            .await;
        let write = next(&mut core, "doc.set").await;
        core.reply(&write["id"], json!({"version":1})).await;
    });
    store
        .submit(request(Action::Close {
            terminals: vec![("t1".into(), 1)],
            memberships: BTreeMap::from([("t1".into(), membership())]),
            fallback: BTreeMap::new(),
            next: None,
        }))
        .unwrap();
    tokio::time::timeout(testcore::DEADLINE, store.results.recv())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .unwrap_written();
    server.await.unwrap();
}

#[tokio::test]
async fn kill_removes_the_record_and_tolerates_rm_not_found() {
    let (mut store, mut core) = store();
    let server = tokio::spawn(async move {
        hello(&mut core, &["terminal.get", "terminal.kill", "terminal.rm"]).await;
        let get = next(&mut core, "terminal.get").await;
        let mut record = testcore::minimal_terminal();
        record["address"] = json!("t1");
        record["created_ms"] = json!(1);
        core.reply(&get["id"], record.clone()).await;
        let kill = next(&mut core, "terminal.kill").await;
        core.reply(&kill["id"], record).await;
        let rm = next(&mut core, "terminal.rm").await;
        assert_eq!(rm["params"]["terminal"], "t1");
        reply_error(&mut core, &rm, "not_found").await;
    });
    store
        .submit(request(Action::Kill {
            terminal: "t1".into(),
            created_ms: 1,
            allow_gone: false,
        }))
        .unwrap();
    let out = tokio::time::timeout(testcore::DEADLINE, store.results.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(out.unwrap(), Outcome::Done));
    server.await.unwrap();
}

#[tokio::test]
async fn kill_allow_gone_accepts_an_already_removed_terminal() {
    let (mut store, mut core) = store();
    let server = tokio::spawn(async move {
        hello(&mut core, &["terminal.get", "terminal.kill"]).await;
        let get = next(&mut core, "terminal.get").await;
        let mut record = testcore::minimal_terminal();
        record["address"] = json!("t1");
        record["created_ms"] = json!(1);
        core.reply(&get["id"], record).await;
        // The kill raced the core's own cleanup — `allow_gone` is Done.
        let kill = next(&mut core, "terminal.kill").await;
        reply_error(&mut core, &kill, "not_found").await;
    });
    store
        .submit(request(Action::Kill {
            terminal: "t1".into(),
            created_ms: 1,
            allow_gone: true,
        }))
        .unwrap();
    let out = tokio::time::timeout(testcore::DEADLINE, store.results.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(out.unwrap(), Outcome::Done));
    server.await.unwrap();
}
