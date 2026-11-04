//! Protocol constants and types for speaking to `hc`, the herdr-core daemon.
//!
//! Milestone M0 fixes only the protocol generation this client speaks.
//! Request/response types, transport and reconnect arrive with milestone M1.

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
