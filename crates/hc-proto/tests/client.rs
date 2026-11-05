//! Client negotiation and typed-call tests over scripted fake cores on
//! in-memory wires. No real core, no real endpoints. Every wait runs
//! under a deadline.

mod common;

use common::{discovery_settled, fakecore};
use hc_proto::client::{CallError, Client, Error};
use hc_proto::doc::{DocGetParams, DocSetParams};
use hc_proto::jsonrpc::error_kind;
use hc_proto::terminal::{ListParams, ReadParams, TerminalParams};
use serde_json::json;
use std::time::Duration;

const DEADLINE: Duration = Duration::from_secs(15);

/// Runs a future to completion inside [`DEADLINE`]; panics rather than hangs.
async fn timeout<F: std::future::Future>(f: F) -> F::Output {
    tokio::time::timeout(DEADLINE, f)
        .await
        .expect("deadline exceeded")
}

async fn client_open(
    read: hc_proto::transport::ReadHalf,
    write: hc_proto::transport::WriteHalf,
) -> Client {
    timeout(Client::open(read, write, None)).await.unwrap()
}

#[tokio::test]
async fn handshake_negotiates_and_typed_calls_decode() {
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(fakecore::ALL_METHODS).await;
        // rpc.discover and terminal.list can arrive in either order.
        for _ in 0..2 {
            let req = core.read_request().await;
            match req["method"].as_str().unwrap() {
                "rpc.discover" => {
                    core.reply(&req["id"], fakecore::discover_doc(fakecore::ALL_METHODS))
                        .await
                }
                "terminal.list" => {
                    core.reply(
                        &req["id"],
                        json!({"terminals": [common::minimal_terminal()], "hidden_finished": 0}),
                    )
                    .await
                }
                other => panic!("unexpected method {other}"),
            }
        }
        core
    });
    let client = client_open(read, write).await;

    assert_eq!(client.core().name, "fake-core");
    assert_eq!(client.hello().version, "fake-core 0.0");
    discovery_settled(&client).await;
    assert!(client.discovery().is_some(), "discover answered");
    assert!(client.discovery_note().is_none());

    let list = timeout(client.terminal_list(ListParams::default()))
        .await
        .unwrap();
    assert_eq!(list.terminals.len(), 1);
    assert_eq!(list.terminals[0].address, "t7");
    timeout(server).await.unwrap();
}

#[tokio::test]
async fn hello_reply_and_first_frame_in_one_write() {
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.read_hello().await;
        // Reply line AND a channel-0 notification frame in a single write:
        // the client's read-ahead buffer must hand the frame to its pump.
        let mut bytes = serde_json::to_vec(&json!({
            "jsonrpc": "2.0", "id": 0,
            "result": fakecore::hello_result(&["machine.list"]),
        }))
        .unwrap();
        bytes.push(b'\n');
        bytes.extend(
            hc_proto::frame::Frame::data(
                0,
                serde_json::to_vec(&json!({
                    "jsonrpc": "2.0", "method": "heartbeat", "params": {"seq": 1}
                }))
                .unwrap(),
            )
            .encode(),
        );
        core.send(&bytes).await;
        let req = core.read_request().await;
        core.reply(&req["id"], json!({"machines": []})).await;
        core
    });
    let client = client_open(read, write).await;
    let list = timeout(client.machine_list()).await.unwrap();
    assert!(list.machines.is_empty());
    timeout(server).await.unwrap();
}

#[tokio::test]
async fn generation_mismatch_is_rejected() {
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    tokio::spawn(async move {
        core.read_hello().await;
        let mut result = fakecore::hello_result(fakecore::ALL_METHODS);
        result["generation"] = json!(2);
        core.answer_hello(result).await;
    });
    let err = timeout(Client::open(read, write, None)).await.unwrap_err();
    assert!(
        err.to_string().contains("generation"),
        "mismatch explained: {err}"
    );
    assert!(matches!(err, Error::Generation(_)));
}

#[tokio::test]
async fn framed_refusal_is_rejected() {
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    tokio::spawn(async move {
        core.read_hello().await;
        let mut result = fakecore::hello_result(fakecore::ALL_METHODS);
        result["framed"] = json!(false);
        core.answer_hello(result).await;
    });
    let err = timeout(Client::open(read, write, None)).await.unwrap_err();
    assert!(matches!(err, Error::FramedRefused));
}

#[tokio::test]
async fn hello_error_kind_is_preserved() {
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    tokio::spawn(async move {
        core.read_hello().await;
        core.answer_hello_error(error_kind::UNSUPPORTED_GENERATION, "speak generation 1")
            .await;
    });
    let err = timeout(Client::open(read, write, None)).await.unwrap_err();
    let Error::Rpc(e) = err else {
        panic!("expected Rpc, got {err:?}");
    };
    assert!(e.is_kind(error_kind::UNSUPPORTED_GENERATION));
    assert_eq!(e.hint(), Some("speak generation 1"));
}

#[tokio::test]
async fn missing_method_disables_only_its_feature() {
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    let server = tokio::spawn(async move {
        // No rpc.discover, no terminal.read.
        core.serve_hello(&["terminal.list"]).await;
        let req = core.read_request().await;
        core.reply(&req["id"], json!({"terminals": [], "hidden_finished": 0}))
            .await;
        core
    });
    let client = client_open(read, write).await;

    // discovery limitation is a note, not a failure
    discovery_settled(&client).await;
    assert!(client.discovery().is_none());
    assert!(
        client.discovery_note().unwrap().contains("rpc.discover"),
        "note explains the missing method"
    );

    let err = timeout(client.terminal_read(ReadParams {
        terminal: "t7".into(),
        lines: 0,
        caller: None,
    }))
    .await
    .unwrap_err();
    let CallError::Unsupported { method, reason } = err else {
        panic!("expected Unsupported, got {err:?}");
    };
    assert_eq!(method, "terminal.read");
    assert!(reason.contains("terminal.read"), "{reason}");

    // An offered method still works on the same connection.
    let list = timeout(client.terminal_list(ListParams::default()))
        .await
        .unwrap();
    assert!(list.terminals.is_empty());
    timeout(server).await.unwrap();
}

#[tokio::test]
async fn core_error_surfaces_with_kind_and_hint() {
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.get"]).await;
        let req = core.read_request().await;
        core.reply_error(&req["id"], error_kind::NOT_FOUND, "no terminal t9")
            .await;
        core
    });
    let client = client_open(read, write).await;
    let err = timeout(client.terminal_get(TerminalParams {
        terminal: "t9".into(),
        caller: None,
    }))
    .await
    .unwrap_err();
    assert!(err.is_not_found());
    let CallError::Rpc(e) = err else {
        panic!("expected Rpc, got {err:?}");
    };
    assert_eq!(e.kind(), Some("not_found"));
    assert_eq!(e.hint(), Some("no terminal t9"));
    timeout(server).await.unwrap();
}

#[tokio::test]
async fn typed_result_shape_is_checked() {
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.list"]).await;
        let req = core.read_request().await;
        // `terminals` must be an array; a number fails the typed decode.
        core.reply(&req["id"], json!({"terminals": 3, "hidden_finished": 0}))
            .await;
        core
    });
    let client = client_open(read, write).await;
    let err = timeout(client.terminal_list(ListParams::default()))
        .await
        .unwrap_err();
    assert!(matches!(err, CallError::Shape(_)), "{err:?}");
    timeout(server).await.unwrap();
}

#[tokio::test]
async fn oversized_request_is_rejected_before_encoding() {
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(&["terminal.run"]).await;
        core
    });
    let client = client_open(read, write).await;
    let huge = "x".repeat(20 << 20);
    let err = timeout(client.call("terminal.run", Some(json!({"line": huge}))))
        .await
        .unwrap_err();
    assert!(matches!(err, CallError::TooLarge { .. }), "{err:?}");
    timeout(server).await.unwrap();
}

#[tokio::test]
async fn non_response_hello_reply_is_a_shape_error() {
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    tokio::spawn(async move {
        core.read_hello().await;
        core.send_line(&json!({"jsonrpc": "2.0", "method": "event", "params": {}}))
            .await;
    });
    let err = timeout(Client::open(read, write, None)).await.unwrap_err();
    assert!(matches!(err, Error::Shape(_)), "{err:?}");
}

#[tokio::test]
async fn eof_during_hello_is_closed() {
    let (core, read, write) = fakecore::pair(64 * 1024);
    tokio::spawn(async move {
        drop(core); // the peer vanishes mid-handshake
    });
    let err = timeout(Client::open(read, write, None)).await.unwrap_err();
    assert!(matches!(err, Error::Closed | Error::Codec(_)), "{err:?}");
}

#[tokio::test]
async fn typed_calls_cover_the_whole_surface() {
    let (mut core, read, write) = fakecore::pair(64 * 1024);
    let server = tokio::spawn(async move {
        core.serve_hello(fakecore::ALL_METHODS).await;
        // rpc.discover can interleave with the six typed calls.
        for _ in 0..7 {
            let req = core.read_request().await;
            let result = match req["method"].as_str().unwrap() {
                "rpc.discover" => fakecore::discover_doc(fakecore::ALL_METHODS),
                "terminal.get" | "terminal.kill" => common::minimal_terminal(),
                "terminal.run" => {
                    let mut t = common::minimal_terminal();
                    t["name"] = json!("fresh");
                    t
                }
                "terminal.read" => {
                    json!({"text": "screen text", "complete": true, "how": "screen"})
                }
                "doc.get" => json!({"value": {"key": 1}, "version": 7}),
                "doc.set" => {
                    assert_eq!(
                        req["params"],
                        json!({"name":"hc-tui","on":"host-a","value":{"focus":"t1"},"version":7})
                    );
                    json!({"version":8})
                }
                other => panic!("unexpected method {other}"),
            };
            core.reply(&req["id"], result).await;
        }
        core
    });
    let client = client_open(read, write).await;

    let got = timeout(client.terminal_get(TerminalParams {
        terminal: "t7".into(),
        caller: None,
    }))
    .await
    .unwrap();
    assert_eq!(got.address, "t7");
    let ran = timeout(client.terminal_run(Default::default()))
        .await
        .unwrap();
    assert_eq!(ran.terminal.name.as_deref(), Some("fresh"));
    let read_out = timeout(client.terminal_read(ReadParams {
        terminal: "t7".into(),
        lines: 5,
        caller: None,
    }))
    .await
    .unwrap();
    assert_eq!(read_out.text, "screen text");
    assert_eq!(read_out.how, "screen");
    let killed = timeout(client.terminal_kill(TerminalParams {
        terminal: "t7".into(),
        caller: None,
    }))
    .await
    .unwrap();
    assert_eq!(killed.address, "t7");
    let doc = timeout(client.doc_get(DocGetParams {
        name: "layout".into(),
        on: None,
    }))
    .await
    .unwrap();
    assert_eq!(doc.version, 7);
    assert_eq!(doc.value["key"], 1);
    let written = timeout(client.doc_set(DocSetParams {
        name: "hc-tui".into(),
        on: Some("host-a".into()),
        value: json!({"focus":"t1"}),
        version: doc.version,
    }))
    .await
    .unwrap();
    assert_eq!(written.version, 8);
    timeout(server).await.unwrap();
}
