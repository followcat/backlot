// Derived from herdr-core schema/README.md and crates/herdr-core/src/{server,events}.rs (Apache-2.0).

//! The subscription's opening handshake: plain-mode `hello`, the
//! `events.subscribe` call, and the shared failure path that drops the
//! codec halves and the backend before reporting what the caller saw.

use super::SubscribeError;
use super::error::with_transport_diagnostics;
use crate::codec::{Reader, Writer};
use crate::events::SubscribeResult;
use crate::hello::{HelloParams, HelloResult};
use crate::jsonrpc::{Id, Message, Request, Response};
use crate::method;
use crate::transport::{Guard, ReadHalf, WriteHalf};

/// Writes `hello` in plain mode (no framed switch) and validates its plain
/// reply. The same `reader` keeps being used — early notification bytes may
/// already sit in its buffer.
pub(super) async fn hello(
    reader: &mut Reader<ReadHalf>,
    writer: &mut Writer<WriteHalf>,
) -> Result<HelloResult, crate::client::Error> {
    let request = Request::typed(Id::Num(0), method::HELLO, &HelloParams::new())
        .map_err(|e| crate::client::Error::Shape(e.to_string()))?;
    let line =
        serde_json::to_vec(&request).map_err(|e| crate::client::Error::Shape(e.to_string()))?;
    writer.write_line(&line).await?;
    let line = reader
        .read_line()
        .await?
        .ok_or(crate::client::Error::Closed)?;
    let message = Message::parse(&line)
        .map_err(|e| crate::client::Error::Shape(format!("hello reply: {e}")))?;
    let Message::Response(response) = message else {
        return Err(crate::client::Error::Shape(
            "hello reply is not a response".into(),
        ));
    };
    if response.id() != Some(&Id::Num(0)) {
        return Err(crate::client::Error::Shape(
            "hello reply id is not the request's".into(),
        ));
    }
    let result = match response {
        Response::Ok(r) => r.result,
        Response::Err(r) => return Err(crate::client::Error::Rpc(r.error)),
    };
    let hello: HelloResult = serde_json::from_value(result)
        .map_err(|e| crate::client::Error::Shape(format!("hello result: {e}")))?;
    if hello.framed {
        return Err(crate::client::Error::Shape(
            "plain-mode hello reply claims framed".into(),
        ));
    }
    Ok(hello)
}

/// Sends the `events.subscribe` request and reads its reply — the very
/// next message must be it: the contract sends the snapshot reply
/// *before* any notification, so a notification line arriving first is
/// a protocol violation, never queued (an unbounded pre-reply queue
/// would let a peer flood memory inside the time budget). Notifications
/// coalesced *after* the reply in one packet stay buffered in `reader`
/// and are served by `recv`.
pub(super) async fn subscribe_call(
    reader: &mut Reader<ReadHalf>,
    writer: &mut Writer<WriteHalf>,
    request: Vec<u8>,
) -> Result<SubscribeResult, crate::client::CallError> {
    use crate::client::CallError;
    writer
        .write_line(&request)
        .await
        .map_err(|_| CallError::Closed)?;
    let line = match reader.read_line().await {
        Ok(Some(line)) => line,
        Ok(None) => return Err(CallError::Closed),
        Err(e) => {
            return Err(match e {
                crate::codec::Error::Format(_) => CallError::Protocol(e.to_string()),
                _ => CallError::Closed,
            });
        }
    };
    let message =
        Message::parse(&line).map_err(|e| CallError::Protocol(format!("subscribe reply: {e}")))?;
    match message {
        Message::Response(response) if response.id() == Some(&Id::Num(1)) => {
            let result = response.into_result().map_err(CallError::Rpc)?;
            serde_json::from_value(result).map_err(CallError::Shape)
        }
        Message::Notification(_) => Err(CallError::Protocol(
            "notification before the subscribe reply".into(),
        )),
        // A reply to a different id, or a request: never valid here.
        _ => Err(CallError::Protocol(
            "unexpected message before the subscribe reply".into(),
        )),
    }
}

/// A failure while opening: the codec halves drop first so the child sees
/// stdin EOF promptly, then the backend's own status and stderr tail
/// replace a bare `Closed` — never a real protocol answer. The close is
/// the prompt EOF one: a child that ignores stdin EOF is killed inside
/// the grace instead of blocking the error for the full close timeout.
pub(super) async fn failed(
    err: SubscribeError,
    reader: Reader<ReadHalf>,
    writer: Writer<WriteHalf>,
    guard: Option<Guard>,
) -> SubscribeError {
    drop(reader);
    drop(writer);
    let Some(guard) = guard else { return err };
    with_transport_diagnostics(err, guard.close_eof().await)
}
