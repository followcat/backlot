use super::rpc_actions::PUT_CHUNK_BYTES;
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
    hello_with(core, methods, &[]).await;
}

pub(super) async fn hello_with(core: &mut FakeCore, methods: &[&str], features: &[&str]) {
    core.read_hello().await;
    core.answer_hello(true, methods, features).await;
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
        .unwrap_written();
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

#[tokio::test]
async fn a_cancelled_send_is_a_refusal_never_a_success() {
    let (mut store, mut core) = store();
    let server = tokio::spawn(async move {
        hello(&mut core, &["terminal.get", "terminal.send"]).await;
        let get = next(&mut core, "terminal.get").await;
        core.reply(&get["id"], testcore::minimal_terminal()).await;
        let send = next(&mut core, "terminal.send").await;
        assert_eq!(
            send["params"],
            json!({"terminal": "t7", "text": "fix the tests"})
        );
        // hc 0.1.21: the caller went away or the core stopped, and
        // nothing was typed.
        let error = json!({"jsonrpc":"2.0","id":send["id"],"error":{"code":-32000,
            "message":"cancelled","data":{"kind":"cancelled","hint":""}}});
        core.send(&Frame::data(0, serde_json::to_vec(&error).unwrap()).encode())
            .await;
    });
    store
        .submit(request(Action::SendLine {
            terminal: "t7".into(),
            created_ms: 1_700_000_000_000,
            text: "fix the tests".into(),
        }))
        .unwrap();
    let error = tokio::time::timeout(testcore::DEADLINE, store.results.recv())
        .await
        .unwrap()
        .unwrap()
        .err()
        .expect("a cancelled send is never a success");
    assert_eq!(error, "Send cancelled; nothing was typed. Send it again.");
    server.await.unwrap();
}

#[tokio::test]
async fn an_other_send_failure_keeps_the_core_word() {
    let (mut store, mut core) = store();
    let server = tokio::spawn(async move {
        hello(&mut core, &["terminal.get", "terminal.send"]).await;
        let get = next(&mut core, "terminal.get").await;
        core.reply(&get["id"], testcore::minimal_terminal()).await;
        let send = next(&mut core, "terminal.send").await;
        let error = json!({"jsonrpc":"2.0","id":send["id"],"error":{"code":-32000,
            "message":"blocked","data":{"kind":"blocked","hint":""}}});
        core.send(&Frame::data(0, serde_json::to_vec(&error).unwrap()).encode())
            .await;
    });
    store
        .submit(request(Action::SendLine {
            terminal: "t7".into(),
            created_ms: 1_700_000_000_000,
            text: "hi".into(),
        }))
        .unwrap();
    let error = tokio::time::timeout(testcore::DEADLINE, store.results.recv())
        .await
        .unwrap()
        .unwrap()
        .err()
        .unwrap();
    assert!(error.contains("blocked"), "{error}");
    assert!(!error.contains("cancelled"), "{error}");
    server.await.unwrap();
}

/// `put_chunks`: the image crosses the link in acknowledged pieces and
/// the finish's path is what the pane pastes.
#[tokio::test]
async fn an_image_upload_uses_acknowledged_pieces_when_offered() {
    let (mut store, mut core) = store();
    let image = vec![0x41u8; PUT_CHUNK_BYTES + 1024];
    let expected = image.clone();
    let server = tokio::spawn(async move {
        hello_with(
            &mut core,
            &[
                "terminal.get",
                "terminal.put_begin",
                "terminal.put_chunk",
                "terminal.put_finish",
                "terminal.put_abort",
            ],
            &[hc_proto::hello::feature::PUT_CHUNKS],
        )
        .await;
        let get = next(&mut core, "terminal.get").await;
        core.reply(&get["id"], testcore::minimal_terminal()).await;
        let begin = next(&mut core, "terminal.put_begin").await;
        assert_eq!(begin["params"]["terminal"], "t7");
        assert_eq!(begin["params"]["bytes"], expected.len());
        let transfer = begin["params"]["transfer"].as_str().unwrap().to_owned();
        assert!(
            (16..=64).contains(&transfer.len()) && transfer.bytes().all(|b| b.is_ascii_hexdigit()),
            "a handle the core accepts: {transfer}"
        );
        core.reply(&begin["id"], json!({"transfer": transfer}))
            .await;
        let first = next(&mut core, "terminal.put_chunk").await;
        assert_eq!(first["params"]["transfer"], transfer);
        assert_eq!(first["params"]["offset"], 0);
        assert_eq!(
            first["params"]["data"],
            crate::selection::base64_encode(&expected[..PUT_CHUNK_BYTES])
        );
        core.reply(&first["id"], json!({"bytes": PUT_CHUNK_BYTES}))
            .await;
        let second = next(&mut core, "terminal.put_chunk").await;
        assert_eq!(second["params"]["offset"], PUT_CHUNK_BYTES);
        assert_eq!(
            second["params"]["data"],
            crate::selection::base64_encode(&expected[PUT_CHUNK_BYTES..])
        );
        core.reply(&second["id"], json!({"bytes": expected.len()}))
            .await;
        let finish = next(&mut core, "terminal.put_finish").await;
        assert_eq!(finish["params"]["transfer"], transfer);
        core.reply(
            &finish["id"],
            json!({"path": "/home/user/i.png", "bytes": expected.len()}),
        )
        .await;
    });
    store
        .submit(request(Action::PutImage {
            terminal: "t7".into(),
            created_ms: 1_700_000_000_000,
            name: "clipboard-1.png".into(),
            bytes: image,
            pieces: true,
        }))
        .unwrap();
    let outcome = tokio::time::timeout(testcore::DEADLINE, store.results.recv())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    match outcome {
        Outcome::Put { terminal, path } => {
            assert_eq!(terminal, "t7");
            assert_eq!(path, "/home/user/i.png");
        }
        _ => panic!("expected a Put outcome"),
    }
    server.await.unwrap();
}

/// A refused piece ends the transfer: `put_abort` takes the handle and
/// the failure surfaces instead of a half-written file.
#[tokio::test]
async fn a_refused_piece_aborts_the_transfer() {
    let (mut store, mut core) = store();
    let image = vec![0x43u8; 2048];
    let server = tokio::spawn(async move {
        hello_with(
            &mut core,
            &[
                "terminal.get",
                "terminal.put_begin",
                "terminal.put_chunk",
                "terminal.put_finish",
                "terminal.put_abort",
            ],
            &[hc_proto::hello::feature::PUT_CHUNKS],
        )
        .await;
        let get = next(&mut core, "terminal.get").await;
        core.reply(&get["id"], testcore::minimal_terminal()).await;
        let begin = next(&mut core, "terminal.put_begin").await;
        let transfer = begin["params"]["transfer"].as_str().unwrap().to_owned();
        core.reply(&begin["id"], json!({"transfer": transfer}))
            .await;
        let piece = next(&mut core, "terminal.put_chunk").await;
        let error = json!({"jsonrpc":"2.0","id":piece["id"],"error":{"code":-32000,
            "message":"unavailable","data":{"kind":"unavailable","hint":""}}});
        core.send(&Frame::data(0, serde_json::to_vec(&error).unwrap()).encode())
            .await;
        let abort = next(&mut core, "terminal.put_abort").await;
        assert_eq!(abort["params"]["transfer"], transfer);
        core.reply(&abort["id"], json!({})).await;
        assert!(
            core.try_read_frame().await.is_none(),
            "nothing follows the abort"
        );
    });
    store
        .submit(request(Action::PutImage {
            terminal: "t7".into(),
            created_ms: 1_700_000_000_000,
            name: "clipboard-1.png".into(),
            bytes: image,
            pieces: true,
        }))
        .unwrap();
    let error = tokio::time::timeout(testcore::DEADLINE, store.results.recv())
        .await
        .unwrap()
        .unwrap()
        .err()
        .expect("a refused piece is a failure");
    assert!(error.contains("unavailable"), "{error}");
    server.await.unwrap();
}

#[tokio::test]
async fn an_image_upload_falls_back_to_one_put_without_the_capability() {
    let (mut store, mut core) = store();
    let image = vec![0x42u8; 4096];
    let expected = image.clone();
    let server = tokio::spawn(async move {
        hello(&mut core, &["terminal.get", "terminal.put"]).await;
        let get = next(&mut core, "terminal.get").await;
        core.reply(&get["id"], testcore::minimal_terminal()).await;
        let put = next(&mut core, "terminal.put").await;
        assert_eq!(put["params"]["terminal"], "t7");
        assert_eq!(
            put["params"]["data"],
            crate::selection::base64_encode(&expected)
        );
        core.reply(&put["id"], json!({"path": "/home/user/i.png"}))
            .await;
    });
    store
        .submit(request(Action::PutImage {
            terminal: "t7".into(),
            created_ms: 1_700_000_000_000,
            name: "clipboard-1.png".into(),
            bytes: image,
            pieces: true,
        }))
        .unwrap();
    let outcome = tokio::time::timeout(testcore::DEADLINE, store.results.recv())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    match outcome {
        Outcome::Put { terminal, path } => {
            assert_eq!(terminal, "t7");
            assert_eq!(path, "/home/user/i.png");
        }
        _ => panic!("expected a Put outcome"),
    }
    server.await.unwrap();
}

#[test]
fn send_failures_are_refusals_never_successes() {
    use hc_proto::client::CallError;
    use hc_proto::jsonrpc::{RpcError, error_kind};
    let cancelled = CallError::Rpc(RpcError::new(
        -32000,
        error_kind::CANCELLED,
        "cancelled",
        "",
    ));
    assert_eq!(
        super::rpc_actions::send_failure(cancelled),
        "Send cancelled; nothing was typed. Send it again."
    );
    assert_eq!(
        super::rpc_actions::send_failure(CallError::Timeout),
        "Send timed out; the core may still type it."
    );
    let blocked = CallError::Rpc(RpcError::new(-32000, "blocked", "blocked", ""));
    assert_eq!(
        super::rpc_actions::send_failure(blocked),
        "blocked: blocked"
    );
}

#[test]
fn put_image_gets_the_call_timeout_everything_else_the_interactive_one() {
    let put = Action::PutImage {
        terminal: "t1".into(),
        created_ms: 1,
        name: "clipboard-1.png".into(),
        bytes: Vec::new(),
        pieces: false,
    };
    assert_eq!(action_deadline(&put), std::time::Duration::from_secs(120));
    for action in [
        Action::Edit(edit(Change::Focus("t1".into()))),
        Action::Kill {
            terminal: "t1".into(),
            created_ms: 1,
            allow_gone: false,
        },
        Action::Exec {
            line: "ls".into(),
            machine: None,
        },
    ] {
        assert_eq!(action_deadline(&action), std::time::Duration::from_secs(15));
    }
}

#[test]
fn primitive_edits_preserve_concurrent_splits_and_opaque_metadata() {
    let mut document = Document::default();
    document.extra.insert("future".into(), json!([1, 2, 3]));
    edit(Change::Split {
        target: "t1".into(),
        terminal: "t2".into(),
        vertical: false,
        zoomed: false,
    })
    .apply(&mut document)
    .unwrap();
    edit(Change::Split {
        target: "t1".into(),
        terminal: "t3".into(),
        vertical: true,
        zoomed: false,
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
