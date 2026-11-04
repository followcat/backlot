//! JSON-RPC envelope tests: versions, member dispatch, ids and structured
//! errors. Pure parsing; no IO.

use hc_proto::jsonrpc::{Id, Message, ParseError, Request, Response, RpcError, error_kind};
use serde_json::json;

fn parse(bytes: &[u8]) -> Result<Message, ParseError> {
    Message::parse(bytes)
}

#[test]
fn request_serializes_with_id_zero_and_trailing_newline() {
    let request = Request::new(Id::Num(0), "terminal.list", Some(json!({"all": true})));
    let line = request.to_line();
    assert_eq!(line.last(), Some(&b'\n'));
    let text = String::from_utf8(line).unwrap();
    // id = 0 is a real correlation id, never omitted.
    assert!(text.contains("\"id\":0"), "{text}");
    assert!(text.contains("\"jsonrpc\":\"2.0\""), "{text}");
    assert!(text.contains("\"method\":\"terminal.list\""), "{text}");
}

#[test]
fn request_without_params_omits_the_member() {
    let request = Request::new(Id::Num(4), "machine.list", None);
    let text = String::from_utf8(request.to_line()).unwrap();
    assert!(!text.contains("params"), "{text}");
}

#[test]
fn response_result_parses() {
    let msg = parse(br#"{"jsonrpc":"2.0","id":7,"result":{"seq":42}}"#).unwrap();
    let Message::Response(Response::Ok(ok)) = msg else {
        panic!("expected result response: {msg:?}");
    };
    assert_eq!(ok.id, Id::Num(7));
    assert_eq!(ok.result["seq"], 42);
}

#[test]
fn error_response_keeps_code_kind_and_hint() {
    let msg = parse(
        br#"{"jsonrpc":"2.0","id":3,"error":{"code":-32000,"message":"no terminal t9","data":{"kind":"not_found","hint":"list them with terminal.list","attempts":2}}}"#,
    )
    .unwrap();
    let Message::Response(Response::Err(err)) = msg else {
        panic!("expected error response: {msg:?}");
    };
    assert_eq!(err.id, Some(Id::Num(3)));
    assert_eq!(err.error.code, -32000);
    assert_eq!(err.error.message, "no terminal t9");
    assert_eq!(err.error.kind(), Some(error_kind::NOT_FOUND));
    assert_eq!(err.error.hint(), Some("list them with terminal.list"));
    assert!(err.error.is_kind(error_kind::NOT_FOUND));
    // Unknown `data` members are retained, not dropped.
    assert_eq!(err.error.data.as_ref().unwrap()["attempts"], 2);
}

#[test]
fn error_response_with_null_id_parses() {
    let msg =
        parse(br#"{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"parse error"}}"#)
            .unwrap();
    let Message::Response(Response::Err(err)) = msg else {
        panic!("expected error response: {msg:?}");
    };
    assert_eq!(err.id, None);
    assert_eq!(err.error.kind(), None);
    assert_eq!(err.error.message, "parse error");
}

#[test]
fn error_round_trip_preserves_structure() {
    let err = RpcError::new(-32000, "not_found", "no terminal t9", "list them");
    let value = json!({"jsonrpc": "2.0", "id": 9, "error": err});
    let msg = Message::from_value(value).unwrap();
    let Message::Response(Response::Err(back)) = msg else {
        panic!("expected error response: {msg:?}");
    };
    assert_eq!(back.id, Some(Id::Num(9)));
    assert_eq!(back.error, err);
}

#[test]
fn response_into_result_splits_ok_and_err() {
    let ok: Response =
        serde_json::from_value(json!({"jsonrpc":"2.0","id":1,"result":[1,2]})).unwrap();
    assert_eq!(ok.into_result().unwrap(), json!([1, 2]));
    let err: Response = serde_json::from_value(
        json!({"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"x"}}),
    )
    .unwrap();
    assert_eq!(err.into_result().unwrap_err().message, "x");
}

#[test]
fn notification_parses() {
    let msg = parse(br#"{"jsonrpc":"2.0","method":"heartbeat","params":{"seq":42}}"#).unwrap();
    let Message::Notification(n) = msg else {
        panic!("expected notification: {msg:?}");
    };
    assert_eq!(n.method, "heartbeat");
    assert_eq!(n.params.as_ref().unwrap()["seq"], 42);
}

#[test]
fn inbound_request_parses() {
    let msg = parse(
        br#"{"jsonrpc":"2.0","id":"abc","method":"terminal.get","params":{"terminal":"t1"}}"#,
    )
    .unwrap();
    let Message::Request(r) = msg else {
        panic!("expected request: {msg:?}");
    };
    assert_eq!(r.id, Id::Str("abc".into()));
    assert_eq!(r.method, "terminal.get");
}

#[test]
fn string_and_numeric_ids_correlate() {
    let a: Id = serde_json::from_value(json!(0)).unwrap();
    let b: Id = serde_json::from_value(json!("abc")).unwrap();
    assert_eq!(a, Id::Num(0));
    assert_eq!(b, Id::Str("abc".into()));
    // Response::id() surfaces the id for correlation either way.
    let r: Response =
        serde_json::from_value(json!({"jsonrpc":"2.0","id":"abc","result":null})).unwrap();
    assert_eq!(r.id(), Some(&Id::Str("abc".into())));
}

#[test]
fn malformed_envelopes_are_rejected() {
    // Not JSON at all.
    assert!(matches!(parse(b"{nope").unwrap_err(), ParseError::Json(_)));
    // Not an object.
    assert!(matches!(
        parse(b"[1,2]").unwrap_err(),
        ParseError::Malformed(_)
    ));
    // Missing jsonrpc.
    assert!(matches!(
        parse(br#"{"id":1,"result":{}}"#).unwrap_err(),
        ParseError::Version(None)
    ));
    // Wrong version.
    assert!(matches!(
        parse(br#"{"jsonrpc":"1.0","id":1,"result":{}}"#).unwrap_err(),
        ParseError::Version(Some(v)) if v == "1.0"
    ));
    // Both result and error.
    assert!(matches!(
        parse(br#"{"jsonrpc":"2.0","id":1,"result":{},"error":{"code":1,"message":"x"}}"#)
            .unwrap_err(),
        ParseError::Malformed(_)
    ));
    // Neither method nor id/result/error.
    assert!(matches!(
        parse(br#"{"jsonrpc":"2.0"}"#).unwrap_err(),
        ParseError::Malformed(_)
    ));
    // Method with null id is no notification and no request.
    assert!(matches!(
        parse(br#"{"jsonrpc":"2.0","method":"event","id":null}"#).unwrap_err(),
        ParseError::Malformed(_)
    ));
    // Result with null id is not a legal response.
    assert!(matches!(
        parse(br#"{"jsonrpc":"2.0","id":null,"result":{}}"#).unwrap_err(),
        ParseError::Invalid(_)
    ));
    // Result without any id cannot be correlated.
    assert!(matches!(
        parse(br#"{"jsonrpc":"2.0","result":{}}"#).unwrap_err(),
        ParseError::Malformed(_)
    ));
}
