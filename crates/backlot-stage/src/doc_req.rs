//! Stage consumer for `plugin.backlot.req` -> `plugin.backlot.res` (B1a).
//!
//! Wakes immediately when `events.subscribe` reports a local `doc` event
//! for `plugin.backlot.req`, polls once on startup and once per minute,
//! executes unhandled requests as `Caller::doc_user` (`by.via = "doc"`),
//! writes capped acks to `plugin.backlot.res`, and CAS-drains handled entries
//! without losing concurrent client appends.

use std::collections::HashSet;
use std::path::Path;

pub(crate) use backlot_model::req_wire::{CARD_QUEUE_REFUSAL, person_only};
use backlot_model::req_wire::{
    MAX_DOC_BYTES, MAX_REQ_QUEUE, MAX_RES_ACKS, REQ_DOC_NAME, REQ_SCHEMA, RES_DOC_NAME, RES_SCHEMA,
    ReqDoc, ReqEnvelope, ResAck, ResDoc, validate_timestamp,
};
use hc_proto::client::{CallError, Client};
use hc_proto::doc::{DocGetParams, DocSetParams};
use hc_proto::events::{ServerNotification, event_kind};
use hc_proto::jsonrpc::error_kind;
use serde_json::Value;

use crate::cli::{Env, Fail};
use crate::daemon::Stage;
use crate::doc_dedupe::DedupeStore;
use crate::doc_dispatch::dispatch_action;

/// Fallback poll cadence if a core notification is missed (60 s).
pub(crate) const POLL_INTERVAL_MS: u64 = 60_000;

const MAX_CAS_ATTEMPTS: usize = 8;

/// Whether a core notification is a local `doc` update on `plugin.backlot.req`
/// (or a `missed` gap notification requiring a queue re-check).
pub(crate) fn is_req_wake_event(notif: &ServerNotification) -> bool {
    match notif {
        ServerNotification::Event(ev) => {
            ev.kind == event_kind::DOC
                && ev.machine.is_none()
                && ev.data.get("name").and_then(Value::as_str) == Some(REQ_DOC_NAME)
        }
        ServerNotification::Missed(_) => true,
        ServerNotification::Heartbeat(_) => false,
    }
}

/// Request queue consumer + durable dedupe state owned by the stage daemon.
pub(crate) struct Consumer {
    dedupe: DedupeStore,
}

impl Consumer {
    pub(crate) fn load(base: &Path, now_ms: u64) -> Self {
        Consumer {
            dedupe: DedupeStore::load(base, now_ms),
        }
    }

    #[cfg(test)]
    pub(crate) fn dedupe_mut(&mut self) -> &mut DedupeStore {
        &mut self.dedupe
    }

    /// Read `plugin.backlot.req`, execute unhandled entries, write acks to
    /// `plugin.backlot.res`, and CAS-drain handled items. Returns `true` when
    /// one or more queue entries were drained.
    pub(crate) async fn process(&mut self, stage: &Stage, now_ms: u64) -> Result<bool, Fail> {
        let _ = self.dedupe.sync_prune(now_ms);
        let got = stage
            .client
            .doc_get(DocGetParams {
                name: REQ_DOC_NAME.into(),
                on: None,
            })
            .await
            .map_err(Fail::call)?;
        if got.value.is_null() {
            return Ok(false);
        }

        let entries = extract_queue(&got.value);
        if entries.is_empty() {
            return Ok(false);
        }

        let env = Env {
            vars: stage.env_vars.clone(),
        };
        let mut new_acks: Vec<ResAck> = Vec::new();
        let mut handled_ids: HashSet<String> = HashSet::new();
        let mut handled_corrupt: Vec<Value> = Vec::new();

        for (idx, raw) in entries.iter().enumerate() {
            let envelope = match serde_json::from_value::<ReqEnvelope>(raw.clone()) {
                Ok(e) if !e.req_id.trim().is_empty() => e,
                _ => {
                    if let Some(req_id) = raw.get("req_id").and_then(Value::as_str)
                        && !req_id.trim().is_empty()
                    {
                        handled_ids.insert(req_id.to_string());
                        if !self.dedupe.contains(req_id) {
                            let ack = ResAck::err(req_id, now_ms, "malformed request envelope");
                            self.dedupe.record(ack.clone(), now_ms)?;
                            new_acks.push(ack);
                        }
                    } else {
                        handled_corrupt.push(raw.clone());
                    }
                    continue;
                }
            };

            handled_ids.insert(envelope.req_id.clone());

            if self.dedupe.contains(&envelope.req_id) {
                if let Some(prev) = self.dedupe.cached_ack(&envelope.req_id) {
                    new_acks.push(prev.clone());
                } else {
                    new_acks.push(ResAck::err(&envelope.req_id, now_ms, "duplicate req_id"));
                }
                continue;
            }

            let ack = if idx >= MAX_REQ_QUEUE {
                ResAck::err(&envelope.req_id, now_ms, "server busy")
            } else if let Err(msg) = validate_timestamp(envelope.ts_ms, now_ms) {
                ResAck::err(&envelope.req_id, now_ms, msg)
            } else {
                match envelope.parse_action() {
                    Ok(action) if person_only(&action) => {
                        ResAck::err(&envelope.req_id, now_ms, CARD_QUEUE_REFUSAL)
                    }
                    Ok(action) => match dispatch_action(&stage.base, &env, action) {
                        Ok(result) => ResAck::ok_value(&envelope.req_id, now_ms, result),
                        Err(e) => ResAck::err(&envelope.req_id, now_ms, e.message()),
                    },
                    Err(msg) => ResAck::err(&envelope.req_id, now_ms, msg),
                }
            };

            self.dedupe.record(ack.clone(), now_ms)?;
            new_acks.push(ack);
        }

        if !new_acks.is_empty() {
            write_res_acks(&stage.client, new_acks).await?;
        }
        drain_handled(
            &stage.client,
            got.version,
            &entries,
            &handled_ids,
            &handled_corrupt,
        )
        .await?;
        Ok(!entries.is_empty())
    }
}

fn extract_queue(value: &Value) -> Vec<Value> {
    value
        .get("queue")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

async fn write_res_acks(client: &Client, new_acks: Vec<ResAck>) -> Result<(), Fail> {
    for _ in 0..MAX_CAS_ATTEMPTS {
        let got = client
            .doc_get(DocGetParams {
                name: RES_DOC_NAME.into(),
                on: None,
            })
            .await
            .map_err(Fail::call)?;
        let mut doc = if got.value.is_null() {
            ResDoc::default()
        } else {
            serde_json::from_value::<ResDoc>(got.value.clone()).unwrap_or_default()
        };
        doc.schema = RES_SCHEMA.into();
        for ack in &new_acks {
            if !doc
                .acks
                .iter()
                .any(|existing| existing.req_id == ack.req_id)
            {
                doc.acks.push(ack.clone());
            }
        }
        trim_res_doc(&mut doc);
        let value =
            serde_json::to_value(&doc).map_err(|e| Fail::refused(format!("res encode: {e}")))?;
        match client
            .doc_set(DocSetParams {
                name: RES_DOC_NAME.into(),
                on: None,
                value,
                version: got.version,
            })
            .await
        {
            Ok(_) => return Ok(()),
            Err(CallError::Rpc(err)) if err.is_kind(error_kind::CONFLICT) => continue,
            Err(e) => return Err(Fail::call(e)),
        }
    }
    Err(Fail::refused("conflict writing plugin.backlot.res"))
}

fn trim_res_doc(doc: &mut ResDoc) {
    if doc.acks.len() > MAX_RES_ACKS {
        let drop_n = doc.acks.len() - MAX_RES_ACKS;
        doc.acks.drain(0..drop_n);
    }
    // Guarantee serialized size stays well under herdr-core's 1 MiB document limit.
    let budget = MAX_DOC_BYTES / 2;
    while doc.acks.len() > 1 {
        let size = serde_json::to_vec(doc)
            .map(|b| b.len())
            .unwrap_or(usize::MAX);
        if size <= budget {
            break;
        }
        doc.acks.remove(0);
    }
}

async fn drain_handled(
    client: &Client,
    initial_version: u64,
    initial_entries: &[Value],
    handled_ids: &HashSet<String>,
    handled_corrupt: &[Value],
) -> Result<(), Fail> {
    let mut version = initial_version;
    let mut current_entries = initial_entries.to_vec();

    for attempt in 0..MAX_CAS_ATTEMPTS {
        let mut corrupt_rem = handled_corrupt.to_vec();
        let remaining: Vec<Value> = current_entries
            .into_iter()
            .filter(|raw| {
                if let Some(id) = raw.get("req_id").and_then(Value::as_str)
                    && handled_ids.contains(id)
                {
                    return false;
                }
                if let Some(pos) = corrupt_rem.iter().position(|c| c == raw) {
                    corrupt_rem.remove(pos);
                    return false;
                }
                true
            })
            .collect();

        let next = ReqDoc {
            schema: REQ_SCHEMA.into(),
            queue: remaining,
        };
        let value = serde_json::to_value(&next)
            .map_err(|e| Fail::refused(format!("req drain encode: {e}")))?;
        match client
            .doc_set(DocSetParams {
                name: REQ_DOC_NAME.into(),
                on: None,
                value,
                version,
            })
            .await
        {
            Ok(_) => return Ok(()),
            Err(CallError::Rpc(err))
                if err.is_kind(error_kind::CONFLICT) && attempt + 1 < MAX_CAS_ATTEMPTS =>
            {
                let got = client
                    .doc_get(DocGetParams {
                        name: REQ_DOC_NAME.into(),
                        on: None,
                    })
                    .await
                    .map_err(Fail::call)?;
                version = got.version;
                current_entries = extract_queue(&got.value);
            }
            Err(e) => return Err(Fail::call(e)),
        }
    }
    Err(Fail::refused("conflict draining plugin.backlot.req"))
}
