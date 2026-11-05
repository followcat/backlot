//! Conformance checks between our hand-written types and the pinned OpenRPC
//! schema in `tests/fixtures/hc.openrpc.json` (herdr-core
//! `ec8adcdf0da38e539e8702f7c37801647da035f3`, Apache-2.0 — see
//! `fixtures/README.md`). They catch renamed/removed fields and methods the
//! schema does not declare, in both directions where feasible.

use hc_proto::doc::{DocGetParams, DocGetResult, DocSetParams, DocSetResult};
use hc_proto::events::{SubscribeParams, SubscribeResult};
use hc_proto::hello::{CoreInfo, HelloParams, HelloResult};
use hc_proto::machine::MachineListResult;
use hc_proto::method;
use hc_proto::terminal::{
    AttachParams, AttachResult, Display, InitialSize, ListParams, ListResult, ReadParams,
    ReadResult, RunParams, RunResult, Terminal, TerminalParams,
};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

const SCHEMA_TEXT: &str = include_str!("fixtures/hc.openrpc.json");

fn schema() -> Value {
    serde_json::from_str(SCHEMA_TEXT).unwrap()
}

fn method_schema<'m>(schema: &'m Value, name: &str) -> &'m Value {
    schema["methods"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == name)
        .unwrap_or_else(|| panic!("method {name} not in schema"))
}

#[test]
fn fixture_is_the_pinned_schema_with_neutral_examples() {
    // Neutral substitutions documented in fixtures/README.md stayed in place.
    // (The upstream example names deliberately appear nowhere in this repo.)
    assert!(SCHEMA_TEXT.contains("host-b"));
    assert!(SCHEMA_TEXT.contains("alice@host-b"));
    let schema = schema();
    assert_eq!(schema["openrpc"], "1.3.2");
    assert_eq!(schema["info"]["title"], "herdr-core");
}

#[test]
fn modeled_methods_exist_and_are_by_name() {
    for name in [
        method::HELLO,
        method::RPC_DISCOVER,
        method::TERMINAL_LIST,
        method::TERMINAL_GET,
        method::TERMINAL_RUN,
        method::TERMINAL_READ,
        method::TERMINAL_KILL,
        method::TERMINAL_ATTACH,
        method::EVENTS_SUBSCRIBE,
        method::MACHINE_LIST,
        method::DOC_GET,
    ] {
        let schema = schema();
        let m = method_schema(&schema, name);
        assert_eq!(m["paramStructure"], "by-name", "{name}");
    }
}

/// Every key our serialized params emit must be a declared schema param, and
/// every required schema param must be emitted by the populated struct.
fn check_params(method: &str, params: &impl Serialize) {
    let schema = schema();
    let m = method_schema(&schema, method);
    let declared: BTreeSet<String> = m["params"]
        .as_array()
        .map(|p| {
            p.iter()
                .map(|p| p["name"].as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default();
    let required: BTreeSet<String> = m["params"]
        .as_array()
        .map(|p| {
            p.iter()
                .filter(|p| p["required"] == true)
                .map(|p| p["name"].as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default();
    let sent: BTreeSet<String> = serde_json::to_value(params)
        .unwrap()
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    let undeclared: Vec<_> = sent.difference(&declared).collect();
    let missing: Vec<_> = required.difference(&sent).collect();
    assert!(
        undeclared.is_empty(),
        "{method}: params not in schema: {undeclared:?}"
    );
    assert!(
        missing.is_empty(),
        "{method}: required params not sent: {missing:?}"
    );
}

#[test]
fn typed_params_match_schema() {
    check_params(
        method::HELLO,
        &HelloParams {
            generation: Some(1),
            framed: true,
            core: Some(CoreInfo {
                id: "c".into(),
                name: "n".into(),
                hostname: None,
            }),
            calls_you: Some("host-b".into()),
            features: vec!["f".into()],
        },
    );
    check_params(
        method::TERMINAL_LIST,
        &ListParams {
            all: true,
            labels: BTreeMap::from([("k".into(), "v".into())]),
        },
    );
    check_params(
        method::TERMINAL_GET,
        &TerminalParams {
            terminal: "t7".into(),
            caller: Some("t1".into()),
        },
    );
    check_params(
        method::TERMINAL_KILL,
        &TerminalParams {
            terminal: "t7".into(),
            caller: Some("t1".into()),
        },
    );
    check_params(
        method::TERMINAL_READ,
        &ReadParams {
            terminal: "t7".into(),
            lines: 5,
            caller: Some("t1".into()),
        },
    );
    check_params(
        method::TERMINAL_RUN,
        &RunParams {
            argv: vec!["agent".into()],
            caller: Some("t1".into()),
            cwd: Some("/home/user".into()),
            die_with_parent: true,
            display: Some(Display {
                bg: Some("#000000".into()),
                cell: Some([9, 18]),
                cursor: None,
                fg: None,
            }),
            env: BTreeMap::from([("E".into(), "1".into())]),
            keep: true,
            labels: BTreeMap::from([("herdr.space".into(), "acme-shop".into())]),
            line: Some("make test".into()),
            name: Some("agent-1".into()),
            no_notify: true,
            no_parent: true,
            on: Some("host-b".into()),
            outlive_parent: true,
            parent: Some("t0".into()),
            rm: true,
            size: Some(InitialSize { cols: 80, rows: 24 }),
            timeout: Some(5.5),
            wait: true,
        },
    );
    check_params(
        method::TERMINAL_ATTACH,
        &AttachParams {
            terminal: "t7".into(),
            observe: true,
            resize_notifications: true,
            history: 10,
            cols: 80,
            rows: 24,
            display: Some(Display::default()),
            resumed: true,
            caller: Some("t1".into()),
        },
    );
    check_params(
        method::EVENTS_SUBSCRIBE,
        &SubscribeParams {
            terminal: Some("t7".into()),
            children_of: Some("t1".into()),
            labels: BTreeMap::from([("k".into(), "v".into())]),
        },
    );
    check_params(
        method::DOC_GET,
        &DocGetParams {
            name: "hc-tui".into(),
            on: Some("host-b".into()),
        },
    );

    check_params(
        method::DOC_SET,
        &DocSetParams {
            name: "hc-tui".into(),
            on: None,
            value: serde_json::json!({"schema":1}),
            version: 0,
        },
    );

    // Methods we call without a params struct must not require any.
    for method in [method::MACHINE_LIST, method::RPC_DISCOVER] {
        let schema = schema();
        let m = method_schema(&schema, method);
        let required = m["params"]
            .as_array()
            .map(|p| p.iter().filter(|p| p["required"] == true).count())
            .unwrap_or(0);
        assert_eq!(required, 0, "{method} gained required params");
    }
}

/// Builds the smallest value satisfying a schema: required object fields and
/// the empty variant of every nullable/union type. It exercises exactly what
/// the wire guarantees, so `#[serde(default)]` gaps surface.
fn minimal(schema: &Value, components: &Value, depth: usize) -> Value {
    assert!(depth < 20, "schema recursion too deep");
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let name = reference
            .strip_prefix("#/components/schemas/")
            .expect("component ref");
        return minimal(&components[name], components, depth + 1);
    }
    if let Some(any_of) = schema.get("anyOf").and_then(Value::as_array) {
        if any_of.iter().any(|s| s["type"] == "null") {
            return Value::Null;
        }
        return minimal(&any_of[0], components, depth + 1);
    }
    if let Some(variants) = schema.get("enum").and_then(Value::as_array) {
        return variants[0].clone();
    }
    let ty = match schema.get("type") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(types)) => {
            if types.iter().any(|t| t == "null") {
                return Value::Null;
            }
            types[0].as_str().unwrap().to_string()
        }
        _ if schema.get("properties").is_some() => "object".to_string(),
        _ => return Value::Null,
    };
    match ty.as_str() {
        "object" => {
            let mut obj = Map::new();
            let empty = Map::new();
            let required = schema["required"].as_array().cloned().unwrap_or_default();
            let properties = schema["properties"].as_object().unwrap_or(&empty);
            for name in required.iter().filter_map(Value::as_str) {
                obj.insert(
                    name.into(),
                    minimal(&properties[name], components, depth + 1),
                );
            }
            Value::Object(obj)
        }
        "string" => Value::String("x".into()),
        "integer" | "number" => json!(0),
        "boolean" => json!(false),
        "array" => {
            let n = schema["minItems"].as_u64().unwrap_or(0) as usize;
            let item = schema
                .get("items")
                .map(|s| minimal(s, components, depth + 1))
                .unwrap_or(Value::Null);
            Value::Array(vec![item; n])
        }
        _ => Value::Null,
    }
}

/// Resolves a method's result schema to its component definition and builds
/// the minimal legal result.
fn minimal_result(method: &str) -> Value {
    let schema = schema();
    let result = &method_schema(&schema, method)["result"]["schema"];
    minimal(result, &schema["components"]["schemas"], 0)
}

fn decodes<T: serde::de::DeserializeOwned>(method: &str) -> T {
    let value = minimal_result(method);
    serde_json::from_value(value.clone())
        .unwrap_or_else(|e| panic!("{method}: minimal result fails: {e}\n{value}"))
}

#[test]
fn minimal_results_deserialize_into_our_types() {
    let _: HelloResult = decodes(method::HELLO);
    let _: ListResult = decodes(method::TERMINAL_LIST);
    let _: Terminal = decodes(method::TERMINAL_GET);
    let _: Terminal = decodes(method::TERMINAL_KILL);
    let _: ReadResult = decodes(method::TERMINAL_READ);
    let _: RunResult = decodes(method::TERMINAL_RUN);
    let _: AttachResult = decodes(method::TERMINAL_ATTACH);
    let _: SubscribeResult = decodes(method::EVENTS_SUBSCRIBE);
    let _: MachineListResult = decodes(method::MACHINE_LIST);
    let _: DocGetResult = decodes(method::DOC_GET);
    let _: DocSetResult = decodes(method::DOC_SET);
}
