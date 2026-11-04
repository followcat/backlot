// Derived from herdr-core schema/README.md and crates/herdr-core/src/server.rs (Apache-2.0).

//! Negotiating a connection: a plain-mode `hello` asking for generation 1 and
//! framed mode, whose plain reply is validated before the same buffered
//! reader switches to binary frames. Read-ahead bytes are never dropped.

use crate::codec::{Reader, Writer};
use crate::hello::{Capabilities, HelloParams, HelloResult};
use crate::jsonrpc::{Id, Message, Request, Response};
use crate::method;
use tokio::io::{AsyncRead, AsyncWrite};

/// Writes `hello` in plain mode and validates its plain reply. On success the
/// connection is framed: the same `reader` — possibly already holding the
/// first frame's bytes — must keep being used, never recreated.
pub(super) async fn negotiate<R, W>(
    reader: &mut Reader<R>,
    writer: &mut Writer<W>,
) -> Result<HelloResult, super::Error>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let request = Request::typed(Id::Num(0), method::HELLO, &HelloParams::framed())
        .map_err(|e| super::Error::Shape(e.to_string()))?;
    let line = serde_json::to_vec(&request).map_err(|e| super::Error::Shape(e.to_string()))?;
    writer.write_line(&line).await?;
    let line = reader.read_line().await?.ok_or(super::Error::Closed)?;
    let message =
        Message::parse(&line).map_err(|e| super::Error::Shape(format!("hello reply: {e}")))?;
    let Message::Response(response) = message else {
        return Err(super::Error::Shape("hello reply is not a response".into()));
    };
    if response.id() != Some(&Id::Num(0)) {
        return Err(super::Error::Shape(
            "hello reply id is not the request's".into(),
        ));
    }
    let result = match response {
        Response::Ok(r) => r.result,
        Response::Err(r) => return Err(super::Error::Rpc(r.error)),
    };
    let hello: HelloResult = serde_json::from_value(result)
        .map_err(|e| super::Error::Shape(format!("hello result: {e}")))?;
    Capabilities::new(&hello).map_err(super::Error::Generation)?;
    if !hello.framed {
        return Err(super::Error::FramedRefused);
    }
    Ok(hello)
}
