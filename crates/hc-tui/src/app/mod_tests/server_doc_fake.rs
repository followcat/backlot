//! Shared in-memory `doc.get`/`doc.set` + `events.subscribe` fake core
//! for B2 server-client integration tests.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use backlot_model::req_catalog::{CatalogDoc, HollywoodCfgCatalog};
use backlot_model::req_wire::{
    CAT_SCHEMA, REQ_DOC_NAME, RES_DOC_NAME, RES_SCHEMA, ReqDoc, ReqEnvelope, ResAck, ResDoc,
};
use hc_proto::frame::Kind;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::testcore;

#[derive(Default)]
pub(super) struct MemDocs {
    pub(super) docs: BTreeMap<String, (u64, Value)>,
    pub(super) auto_ack: Option<ResAck>,
}

pub(super) fn catalog_json(configured: bool) -> Value {
    let cat = CatalogDoc {
        schema: CAT_SCHEMA.into(),
        server_id: "srv-1".into(),
        ts_ms: 100,
        runners: Vec::new(),
        hollywood_cfg: HollywoodCfgCatalog {
            configured,
            runner: configured.then(|| "fake".into()),
            model: configured.then(|| "model-f".into()),
            effort: None,
            default_cast: None,
        },
        default_cast: None,
        archived: Vec::new(),
        home: "/home/user".into(),
    };
    serde_json::to_value(cat).unwrap()
}

pub(super) fn summary_json() -> Value {
    json!({
        "schema": "backlot/1",
        "updated_ms": 2,
        "studios": [{
            "id": "acme-shop",
            "title": "Shop",
            "state": "live",
            "requirement_version": 1,
            "mode": "standard",
            "open_reviews": 0,
            "waiting": 0,
            "tasks_total": 1,
            "tasks_done": 0,
            "roles": [{
                "role": "producer",
                "name": "producer",
                "title": "Producer",
                "runner": "fake",
                "state": "live",
                "terminal": "t1"
            }],
            "cards": []
        }]
    })
}

fn handle_rpc_req(store: &Mutex<MemDocs>, req: &Value) -> Result<Value, Value> {
    let method = req["method"].as_str().unwrap_or("");
    let mut st = store.lock().unwrap();
    match method {
        "doc.get" => {
            let name = req["params"]["name"].as_str().unwrap_or("");
            let (ver, val) = st.docs.get(name).cloned().unwrap_or((0, Value::Null));
            Ok(json!({ "value": val, "version": ver }))
        }
        "doc.set" => {
            let name = req["params"]["name"].as_str().unwrap_or("");
            let want_ver = req["params"]["version"].as_u64();
            let cur_ver = st.docs.get(name).map(|(v, _)| *v).unwrap_or(0);
            if want_ver.is_some_and(|v| v != cur_ver) {
                return Err(json!({
                    "code": -32000, "message": "version mismatch",
                    "data": { "kind": "conflict" }
                }));
            }
            let new_ver = cur_ver + 1;
            let val = req["params"]["value"].clone();
            st.docs.insert(name.to_owned(), (new_ver, val.clone()));
            if name == REQ_DOC_NAME
                && let Ok(req_doc) = serde_json::from_value::<ReqDoc>(val)
                && let Some(last) = req_doc.queue.last()
                && let Ok(env) = serde_json::from_value::<ReqEnvelope>(last.clone())
                && let Some(mut ack) = st.auto_ack.take()
            {
                ack.req_id = env.req_id;
                let res_doc = ResDoc {
                    schema: RES_SCHEMA.into(),
                    acks: vec![ack],
                };
                let rv = st.docs.get(RES_DOC_NAME).map(|(v, _)| *v).unwrap_or(0) + 1;
                st.docs.insert(
                    RES_DOC_NAME.into(),
                    (rv, serde_json::to_value(res_doc).unwrap()),
                );
            }
            Ok(json!({ "version": new_ver }))
        }
        "machine.list" => Ok(json!({ "machines": [] })),
        "terminal.list" => Ok(json!({ "terminals": [] })),
        _ => Ok(Value::Null),
    }
}

pub(super) fn spawn_doc_core_with(
    store: Arc<Mutex<MemDocs>>,
    sub_notify: Option<Arc<tokio::sync::Notify>>,
) -> (hc_proto::watch::OpenRpc, hc_proto::watch::OpenSub) {
    let (tx, rx) = mpsc::channel(16);
    let (rpc, sub) = testcore::openers(tx);
    testcore::spawn_fake(rx, move |mut core, framed| {
        let store = store.clone();
        let notify = sub_notify.clone();
        Box::pin(async move {
            if !framed {
                let _ =
                    testcore::serve_subscribe(&mut core, json!({"seq": 1, "terminals": []})).await;
                if let Some(n) = notify {
                    n.notified().await;
                    core.notify(
                        "event",
                        json!({
                            "seq": 2, "type": "doc",
                            "data": { "name": "backlot", "version": 2 }
                        }),
                    )
                    .await;
                }
                futures_lite::future::pending::<()>().await;
                return;
            }
            let methods = [
                "hello",
                "terminal.list",
                "machine.list",
                "doc.get",
                "doc.set",
                "events.subscribe",
            ];
            core.answer_hello(true, &methods, &[]).await;
            while let Some(f) = core.try_read_frame().await {
                if f.channel != 0 || f.kind != Kind::Data {
                    continue;
                }
                let req: Value = serde_json::from_slice(&f.payload).unwrap();
                match handle_rpc_req(&store, &req) {
                    Ok(val) => core.reply(&req["id"], val).await,
                    Err(err) => core.reply_error(&req["id"], err).await,
                }
            }
        })
    });
    (rpc, sub)
}
