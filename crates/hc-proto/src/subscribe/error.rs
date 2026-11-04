// Derived from herdr-core schema/README.md and crates/herdr-core/src/server.rs (Apache-2.0).

//! Subscription error type.

use crate::client::{CallError, Error};
use crate::{codec, transport};

/// A subscription failure: opening, the `events.subscribe` request, or a
/// notification read. `Closed`/`Codec`/`Protocol` are terminal — the stream
/// carries no second chance, so they end the subscription rather than wait.
#[derive(Debug)]
pub enum SubscribeError {
    /// The transport or `hello` failed; nothing was subscribed.
    Open(Error),
    /// The `events.subscribe` request itself failed — `Rpc` keeps the
    /// structured kinds whole.
    Call(CallError),
    /// The peer ended the stream cleanly: a subscription has nothing left
    /// to wait for.
    Closed,
    /// A line could not be read (truncated, oversized, or an I/O failure).
    Codec(codec::Error),
    /// The peer violated the subscription contract: a non-notification
    /// envelope, or a known notification whose required params did not fit.
    Protocol(String),
}

impl std::fmt::Display for SubscribeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubscribeError::Open(e) => write!(f, "{e}"),
            SubscribeError::Call(e) => write!(f, "subscribe failed: {e}"),
            SubscribeError::Closed => write!(f, "the subscription stream ended"),
            SubscribeError::Codec(e) => write!(f, "wire: {e}"),
            SubscribeError::Protocol(what) => write!(f, "protocol violation: {what}"),
        }
    }
}

impl std::error::Error for SubscribeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SubscribeError::Open(e) => Some(e),
            SubscribeError::Call(e) => Some(e),
            SubscribeError::Codec(e) => Some(e),
            _ => None,
        }
    }
}

impl From<Error> for SubscribeError {
    fn from(e: Error) -> SubscribeError {
        SubscribeError::Open(e)
    }
}

impl From<CallError> for SubscribeError {
    fn from(e: CallError) -> SubscribeError {
        SubscribeError::Call(e)
    }
}

/// When the peer plainly died (a bare `Closed`) its real exit status and
/// stderr tail are the better diagnosis; a real protocol answer — an error
/// reply, a generation mismatch — is never replaced.
pub(super) fn with_transport_diagnostics(
    err: SubscribeError,
    closed: Result<transport::Closed, transport::Error>,
) -> SubscribeError {
    let peer_died = matches!(
        err,
        SubscribeError::Open(Error::Closed | Error::Codec(_))
            | SubscribeError::Call(CallError::Closed)
    );
    match (peer_died, closed) {
        (true, Err(transport)) => SubscribeError::Open(Error::Transport(transport)),
        _ => err,
    }
}
