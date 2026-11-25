//! Client-side CAS request helper for `plugin.backlot.req` -> `plugin.backlot.res`.
//!
//! Appends a single request atomically using compare-and-set (retrying up to
//! 3 times on `conflict`, refusing with `"server busy"` when `queue.len() >= 32`)
//! and waits for the matching ack in `plugin.backlot.res` up to `timeout`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use backlot_model::req_wire::{
    MAX_REQ_QUEUE, REQ_DOC_NAME, REQ_SCHEMA, RES_DOC_NAME, ReqAction, ReqDoc, ReqEnvelope, ResAck,
    ResDoc,
};
use hc_proto::client::{CallError, Client};
use hc_proto::doc::{DocGetParams, DocSetParams};
use hc_proto::endpoint::Endpoint;
use hc_proto::events::{ServerNotification, SubscribeParams, event_kind};
use hc_proto::jsonrpc::error_kind;
use hc_proto::subscribe::Subscription;
use serde::de::DeserializeOwned;

use crate::cli::Fail;

const MAX_CAS_ATTEMPTS: usize = 3;
const POLL_FALLBACK: Duration = Duration::from_millis(25);

static NEXT_REQ_SEQ: AtomicU64 = AtomicU64::new(1);

fn current_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn next_req_id(ts_ms: u64) -> String {
    let seq = NEXT_REQ_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("req-{ts_ms}-{}-{seq}", std::process::id())
}

/// CAS-append one request envelope onto `plugin.backlot.req` (up to 3 CAS
/// attempts), failing with `"server busy"` when `queue.len() >= 32`.
pub async fn append_request(client: &Client, envelope: &ReqEnvelope) -> Result<u64, Fail> {
    let env_val = serde_json::to_value(envelope)
        .map_err(|e| Fail::refused(format!("request does not serialize: {e}")))?;

    for attempt in 0..MAX_CAS_ATTEMPTS {
        let got = client
            .doc_get(DocGetParams {
                name: REQ_DOC_NAME.into(),
                on: None,
            })
            .await
            .map_err(Fail::call)?;
        let mut doc = if got.value.is_null() {
            ReqDoc::default()
        } else {
            serde_json::from_value::<ReqDoc>(got.value.clone())
                .map_err(|e| Fail::refused(format!("malformed {REQ_DOC_NAME}: {e}")))?
        };
        if doc.queue.len() >= MAX_REQ_QUEUE {
            return Err(Fail::refused("server busy"));
        }
        doc.schema = REQ_SCHEMA.into();
        doc.queue.push(env_val.clone());
        let value = serde_json::to_value(&doc)
            .map_err(|e| Fail::refused(format!("request queue does not serialize: {e}")))?;

        match client
            .doc_set(DocSetParams {
                name: REQ_DOC_NAME.into(),
                on: None,
                value,
                version: got.version,
            })
            .await
        {
            Ok(res) => return Ok(res.version),
            Err(CallError::Rpc(err))
                if err.is_kind(error_kind::CONFLICT) && attempt + 1 < MAX_CAS_ATTEMPTS =>
            {
                continue;
            }
            Err(CallError::Rpc(err)) if err.is_kind(error_kind::CONFLICT) => {
                return Err(Fail::refused("server busy"));
            }
            Err(e) => return Err(Fail::call(e)),
        }
    }
    Err(Fail::refused("server busy"))
}

/// Send one request via `plugin.backlot.req` and wait for its ack in
/// `plugin.backlot.res` up to `timeout`. When `endpoint` is provided,
/// subscribes to `doc` events for immediate wakeup instead of relying only
/// on polling.
pub async fn send_request(
    client: &Client,
    endpoint: Option<&Endpoint>,
    action: &ReqAction,
    confirm_token: Option<String>,
    timeout: Duration,
) -> Result<ResAck, Fail> {
    let ts_ms = current_ms();
    let req_id = next_req_id(ts_ms);
    let envelope = ReqEnvelope::new(req_id.clone(), ts_ms, action, confirm_token)
        .map_err(|e| Fail::refused(format!("request encode: {e}")))?;
    send_envelope(client, endpoint, &envelope, timeout).await
}

/// Send a pre-built [`ReqEnvelope`] and wait for its matching [`ResAck`].
pub async fn send_envelope(
    client: &Client,
    endpoint: Option<&Endpoint>,
    envelope: &ReqEnvelope,
    timeout: Duration,
) -> Result<ResAck, Fail> {
    let mut sub = match endpoint {
        Some(ep) => Subscription::connect(ep, None, &SubscribeParams::default())
            .await
            .ok(),
        None => None,
    };

    let _ = append_request(client, envelope).await?;
    let deadline = Instant::now() + timeout;

    loop {
        if let Some(ack) = find_ack(client, &envelope.req_id).await? {
            return Ok(ack);
        }
        let Some(rem) = deadline.checked_duration_since(Instant::now()) else {
            return Err(Fail::refused(format!(
                "timed out waiting for ack of '{}'",
                envelope.req_id
            )));
        };
        let wait = rem.min(POLL_FALLBACK);
        if let Some(s) = sub.as_mut() {
            match tokio::time::timeout(rem, s.recv()).await {
                Ok(Ok(ServerNotification::Event(ev)))
                    if ev.kind == event_kind::DOC
                        && ev.data.get("name").and_then(|n| n.as_str()) == Some(RES_DOC_NAME) => {}
                Ok(Ok(_)) => continue,
                Ok(Err(_)) => {
                    sub = None;
                    tokio::time::sleep(wait).await;
                }
                Err(_) => {
                    if let Some(ack) = find_ack(client, &envelope.req_id).await? {
                        return Ok(ack);
                    }
                    return Err(Fail::refused(format!(
                        "timed out waiting for ack of '{}'",
                        envelope.req_id
                    )));
                }
            }
        } else {
            tokio::time::sleep(wait).await;
        }
    }
}

/// Convenience helper that unpacks `ack.ok` and deserializes `ack.result` into `T`.
pub async fn send_typed<T: DeserializeOwned>(
    client: &Client,
    endpoint: Option<&Endpoint>,
    action: &ReqAction,
    confirm_token: Option<String>,
    timeout: Duration,
) -> Result<T, Fail> {
    let ack = send_request(client, endpoint, action, confirm_token, timeout).await?;
    if !ack.ok {
        return Err(Fail::refused(
            ack.error.unwrap_or_else(|| "request failed".into()),
        ));
    }
    let val = ack
        .result
        .ok_or_else(|| Fail::refused("missing result payload in ack"))?;
    serde_json::from_value(val).map_err(|e| Fail::refused(format!("bad result payload: {e}")))
}

async fn find_ack(client: &Client, req_id: &str) -> Result<Option<ResAck>, Fail> {
    let got = client
        .doc_get(DocGetParams {
            name: RES_DOC_NAME.into(),
            on: None,
        })
        .await
        .map_err(Fail::call)?;
    if got.value.is_null() {
        return Ok(None);
    }
    let doc = serde_json::from_value::<ResDoc>(got.value)
        .map_err(|e| Fail::refused(format!("malformed {RES_DOC_NAME}: {e}")))?;
    Ok(doc.acks.into_iter().find(|a| a.req_id == req_id))
}
