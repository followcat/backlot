//! Protocol types and wire codecs for speaking to `hc`, the herdr-core daemon.
//!
//! The API is a preview at protocol generation 1 (schema version 0.x), defined
//! by `schema/hc.openrpc.json` in the herdr-core source. A connection starts
//! in plain mode — one JSON-RPC 2.0 message per line — and `hello` with
//! `framed: true` switches it to binary frames for attach streams and events.
//! This crate covers the envelopes, the typed methods hc-tui uses, both
//! codecs, endpoint discovery, transport and the concurrent RPC client.
//! Subscription recovery and reconnect live one layer up.

pub mod client;
pub mod codec;
pub mod doc;
pub mod endpoint;
pub mod events;
pub mod frame;
pub mod hello;
pub mod jsonrpc;
pub mod machine;
pub mod terminal;
pub mod transport;

/// The `rpc.discover` result: the whole API as an OpenRPC document. Kept
/// opaque — its shape is the schema's own, not this crate's types.
pub type OpenRpcDocument = serde_json::Value;

/// The wire protocol generation this client speaks.
///
/// `hc` reports its protocol generation in the `hello` handshake reply.
/// Generation 1 is the current preview generation (schema version 0.x); a
/// client must check the peer's generation before issuing any further
/// requests.
pub const PROTOCOL_GENERATION: u32 = 1;

/// Returns `true` when `generation` is a protocol generation this client can
/// speak. Only [`PROTOCOL_GENERATION`] is supported.
pub fn is_supported_generation(generation: u32) -> bool {
    generation == PROTOCOL_GENERATION
}

/// JSON-RPC method names hc-tui relies on.
pub mod method {
    /// Handshake: generation, identity, method and feature lists, framed mode.
    pub const HELLO: &str = "hello";
    /// The whole API as an OpenRPC document.
    pub const RPC_DISCOVER: &str = "rpc.discover";
    /// Live terminals, optionally filtered by labels.
    pub const TERMINAL_LIST: &str = "terminal.list";
    /// One terminal's facts.
    pub const TERMINAL_GET: &str = "terminal.get";
    /// Start a program in a new terminal.
    pub const TERMINAL_RUN: &str = "terminal.run";
    /// The screen plus history lines as text.
    pub const TERMINAL_READ: &str = "terminal.read";
    /// End a terminal's program.
    pub const TERMINAL_KILL: &str = "terminal.kill";
    /// Open a raw stream to a live terminal (framed connections only).
    pub const TERMINAL_ATTACH: &str = "terminal.attach";
    /// Snapshot of matching terminals, then change notifications.
    pub const EVENTS_SUBSCRIBE: &str = "events.subscribe";
    /// The machines this core has added.
    pub const MACHINE_LIST: &str = "machine.list";
    /// Read an opaque JSON document and its version.
    pub const DOC_GET: &str = "doc.get";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_generation_is_one() {
        assert_eq!(PROTOCOL_GENERATION, 1);
    }

    #[test]
    fn only_generation_one_is_supported() {
        assert!(is_supported_generation(PROTOCOL_GENERATION));
        for generation in [0, 2, 3, u32::MAX] {
            assert!(!is_supported_generation(generation));
        }
    }
}
