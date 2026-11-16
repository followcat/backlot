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
mod open;

pub use error::SubscribeError;

use crate::codec::{Reader, Writer};
use crate::endpoint::Endpoint;
use crate::events::{ServerNotification, SubscribeParams, SubscribeResult};
use crate::hello::{Capabilities, HelloResult};
use crate::jsonrpc::{Id, Message, Request};
use crate::method;
use crate::transport::{self, Guard, ReadHalf, WriteHalf};
use open::{failed, hello, subscribe_call};
use std::ffi::OsStr;
use std::time::Duration;

/// Deadline for opening the transport (socket connect or ssh spawn).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Deadline for the `hello` reply.
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
/// Deadline for the `events.subscribe` reply (it includes the snapshot).
const SUBSCRIBE_TIMEOUT: Duration = Duration::from_secs(30);
/// Silence bound for [`Subscription::recv`]: the core heartbeats its
/// subscriptions every 15 s, so 45 s without any line — heartbeat or
/// event — is a dead link, not an idle one (a blackholed path delivers
/// no EOF for `read_line` to see). Reported exactly like a clean EOF.
const RECV_DEADLINE: Duration = Duration::from_secs(45);

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
    /// The read deadline each `recv` applies — [`RECV_DEADLINE`]
    /// unless a test overrode it.
    recv_deadline: Duration,
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
            recv_deadline: RECV_DEADLINE,
        })
    }

    /// Test hook: overrides [`RECV_DEADLINE`] so paused-time tests do
    /// not drive three heartbeat periods of virtual silence.
    #[doc(hidden)]
    pub fn with_recv_deadline(mut self, deadline: Duration) -> Subscription {
        self.recv_deadline = deadline;
        self
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
    /// untrusted stream. A whole [`RECV_DEADLINE`] without any line —
    /// heartbeat or event — ends the same way: the core heartbeats its
    /// subscriptions, so silence is a dead link, not an idle one.
    pub async fn recv(&mut self) -> Result<ServerNotification, SubscribeError> {
        loop {
            let Some(reader) = self.reader.as_mut() else {
                return Err(SubscribeError::Closed);
            };
            let line = match tokio::time::timeout(self.recv_deadline, reader.read_line()).await {
                Ok(Ok(Some(line))) => line,
                Ok(Ok(None)) | Err(_) => return Err(self.end(SubscribeError::Closed)),
                Ok(Err(e)) => return Err(self.end(SubscribeError::Codec(e))),
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
