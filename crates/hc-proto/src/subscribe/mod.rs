// Derived from herdr-core schema/README.md and crates/herdr-core/src/{server,events}.rs (Apache-2.0).

//! A dedicated plain-mode `events.subscribe` connection.
//!
//! The subscription owns its own socket or ssh child — separate from the
//! framed RPC client — so a stalled event stream never blocks calls and a
//! dropped subscription never strands a transport. `hello` negotiates
//! generation 1 *without* asking for framed mode: the reply, the subscribe
//! result and every later notification arrive one JSON message per line,
//! all through the same buffered reader — notifications coalesced with the
//! reply in one packet are preserved, never dropped.
//!
//! One owner reads with [`Subscription::recv`]; there is no background
//! queue and no polling timer. After the subscribe reply the wire carries
//! notifications only: unknown methods are ignored, unknown event kinds are
//! retained as their `type` string, and anything malformed — a bad line, a
//! non-notification envelope, missing required params — ends the stream
//! with a terminal error instead of an endless silent wait. Nothing is
//! replayed: a reconnect is a fresh `Subscription` and a fresh sequence
//! epoch handled by the caller.

mod error;

pub use error::SubscribeError;

use crate::codec::{Reader, Writer};
use crate::endpoint::Endpoint;
use crate::events::{ServerNotification, SubscribeParams, SubscribeResult};
use crate::hello::{Capabilities, HelloParams, HelloResult};
use crate::jsonrpc::{Id, Message, Request, Response};
use crate::method;
use crate::transport::{self, Guard, ReadHalf, WriteHalf};
use error::with_transport_diagnostics;
use std::ffi::OsStr;
use std::time::Duration;

/// Deadline for opening the transport (socket connect or ssh spawn).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Deadline for the `hello` reply.
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
/// Deadline for the `events.subscribe` reply (it includes the snapshot).
const SUBSCRIBE_TIMEOUT: Duration = Duration::from_secs(30);

/// An open `events.subscribe` stream: the initial snapshot plus a
/// single-owner notification reader. A terminal read error latches the
/// end — both halves and the owned backend drop at once — and dropping
/// the subscription itself always owns the same cleanup (for ssh, the
/// child is killed and reaped).
pub struct Subscription {
    /// The validated `hello` result (generation 1, plain mode).
    hello: HelloResult,
    /// The subscribe reply: starting `seq` and the snapshot terminals.
    snapshot: SubscribeResult,
    /// What the subscription was asked to filter by.
    params: SubscribeParams,
    /// Buffered reader; its read-ahead bytes are never dropped. `None`
    /// after a terminal error — the latch making later reads fail fast.
    reader: Option<Reader<ReadHalf>>,
    /// Kept so `close` can end the peer's stdin before finishing the child.
    writer: Option<Writer<WriteHalf>>,
    /// The owned backend; its `Drop` kills and reaps an ssh child.
    guard: Option<Guard>,
}

impl Subscription {
    /// Opens `endpoint`, negotiates the plain wire and issues
    /// `events.subscribe`. `ssh_program` overrides the `ssh` name for
    /// tests; production passes `None`.
    pub async fn connect(
        endpoint: &Endpoint,
        ssh_program: Option<&OsStr>,
        params: &SubscribeParams,
    ) -> Result<Subscription, SubscribeError> {
        let conn = tokio::time::timeout(CONNECT_TIMEOUT, transport::connect(endpoint, ssh_program))
            .await
            .map_err(|_| SubscribeError::Open(crate::client::Error::Timeout("connect")))?
            .map_err(crate::client::Error::Transport)?;
        let (reader, writer, guard) = conn.split();
        Subscription::negotiated(reader, writer, Some(guard), params).await
    }

    /// Subscribes over an injected byte stream — tests use in-memory wires.
    /// `guard` owns the backend's lifetime when there is one.
    pub async fn open(
        read: ReadHalf,
        write: WriteHalf,
        guard: Option<Guard>,
        params: &SubscribeParams,
    ) -> Result<Subscription, SubscribeError> {
        Subscription::negotiated(Reader::new(read), Writer::new(write), guard, params).await
    }

    async fn negotiated(
        mut reader: Reader<ReadHalf>,
        mut writer: Writer<WriteHalf>,
        guard: Option<Guard>,
        params: &SubscribeParams,
    ) -> Result<Subscription, SubscribeError> {
        let hello = match tokio::time::timeout(HELLO_TIMEOUT, hello(&mut reader, &mut writer)).await
        {
            Ok(Ok(hello)) => hello,
            Ok(Err(e)) => {
                return Err(failed(SubscribeError::Open(e), reader, writer, guard).await);
            }
            Err(_) => {
                return Err(failed(
                    SubscribeError::Open(crate::client::Error::Timeout("hello")),
                    reader,
                    writer,
                    guard,
                )
                .await);
            }
        };
        let caps = match Capabilities::new(&hello) {
            Ok(caps) => caps,
            Err(e) => {
                return Err(failed(
                    SubscribeError::Open(crate::client::Error::Generation(e)),
                    reader,
                    writer,
                    guard,
                )
                .await);
            }
        };
        if !caps.has_method(method::EVENTS_SUBSCRIBE) {
            return Err(failed(
                crate::client::CallError::Unsupported {
                    method: method::EVENTS_SUBSCRIBE.into(),
                    reason: format!("core {} does not offer it", hello.core.name),
                }
                .into(),
                reader,
                writer,
                guard,
            )
            .await);
        }
        let request = Request::typed(Id::Num(1), method::EVENTS_SUBSCRIBE, params)
            .and_then(|request| serde_json::to_vec(&request));
        let request = match request {
            Ok(request) => request,
            Err(e) => {
                return Err(failed(
                    crate::client::CallError::Shape(e).into(),
                    reader,
                    writer,
                    guard,
                )
                .await);
            }
        };
        let call = subscribe_call(&mut reader, &mut writer, request);
        let result = match tokio::time::timeout(SUBSCRIBE_TIMEOUT, call).await {
            Ok(Ok(result)) => result,
            Ok(Err(e)) => return Err(failed(e.into(), reader, writer, guard).await),
            Err(_) => {
                return Err(failed(
                    crate::client::CallError::Timeout.into(),
                    reader,
                    writer,
                    guard,
                )
                .await);
            }
        };
        if result.channel.is_some() {
            return Err(failed(
                crate::client::CallError::Protocol(
                    "plain-mode subscribe returned an event channel".into(),
                )
                .into(),
                reader,
                writer,
                guard,
            )
            .await);
        }
        Ok(Subscription {
            hello,
            snapshot: result,
            params: params.clone(),
            reader: Some(reader),
            writer: Some(writer),
            guard,
        })
    }

    /// The validated handshake: generation, identity, methods, features.
    pub fn hello(&self) -> &HelloResult {
        &self.hello
    }

    /// The subscribe reply: the starting `seq` and the terminal snapshot.
    /// Snapshots contain terminals only — documents are always refetched.
    pub fn snapshot(&self) -> &SubscribeResult {
        &self.snapshot
    }

    /// Whether the subscribe params filtered the stream. A filtered
    /// subscription need not receive every `seq` — gaps are expected, not
    /// a resync signal (see [`crate::inventory::Inventory`]).
    pub fn filtered(&self) -> bool {
        self.params.terminal.is_some()
            || self.params.children_of.is_some()
            || !self.params.labels.is_empty()
    }

    /// An inventory seeded from this subscription's snapshot and filter.
    pub fn inventory(&self) -> crate::inventory::Inventory {
        let mut inventory = crate::inventory::Inventory::for_subscription(&self.params);
        inventory.replace(self.snapshot.seq, self.snapshot.terminals.clone());
        inventory
    }

    /// Reads the next notification. Unknown methods are ignored; a clean
    /// EOF is [`SubscribeError::Closed`], a broken line
    /// [`SubscribeError::Codec`], and a contract violation
    /// [`SubscribeError::Protocol`] — all *terminal*: the error latches
    /// the end, both halves and the owned backend drop at once (the peer
    /// sees EOF, an ssh child is killed and reaped), and every later
    /// `recv` fails promptly as `Closed` instead of resuming on an
    /// untrusted stream.
    pub async fn recv(&mut self) -> Result<ServerNotification, SubscribeError> {
        loop {
            let Some(reader) = self.reader.as_mut() else {
                return Err(SubscribeError::Closed);
            };
            let line = match reader.read_line().await {
                Ok(Some(line)) => line,
                Ok(None) => return Err(self.end(SubscribeError::Closed)),
                Err(e) => return Err(self.end(SubscribeError::Codec(e))),
            };
            let message = match Message::parse(&line) {
                Ok(message) => message,
                Err(e) => {
                    return Err(self.end(SubscribeError::Protocol(format!("notification: {e}"))));
                }
            };
            let Message::Notification(notification) = message else {
                return Err(self.end(SubscribeError::Protocol(
                    "subscription wire carried a request or response".into(),
                )));
            };
            match ServerNotification::from_notification(&notification) {
                Ok(Some(notification)) => return Ok(notification),
                // A method this generation does not define: ignored.
                Ok(None) => continue,
                Err(e) => {
                    return Err(self.end(SubscribeError::Protocol(format!(
                        "malformed {} params: {e}",
                        notification.method
                    ))));
                }
            }
        }
    }

    /// A terminal failure: drop both codec halves and the owned backend
    /// so a failed subscription can never resume on the untrusted stream,
    /// then hand the caller the error it actually saw.
    fn end(&mut self, err: SubscribeError) -> SubscribeError {
        self.reader = None;
        self.writer = None;
        self.guard = None;
        err
    }

    /// Orderly shutdown: the read half drops first — a remote child
    /// blocked writing a full stdout pipe then dies on EPIPE instead of
    /// holding the close until the kill deadline — the write half ends
    /// the peer's stdin, and the backend is waited for or killed with
    /// its status and stderr tail reported. A subscription already ended
    /// by a terminal `recv` error reports default `Closed`: its halves
    /// and backend were dropped with the error.
    pub async fn close(mut self) -> Result<transport::Closed, transport::Error> {
        drop(self.reader.take());
        drop(self.writer.take());
        match self.guard.take() {
            Some(guard) => guard.close().await,
            None => Ok(transport::Closed::default()),
        }
    }
}

impl std::fmt::Debug for Subscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subscription")
            .field("core", &self.hello.core.name)
            .field("seq", &self.snapshot.seq)
            .field("filtered", &self.filtered())
            .field("ended", &self.reader.is_none())
            .finish_non_exhaustive()
    }
}

/// Writes `hello` in plain mode (no framed switch) and validates its plain
/// reply. The same `reader` keeps being used — early notification bytes may
/// already sit in its buffer.
async fn hello<R, W>(
    reader: &mut Reader<R>,
    writer: &mut Writer<W>,
) -> Result<HelloResult, crate::client::Error>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
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
async fn subscribe_call<R, W>(
    reader: &mut Reader<R>,
    writer: &mut Writer<W>,
    request: Vec<u8>,
) -> Result<SubscribeResult, crate::client::CallError>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
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
/// replace a bare `Closed` — never a real protocol answer.
async fn failed(
    err: SubscribeError,
    reader: Reader<ReadHalf>,
    writer: Writer<WriteHalf>,
    guard: Option<Guard>,
) -> SubscribeError {
    drop(reader);
    drop(writer);
    let Some(guard) = guard else { return err };
    with_transport_diagnostics(err, guard.close().await)
}
