// Derived from herdr-core schema/README.md and crates/herdr-core/src/rpc.rs (Apache-2.0).

//! JSON-RPC 2.0 envelopes as `hc` speaks them: one message per line in plain
//! mode, and DATA payloads on channel 0 in framed mode. Errors carry a stable
//! `data.kind` and an optional `hint`.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

/// The only JSON-RPC version accepted on the wire.
pub const JSONRPC: &str = "2.0";

/// Stable `data.kind` values seen from `hc`, and the protocol-level kinds of
/// JSON-RPC itself. Unknown kinds stay usable through [`RpcError::kind`].
pub mod error_kind {
    /// The peer does not speak the requested protocol generation.
    pub const UNSUPPORTED_GENERATION: &str = "unsupported_generation";
    /// -32700: the message was not valid JSON.
    pub const PARSE_ERROR: &str = "parse_error";
    /// -32600: the envelope was not a legal request.
    pub const INVALID_REQUEST: &str = "invalid_request";
    /// -32601: the method does not exist.
    pub const UNKNOWN_METHOD: &str = "unknown_method";
    /// -32602: the params do not fit the method.
    pub const INVALID_PARAMS: &str = "invalid_params";
    /// -32603: the method failed internally.
    pub const INTERNAL: &str = "internal";
    /// What was asked for does not exist.
    pub const NOT_FOUND: &str = "not_found";
    /// The target is lost or otherwise cannot serve the method now.
    pub const UNAVAILABLE: &str = "unavailable";
    /// A versioned write found the value changed under it.
    pub const CONFLICT: &str = "conflict";
}

/// Serializes as `"2.0"` and accepts only `"2.0"`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Version;

impl Serialize for Version {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(JSONRPC)
    }
}

impl<'de> Deserialize<'de> for Version {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        if s == JSONRPC {
            Ok(Version)
        } else {
            Err(serde::de::Error::custom(format_args!(
                "unsupported jsonrpc {s:?}"
            )))
        }
    }
}

/// A request/response correlation id: a number or a string. `0` is a real id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Id {
    /// Numeric id; every id this client issues is one.
    Num(i64),
    /// String id; a peer may answer with one it was given.
    Str(String),
}

impl From<i64> for Id {
    fn from(n: i64) -> Id {
        Id::Num(n)
    }
}

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Id::Num(n) => write!(f, "{n}"),
            Id::Str(s) => write!(f, "{s}"),
        }
    }
}

/// An outgoing call: `{"jsonrpc":"2.0","id":..,"method":..,"params":..}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// Always `"2.0"`.
    pub jsonrpc: Version,
    /// Correlates the response; `0` is not a sentinel.
    pub id: Id,
    /// The method to call, e.g. `terminal.list`.
    pub method: String,
    /// By-name params; absent when the method takes none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl Request {
    /// Builds a request with by-name params already JSON-encoded.
    pub fn new(id: Id, method: &str, params: Option<Value>) -> Request {
        Request {
            jsonrpc: Version,
            id,
            method: method.into(),
            params,
        }
    }

    /// Params from a typed params struct; `None` sends no `params` member.
    pub fn typed<T: Serialize>(
        id: Id,
        method: &str,
        params: &T,
    ) -> Result<Request, serde_json::Error> {
        Ok(Request::new(
            id,
            method,
            Some(serde_json::to_value(params)?),
        ))
    }

    /// One line for the wire: the message plus a trailing newline.
    pub fn to_line(&self) -> Vec<u8> {
        let mut line = serde_json::to_vec(self).expect("request serializes");
        line.push(b'\n');
        line
    }
}

/// A server-initiated message without an `id`, e.g. `event` notifications.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    /// Always `"2.0"`.
    pub jsonrpc: Version,
    /// The notification method: `event`, `heartbeat`, `missed`, or newer ones.
    pub method: String,
    /// Method payload.
    #[serde(default)]
    pub params: Option<Value>,
}

impl Notification {
    /// Decodes `params` into `T`; an absent payload gives `T::default()`.
    pub fn params<T: serde::de::DeserializeOwned + Default>(&self) -> Result<T, serde_json::Error> {
        match &self.params {
            Some(params) => serde_json::from_value(params.clone()),
            None => Ok(T::default()),
        }
    }
}

/// An `error` member: the JSON-RPC `code`/`message`, plus `hc`'s structured
/// `data` (`kind`, `hint` and anything else) kept whole.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    /// The JSON-RPC code; `hc` uses -32000 for its own kinds.
    pub code: i64,
    /// A human-readable summary; on `hc` it is often the kind itself.
    pub message: String,
    /// Structured details. `data.kind` and `data.hint` are stable on `hc`;
    /// foreign errors may carry another shape, kept here unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    /// Builds an error in `hc` style: kind and hint under `data`.
    pub fn new(
        code: i64,
        kind: &str,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) -> RpcError {
        RpcError {
            code,
            message: message.into(),
            data: Some(serde_json::json!({"kind": kind, "hint": hint.into()})),
        }
    }

    /// `data.kind` when present and a string.
    pub fn kind(&self) -> Option<&str> {
        self.data.as_ref()?.get("kind")?.as_str()
    }

    /// `data.hint` when present and a string.
    pub fn hint(&self) -> Option<&str> {
        self.data.as_ref()?.get("hint")?.as_str()
    }

    /// `true` when `data.kind` equals `kind`.
    pub fn is_kind(&self, kind: &str) -> bool {
        self.kind() == Some(kind)
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.kind(), self.hint()) {
            (Some(kind), Some(hint)) if !hint.is_empty() => {
                write!(f, "{kind}: {} ({hint})", self.message)
            }
            (Some(kind), _) => write!(f, "{kind}: {}", self.message),
            _ => write!(f, "{} (code {})", self.message, self.code),
        }
    }
}

impl std::error::Error for RpcError {}

/// The `result` member of a reply that succeeded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OkResponse {
    /// Always `"2.0"`.
    pub jsonrpc: Version,
    /// The request's id, echoed.
    pub id: Id,
    /// The method's result.
    pub result: Value,
}

/// The `error` member of a reply that failed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrResponse {
    /// Always `"2.0"`.
    pub jsonrpc: Version,
    /// The request's id, or null when it could not be determined.
    pub id: Option<Id>,
    /// The structured error.
    pub error: RpcError,
}

/// One half of a reply: result or error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Response {
    /// `{"jsonrpc":"2.0","id":..,"result":..}`.
    Ok(OkResponse),
    /// `{"jsonrpc":"2.0","id":..,"error":..}`.
    Err(ErrResponse),
}

impl Response {
    /// The response's correlation id, when it carries one.
    pub fn id(&self) -> Option<&Id> {
        match self {
            Response::Ok(r) => Some(&r.id),
            Response::Err(r) => r.id.as_ref(),
        }
    }

    /// The result, or the structured error.
    pub fn into_result(self) -> Result<Value, RpcError> {
        match self {
            Response::Ok(r) => Ok(r.result),
            Response::Err(r) => Err(r.error),
        }
    }
}

/// Any inbound envelope, dispatched by which members it carries.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// A call with an `id`: `hc` never sends these to a client, but the shape
    /// parses so relays and tests can see it.
    Request(Request),
    /// A reply to a call we made.
    Response(Response),
    /// A server message without an `id`.
    Notification(Notification),
}

/// Why an inbound line or DATA payload is not a usable envelope.
#[derive(Debug)]
pub enum ParseError {
    /// Not valid JSON.
    Json(serde_json::Error),
    /// `jsonrpc` was missing or not `"2.0"`; carries the found value.
    Version(Option<String>),
    /// Valid JSON that is no legal request, response or notification.
    Malformed(&'static str),
    /// The envelope shape was right but a typed field did not fit.
    Invalid(serde_json::Error),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Json(e) => write!(f, "not JSON: {e}"),
            ParseError::Version(found) => {
                write!(
                    f,
                    "unsupported jsonrpc member: {}",
                    found.as_deref().unwrap_or("missing")
                )
            }
            ParseError::Malformed(what) => write!(f, "malformed envelope: {what}"),
            ParseError::Invalid(e) => write!(f, "invalid envelope field: {e}"),
        }
    }
}

impl std::error::Error for ParseError {}

impl Message {
    /// Parses one wire message: a line's bytes without the newline, or a
    /// channel-0 DATA payload. Blank input is malformed, not empty.
    pub fn parse(bytes: &[u8]) -> Result<Message, ParseError> {
        let value: Value = serde_json::from_slice(bytes).map_err(ParseError::Json)?;
        Message::from_value(value)
    }

    /// Parses a decoded JSON value, checking the member combination before
    /// interpreting it as an envelope.
    pub fn from_value(value: Value) -> Result<Message, ParseError> {
        let Value::Object(obj) = value else {
            return Err(ParseError::Malformed("envelope is not an object"));
        };
        match obj.get("jsonrpc") {
            Some(Value::String(s)) if s == JSONRPC => {}
            other => {
                let found = other.and_then(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .or_else(|| Some(v.to_string()))
                });
                return Err(ParseError::Version(found));
            }
        }
        // None = id absent, Some(false) = id null, Some(true) = a real id.
        let id = obj.get("id").map(|v| !v.is_null());
        let (method, result, error) = (
            obj.contains_key("method"),
            obj.contains_key("result"),
            obj.contains_key("error"),
        );
        let value = Value::Object(obj);
        match (method, id, result, error) {
            // Method plus a non-null id: a call.
            (true, Some(true), false, false) => serde_json::from_value(value)
                .map(Message::Request)
                .map_err(ParseError::Invalid),
            // Method without an id: a notification.
            (true, None, false, false) => serde_json::from_value(value)
                .map(Message::Notification)
                .map_err(ParseError::Invalid),
            // No method, an id (null only with an error), exactly one payload.
            (false, Some(_), true, false) | (false, Some(_), false, true) => {
                serde_json::from_value(value)
                    .map(Message::Response)
                    .map_err(ParseError::Invalid)
            }
            _ => Err(ParseError::Malformed(
                "not a request, response or notification",
            )),
        }
    }
}
