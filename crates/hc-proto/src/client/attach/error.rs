// Derived from herdr-core crates/herdr-core/src/server.rs and viewers.rs (Apache-2.0).

//! Attach error and end-of-stream types.

use super::super::{CallError, Error};
use super::output::CloseReason;
use crate::transport;

/// What `detach` reports back.
#[derive(Debug)]
pub struct Ended {
    /// The stream's close reason — `Some` when the core sent CLOSE (the
    /// detach ack itself is [`CloseReason::Empty`]); `None` when the ack
    /// deadline elapsed or the wire failed first.
    pub reason: Option<CloseReason>,
    /// The owned connection's report: an ssh child's status and stderr.
    pub transport: transport::Closed,
}

/// An attach failure: opening, the attach RPC, or a stream operation.
#[derive(Debug)]
pub enum AttachError {
    /// The transport or `hello` failed; nothing attached.
    Open(Error),
    /// The `terminal.attach` call failed — `Rpc` keeps structured kinds
    /// (`finished`, `not_found`, `needs_framed`, `self_attach`) whole.
    Call(CallError),
    /// This attach is an observer: input, resize, focus and facts are
    /// refused before they ever reach the wire.
    Observe,
    /// The stream ended; the reason is in the events already delivered.
    Closed,
    /// The peer violated the attach contract (e.g. channel 0).
    Protocol(String),
    /// `consume` acknowledged more than was received-and-unconsumed.
    OverCredit {
        /// What was asked to acknowledge.
        asked: usize,
        /// What was actually outstanding.
        outstanding: usize,
    },
}

impl std::fmt::Display for AttachError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AttachError::Open(e) => write!(f, "{e}"),
            AttachError::Call(e) => write!(f, "attach failed: {e}"),
            AttachError::Observe => write!(f, "an observer cannot send this"),
            AttachError::Closed => write!(f, "the attach stream ended"),
            AttachError::Protocol(what) => write!(f, "protocol violation: {what}"),
            AttachError::OverCredit { asked, outstanding } => {
                write!(f, "cannot credit {asked} bytes; {outstanding} outstanding")
            }
        }
    }
}

impl std::error::Error for AttachError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AttachError::Open(e) => Some(e),
            AttachError::Call(e) => Some(e),
            _ => None,
        }
    }
}

impl From<Error> for AttachError {
    fn from(e: Error) -> AttachError {
        AttachError::Open(e)
    }
}

impl From<CallError> for AttachError {
    fn from(e: CallError) -> AttachError {
        AttachError::Call(e)
    }
}

/// When the peer plainly died (`Closed`, a codec failure) its real exit
/// status and stderr tail are the better diagnosis; a real protocol
/// answer is never replaced.
pub(super) fn with_transport_diagnostics(
    err: AttachError,
    closed: Result<transport::Closed, transport::Error>,
) -> AttachError {
    let peer_died = matches!(
        err,
        AttachError::Open(Error::Closed | Error::Codec(_)) | AttachError::Call(CallError::Closed)
    );
    match (peer_died, closed) {
        (true, Err(transport)) => AttachError::Open(Error::Transport(transport)),
        _ => err,
    }
}
