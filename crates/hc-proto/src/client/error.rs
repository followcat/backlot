// Derived from herdr-core schema/README.md and crates/herdr-core/src/server.rs (Apache-2.0).

//! Client error types: [`Error`] for opening and negotiating a connection,
//! [`CallError`] for individual RPC calls.

use crate::{codec, hello, jsonrpc::RpcError, transport};
use std::fmt;

/// Opening the transport or negotiating `hello` failed. No calls were made.
#[derive(Debug)]
pub enum Error {
    /// The endpoint could not be reached (socket, ssh child).
    Transport(transport::Error),
    /// A deadline elapsed: `connect` or `hello`.
    Timeout(&'static str),
    /// The hello line could not be written or its reply read.
    Codec(codec::Error),
    /// The peer ended the stream before or inside the hello reply.
    Closed,
    /// The reply's generation is not one this client speaks.
    Generation(hello::UnsupportedGeneration),
    /// The core answered `framed: false`: concurrent RPC needs framed mode.
    FramedRefused,
    /// `hello` itself answered with a JSON-RPC error (for example an
    /// `unsupported_generation` kind). The structured error is kept whole.
    Rpc(RpcError),
    /// The hello reply was not a response, or its result did not fit.
    Shape(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Transport(e) => write!(f, "transport: {e}"),
            Error::Timeout(what) => write!(f, "{what} timed out"),
            Error::Codec(e) => write!(f, "wire: {e}"),
            Error::Closed => write!(f, "peer closed during handshake"),
            Error::Generation(e) => write!(f, "{e}"),
            Error::FramedRefused => {
                write!(f, "core refused framed mode; concurrent RPC unavailable")
            }
            Error::Rpc(e) => write!(f, "hello failed: {e}"),
            Error::Shape(what) => write!(f, "hello reply malformed: {what}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Transport(e) => Some(e),
            Error::Codec(e) => Some(e),
            Error::Generation(e) => Some(e),
            Error::Rpc(e) => Some(e),
            _ => None,
        }
    }
}

impl From<transport::Error> for Error {
    fn from(e: transport::Error) -> Error {
        Error::Transport(e)
    }
}

impl From<codec::Error> for Error {
    fn from(e: codec::Error) -> Error {
        Error::Codec(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::Codec(codec::Error::Io(e))
    }
}

/// One RPC call failed. `Rpc` keeps the core's structured `code`/`message`/
/// `data` whole; the rest are client-side conditions.
#[derive(Debug)]
pub enum CallError {
    /// The core answered with a structured error, kept unchanged.
    Rpc(RpcError),
    /// No reply inside the call's deadline; the pending entry is removed and
    /// a later reply is ignored.
    Timeout,
    /// The connection ended before the reply arrived (EOF, close, last
    /// client handle dropped).
    Closed,
    /// The core's `hello`/`rpc.discover` method lists do not offer this
    /// method. Only this call's feature is disabled; the reason explains why.
    Unsupported {
        /// The method that is not offered.
        method: String,
        /// Why it is unavailable, for showing to the user.
        reason: String,
    },
    /// The serialized request would exceed [`crate::frame::MAX_PAYLOAD`];
    /// checked before encoding so no frame can panic.
    TooLarge {
        /// The request size in bytes.
        size: usize,
    },
    /// The in-flight bound or outgoing queue bound is reached; retry later.
    Full,
    /// The core sent bytes that are not a valid JSON-RPC message — a wire
    /// contract violation that ends the connection.
    Protocol(String),
    /// Params would not serialize or the result did not fit the typed shape.
    Shape(serde_json::Error),
}

impl CallError {
    /// `true` when the core answered a `not_found` error.
    pub fn is_not_found(&self) -> bool {
        matches!(self, CallError::Rpc(e) if e.is_kind(crate::jsonrpc::error_kind::NOT_FOUND))
    }
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CallError::Rpc(e) => write!(f, "{e}"),
            CallError::Timeout => write!(f, "call timed out"),
            CallError::Closed => write!(f, "connection closed"),
            CallError::Unsupported { method, reason } => {
                write!(f, "`{method}` unsupported: {reason}")
            }
            CallError::TooLarge { size } => write!(f, "request of {size} bytes too large"),
            CallError::Full => write!(f, "too many calls in flight"),
            CallError::Protocol(what) => write!(f, "protocol violation: {what}"),
            CallError::Shape(e) => write!(f, "result shape: {e}"),
        }
    }
}

impl std::error::Error for CallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CallError::Rpc(e) => Some(e),
            CallError::Shape(e) => Some(e),
            _ => None,
        }
    }
}
