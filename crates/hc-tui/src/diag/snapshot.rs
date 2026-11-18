//! What a snapshot reads, and the JSON it builds. Every number is a counter
//! or a gauge the client already keeps; nothing comes from the terminal,
//! the screen or the endpoint's target.

use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering::Relaxed;

use hc_proto::client::AttachConn;
use hc_proto::endpoint::Endpoint;
use hc_proto::stats::{ConnSnapshot, ConnStats};
use serde_json::{Map, Value, json};

use crate::app::{PANE_ROLE_NAMES, Probe, WAKE_NAMES};

/// What a snapshot reads. The transport is recorded by kind only, so no
/// `--remote` target can reach a snapshot.
pub struct Sources {
    probe: Arc<Probe>,
    transport: &'static str,
    subscription: Arc<ConnStats>,
    rpc: Arc<ConnStats>,
    attach: AttachConn,
}

impl Sources {
    pub fn new(
        probe: Arc<Probe>,
        endpoint: &Endpoint,
        subscription: Arc<ConnStats>,
        rpc: Arc<ConnStats>,
        attach: AttachConn,
    ) -> Sources {
        let transport = match endpoint {
            Endpoint::Local(_) => "local",
            Endpoint::Remote(_) => "remote",
        };
        Sources {
            probe,
            transport,
            subscription,
            rpc,
            attach,
        }
    }

    /// The snapshot object, `uptime_ms` into the session.
    pub(super) fn snapshot(&self, uptime_ms: u64) -> Value {
        let probe = &self.probe;
        let mut wakeups = named(&WAKE_NAMES, &probe.wake_kinds);
        wakeups.insert("total".into(), json!(probe.wakeups.load(Relaxed)));
        let gauges = self.attach.gauges();
        json!({
            "schema": super::SCHEMA,
            "uptime_ms": uptime_ms,
            "loop": {
                "wakeups": wakeups,
                "dirty_marks": named(&WAKE_NAMES, &probe.dirty_marks),
                "draws": probe.draws.load(Relaxed),
                "elided_draws": probe.elided_draws.load(Relaxed),
                "projections": probe.projections.load(Relaxed),
                "cells_painted": probe.cells_painted.load(Relaxed),
                "terminal_writes": probe.terminal_writes.load(Relaxed),
                "terminal_bytes": probe.terminal_bytes.load(Relaxed),
                "stdin_reads": probe.stdin_reads.load(Relaxed),
                "stdin_empty_reads": probe.stdin_empty_reads.load(Relaxed),
            },
            "panes": {
                "by_role": named(&PANE_ROLE_NAMES, &probe.panes),
                "attach_calls_in_flight": gauges.calls_in_flight,
                "channels": {
                    "live": gauges.channels_live,
                    "held": gauges.channels_held,
                    "dead": gauges.channels_dead,
                },
            },
            "endpoints": [{"index": 0, "transport": self.transport}],
            "connections": [
                connection("subscription", "events.subscribe", &self.subscription.snapshot()),
                connection("shared_rpc", "json-rpc", &self.rpc.snapshot()),
                connection(
                    "attach_mux",
                    "terminal.attach",
                    &self.attach.stats().snapshot(),
                ),
            ],
        })
    }
}

/// Counter slots keyed by their snapshot names, in slot order.
fn named(names: &[&str], slots: &[AtomicU64]) -> Map<String, Value> {
    names
        .iter()
        .zip(slots)
        .map(|(name, slot)| ((*name).to_owned(), json!(slot.load(Relaxed))))
        .collect()
}

/// One connection kind on the only endpoint, by its protocol id.
fn connection(kind: &str, protocol: &str, stats: &ConnSnapshot) -> Value {
    json!({
        "endpoint": 0,
        "kind": kind,
        "protocol": protocol,
        "reads": stats.reads,
        "empty_reads": stats.empty_reads,
        "bytes_in": stats.bytes_in,
        "writes": stats.writes,
        "bytes_out": stats.bytes_out,
        "connects": stats.connects,
        "reconnects": stats.reconnects,
    })
}
