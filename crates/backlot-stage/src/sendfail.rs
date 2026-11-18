//! What a failed `terminal.send` means to the stage.
//!
//! hc-tui's client already names the two cases a core answer or a
//! deadline can produce (`operations::rpc_actions::send_failure`); the
//! stage says the same words, because its log and the client's notice
//! must not disagree about whether a line was typed. Neither case may
//! ever be recorded as delivered: a cancelled send typed nothing, and
//! a timed-out one is still ambiguous.

use hc_proto::client::CallError;

/// hc-tui's wording for a cancelled send.
pub const CANCELLED: &str = "Send cancelled; nothing was typed. Send it again.";
/// hc-tui's wording for a client-side deadline.
pub const TIMED_OUT: &str = "Send timed out; the core may still type it.";

/// How the stage must treat a failed send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The core's `cancelled` kind: the caller went away or the core
    /// stopped before the call took effect, so nothing was typed.
    Cancelled,
    /// The client's deadline expired while the core may still be
    /// holding the send behind a person's unsent draft.
    TimedOut,
    /// Any other core refusal: nothing was typed, the core's own words.
    Other(String),
}

impl Refusal {
    /// Whether the line is known not to have been typed. A timeout is
    /// not: the core may still type it once a draft is released.
    pub fn not_typed(&self) -> bool {
        !matches!(self, Refusal::TimedOut)
    }

    /// The line the stage logs — hc-tui's notice wording for its two
    /// named cases, the core's own words otherwise.
    pub fn text(&self) -> String {
        match self {
            Refusal::Cancelled => CANCELLED.to_owned(),
            Refusal::TimedOut => TIMED_OUT.to_owned(),
            Refusal::Other(words) => words.clone(),
        }
    }
}

/// Classifies a failed `terminal.send`.
pub fn refusal(error: &CallError) -> Refusal {
    match error {
        error if error.is_kind(hc_proto::jsonrpc::error_kind::CANCELLED) => Refusal::Cancelled,
        CallError::Timeout => Refusal::TimedOut,
        error => Refusal::Other(error.to_string()),
    }
}

/// Whether a failed send leaves the message queued for a later tick
/// instead of failing the stage: a core refusal or a deadline. A dead
/// wire or a protocol violation still ends the tick.
pub fn retryable(error: &CallError) -> bool {
    matches!(error, CallError::Rpc(_) | CallError::Timeout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hc_proto::jsonrpc::RpcError;

    fn cancelled() -> CallError {
        CallError::Rpc(RpcError {
            code: -32000,
            message: "cancelled".into(),
            data: Some(serde_json::json!({ "kind": "cancelled" })),
        })
    }

    fn unavailable() -> CallError {
        CallError::Rpc(RpcError {
            code: -32000,
            message: "the target is unavailable".into(),
            data: Some(serde_json::json!({ "kind": "unavailable" })),
        })
    }

    /// `cancelled` is a refusal that typed nothing, in hc-tui's words.
    #[test]
    fn a_cancelled_send_is_a_refusal_that_typed_nothing() {
        let refusal = refusal(&cancelled());
        assert_eq!(refusal, Refusal::Cancelled);
        assert!(refusal.not_typed());
        assert_eq!(refusal.text(), CANCELLED);
        assert!(retryable(&cancelled()));
    }

    /// A timeout is ambiguous: the core may still type the line, so it
    /// must never read as "not typed".
    #[test]
    fn a_timed_out_send_may_still_be_typed() {
        let refusal = refusal(&CallError::Timeout);
        assert_eq!(refusal, Refusal::TimedOut);
        assert!(!refusal.not_typed());
        assert_eq!(refusal.text(), TIMED_OUT);
        assert!(retryable(&CallError::Timeout));
    }

    /// Any other core refusal keeps the core's own words and still
    /// leaves the message queued.
    #[test]
    fn another_refusal_keeps_the_cores_word() {
        let refusal = refusal(&unavailable());
        assert!(refusal.not_typed());
        assert_eq!(refusal.text(), unavailable().to_string());
        assert!(retryable(&unavailable()));
    }

    /// A broken wire is not a refusal — the tick fails instead.
    #[test]
    fn a_dead_wire_is_not_retried_as_a_refusal() {
        assert!(!retryable(&CallError::Closed));
        assert!(!retryable(&CallError::Protocol("bad frame".into())));
    }
}
