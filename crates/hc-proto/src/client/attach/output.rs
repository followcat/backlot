// Derived from herdr-core crates/herdr-core/src/viewers.rs and wire.rs (Apache-2.0).

//! The attach stream's public event surface: what `recv` yields and how a
//! stream end reports its reason.

use serde_json::Value;

/// A stream event, in wire order.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    /// Terminal bytes; consecutive DATA frames coalesce.
    Data(Vec<u8>),
    /// A core→observer grid change ahead of the paint it replaces —
    /// guaranteed only when `resize_notifications` was confirmed.
    Resize { cols: u16, rows: u16 },
    /// The core ended the stream: its reason, parsed or raw.
    Closed(CloseReason),
    /// A kind this version does not define, or a frame for another
    /// channel — surfaced, never fatal.
    Control {
        /// The channel it arrived on.
        channel: u32,
        /// The frame's kind byte.
        kind: u8,
        /// The untouched payload.
        payload: Vec<u8>,
    },
    /// The wire failed (EOF, transport or protocol violation). Terminal.
    Failed(String),
}

/// Why a stream ended: the core's reason with unknown fields preserved.
#[derive(Debug, Clone, PartialEq)]
pub enum CloseReason {
    /// Empty payload — the ack of a client's own CLOSE, or a bare end.
    Empty,
    /// The core's JSON reason (exit, loss, ...); every field kept.
    Json(Value),
    /// A non-JSON payload, preserved byte-for-byte.
    Raw(Vec<u8>),
}

impl CloseReason {
    pub(super) fn parse(payload: &[u8]) -> CloseReason {
        if payload.is_empty() {
            return CloseReason::Empty;
        }
        serde_json::from_slice(payload)
            .map(CloseReason::Json)
            .unwrap_or_else(|_| CloseReason::Raw(payload.to_vec()))
    }
}
