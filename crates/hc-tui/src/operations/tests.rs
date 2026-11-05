use super::*;
use crate::{
    structure::Tree,
    testcore::{self, FakeCore},
};
use hc_proto::{client::Client, frame::Frame};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

pub(super) fn store() -> (Store, FakeCore) {
    let (core, read, write) = testcore::pair(65536);
    let wire = Arc::new(Mutex::new(Some((read, write))));
    let open: OpenRpc = Arc::new(move || {
        let (read, write) = wire
            .lock()
            .unwrap()
            .take()
            .expect("mutations are not automatically replayed on a new connection");
        Box::pin(Client::open(read, write, None))
    });
    (Store::new(open), core)
}

pub(super) async fn hello(core: &mut FakeCore, methods: &[&str]) {
    core.read_hello().await;
    core.answer_hello(true, methods, &[]).await;
}

pub(super) fn edit(change: Change) -> Edit {
    Edit {
        tab: "0:/space:4:work/tab:3:tab".into(),
        space: "0:/space:4:work".into(),
        fallback: Tree::pane("t1"),
        incarnations: BTreeMap::new(),
        change,
    }
}

pub(super) fn request(action: Action) -> Request {
    Request {
        core: "core-fake-1".into(),
        action,
    }
}

pub(super) async fn next(core: &mut FakeCore, method: &str) -> Value {
    let call = core.read_request().await;
    assert_eq!(call["method"], method);
    if method.starts_with("doc.") {
        assert_eq!(call["params"]["name"], "hc-tui");
        assert!(call["params"]["on"].is_null());
    }
    call
}

#[tokio::test]
async fn cas_conflict_rereads_and_rebases_without_losing_another_clients_split() {
    let (mut store, mut core) = store();
    let mutation = edit(Change::Focus("t1".into()));
    let key = mutation.tab.clone();
    let server = tokio::spawn(async move {
        hello(&mut core, &["doc.get", "doc.set"]).await;
        let read = next(&mut core, "doc.get").await;
        core.reply(&read["id"], json!({"version":5,"value":null}))
            .await;
        let write = next(&mut core, "doc.set").await;
        assert_eq!(write["params"]["version"], 5);
        let conflict = json!({"jsonrpc":"2.0","id":write["id"],"error":{"code":-32000,"message":"conflict","data":{"kind":"conflict"}}});
        core.send(&Frame::data(0, serde_json::to_vec(&conflict).unwrap()).encode())
            .await;
        let read = next(&mut core, "doc.get").await;
        let root = Tree::split(Tree::pane("t1"), Tree::pane("t2"), true, 0.25);
        core.reply(&read["id"],json!({"version":6,"value":{"schema":1,"future":{"preserve":true},"tabs":{key.clone():{"root":root}}}})).await;
        let write = next(&mut core, "doc.set").await;
        assert_eq!(write["params"]["version"], 6);
        assert_eq!(write["params"]["value"]["future"]["preserve"], true);
        let document = Document::decode(write["params"]["value"].clone()).unwrap();
        assert_eq!(
            document.tabs[&key].root.as_ref().unwrap().terminals(),
            ["t1", "t2"]
        );
        core.reply(&write["id"], json!({"version":7})).await;
    });
    store.submit(request(Action::Edit(mutation))).unwrap();
    let written = tokio::time::timeout(testcore::DEADLINE, store.results.recv())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(written.version, 7);
    assert_eq!(written.core, "core-fake-1");
    server.await.unwrap();
}

#[tokio::test]
async fn ambiguous_write_dies_without_reconnect_or_replay() {
    let (mut store, mut core) = store();
    let server = tokio::spawn(async move {
        hello(&mut core, &["doc.get", "doc.set"]).await;
        let read = next(&mut core, "doc.get").await;
        core.reply(&read["id"], json!({"value":null,"version":0}))
            .await;
        next(&mut core, "doc.set").await;
        // Simulate a committed write whose reply was lost. End this wire.
    });
    store
        .submit(request(Action::Edit(edit(Change::Focus("t1".into())))))
        .unwrap();
    let error = tokio::time::timeout(testcore::DEADLINE, store.results.recv())
        .await
        .unwrap()
        .unwrap()
        .err()
        .unwrap();
    assert!(error.contains("not replayed"));
    server.await.unwrap();
}

#[tokio::test]
async fn unknown_layout_schema_blocks_close_before_terminal_kill() {
    let (mut store, mut core) = store();
    let server = tokio::spawn(async move {
        hello(&mut core, &["doc.get", "doc.set", "terminal.kill"]).await;
        let read = next(&mut core, "doc.get").await;
        core.reply(&read["id"], json!({"value":{"schema":2},"version":2}))
            .await;
        assert!(
            core.try_read_frame().await.is_none(),
            "no destructive call may follow preflight failure"
        );
    });
    store
        .submit(request(Action::Close {
            terminals: vec![("t1".into(), 1)],
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
    assert!(error.contains("Unsupported"));
    server.await.unwrap();
}

#[tokio::test]
async fn a_changed_core_cannot_receive_a_stale_mutation() {
    let (mut store, mut core) = store();
    let server = tokio::spawn(async move {
        hello(&mut core, &["doc.get", "doc.set"]).await;
        assert!(core.try_read_frame().await.is_none());
    });
    store
        .submit(Request {
            core: "older-core".into(),
            action: Action::Edit(edit(Change::Focus("t1".into()))),
        })
        .unwrap();
    let error = tokio::time::timeout(testcore::DEADLINE, store.results.recv())
        .await
        .unwrap()
        .unwrap()
        .err()
        .unwrap();
    assert!(error.contains("identity changed"));
    server.await.unwrap();
}

#[test]
fn primitive_edits_preserve_concurrent_splits_and_opaque_metadata() {
    let mut document = Document::default();
    document.extra.insert("future".into(), json!([1, 2, 3]));
    edit(Change::Split {
        target: "t1".into(),
        terminal: "t2".into(),
        vertical: false,
    })
    .apply(&mut document)
    .unwrap();
    edit(Change::Split {
        target: "t1".into(),
        terminal: "t3".into(),
        vertical: true,
    })
    .apply(&mut document)
    .unwrap();
    let key = edit(Change::Name(String::new())).tab;
    assert_eq!(
        document.tabs[&key].root.as_ref().unwrap().terminals(),
        ["t1", "t3", "t2"]
    );
    edit(Change::Zoom("t2".into()))
        .apply(&mut document)
        .unwrap();
    edit(Change::Focus("t1".into()))
        .apply(&mut document)
        .unwrap();
    assert_eq!(document.tabs[&key].zoomed.as_deref(), Some("t1"));
    edit::hide(&mut document, "t1", 1);
    assert!(
        !document.tabs[&key]
            .root
            .as_ref()
            .unwrap()
            .terminals()
            .contains(&"t1".into())
    );
    assert_eq!(document.extra["future"], json!([1, 2, 3]));
}
