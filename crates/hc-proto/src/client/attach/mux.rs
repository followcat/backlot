// Derived from herdr-core crates/herdr-core/src/server.rs and wire.rs (Apache-2.0).

//! The shared connection's background halves.
//!
//! The reader demultiplexes every inbound frame by channel: channel 0
//! keeps JSON-RPC replies (attach calls through the attach-call table,
//! anything else through the pending table), and every stream frame
//! routes through the channel table. The reader never waits on a
//! consumer, so a stalled `recv` on one channel cannot delay any other
//! channel or a channel-0 reply.
//!
//! The writer owns every complete frame write — control frames and owed
//! credit ahead of stream input, as on a dedicated connection — and
//! serves input fairly across channels: each live channel yields a
//! bounded batch per round, so one backed-up channel cannot starve
//! another channel's input or CLOSE.

use super::pump::CTRL_QUEUE;
use super::table::{AttachCalls, Channels, Route};
use crate::client::call;
use crate::client::error::CallError;
use crate::client::lock;
use crate::client::pump::{self, Pending};
use crate::codec::{Reader, Writer};
use crate::frame::{CONTROL_CHANNEL, Frame, Kind};
use crate::jsonrpc::{Message, Response};
use crate::transport::{ReadHalf, WriteHalf};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tokio::sync::{Notify, mpsc};

/// Stream-input frames one channel yields per writer round — the same
/// per-round budget a dedicated writer gave its single queue.
const DATA_PER_CHANNEL: usize = 16;

/// The field-level cells the reader task needs — never the whole
/// connection, so a dropped handle never keeps the task alive.
pub(super) struct Cells {
    pub(super) wire: Arc<call::Wire>,
    pub(super) channels: Arc<Mutex<Channels>>,
    pub(super) attach_calls: Arc<AttachCalls>,
}

/// What `dispatch` decided for one inbound channel-0 frame.
enum Dispatch {
    /// Handled (or ignorable).
    Done,
    /// A late attach reply whose caller is gone — CLOSE this channel.
    CloseChannel(u32),
}

/// The channel a `terminal.attach` reply named — lenient, for the
/// cancelled-open CLOSE: even a reply whose `terminal` record fails
/// shape still owes its channel a CLOSE.
fn reply_channel(value: &Value) -> Option<u32> {
    u32::try_from(value.get("channel")?.as_u64()?)
        .ok()
        .filter(|channel| *channel != 0)
}

/// One channel-0 frame: attach replies go to their call — a dead
/// receiver means the open was cancelled, so the reply's channel gets a
/// CLOSE — and everything else is a plain pending-table reply. `Err` is
/// a connection-level protocol violation, exactly as on an RPC wire.
fn dispatch(
    frame: Frame,
    pending: &Pending,
    attach_calls: &AttachCalls,
) -> Result<Dispatch, String> {
    if frame.kind != Kind::Data {
        return Ok(Dispatch::Done);
    }
    let message = Message::parse(&frame.payload)
        .map_err(|e| format!("malformed message on the control channel: {e}"))?;
    let Message::Response(response) = message else {
        return Ok(Dispatch::Done);
    };
    let Some(id) = response.id().cloned() else {
        return Err("response with a null id on the control channel".into());
    };
    if !attach_calls.contains(&id) {
        // Not an attach reply: the shared pending-table path handles it.
        pump::dispatch(frame, pending)?;
        return Ok(Dispatch::Done);
    }
    let sender = attach_calls.take(&id).expect("id checked present");
    let result = match response {
        Response::Ok(r) => Ok(r.result),
        Response::Err(r) => Err(CallError::Rpc(r.error)),
    };
    Ok(match sender.send(result) {
        Ok(()) => Dispatch::Done,
        Err(Ok(value)) => match reply_channel(&value) {
            Some(channel) => Dispatch::CloseChannel(channel),
            None => Dispatch::Done,
        },
        Err(Err(_)) => Dispatch::Done,
    })
}

/// Queues one channel CLOSE without awaiting — the synchronous half of
/// `Attach::drop`'s contract on a shared connection, where a silent drop
/// would leak the viewer. The priority queue first; a full queue hands
/// the frame to a detached task instead of dropping the core's only
/// close notification for this viewer.
pub(super) fn queue_close(wire: &Arc<call::Wire>, channel: u32) {
    let frame = Frame::close(channel, Vec::new());
    match wire.tx.try_send(frame) {
        Ok(()) | Err(mpsc::error::TrySendError::Closed(_)) => {}
        Err(mpsc::error::TrySendError::Full(frame)) => {
            let tx = wire.tx.clone();
            if let Ok(rt) = tokio::runtime::Handle::try_current() {
                rt.spawn(async move {
                    let _ = tx.send(frame).await;
                });
            }
        }
    }
}

/// Reads frames until the peer ends or violates the protocol. Channel 0
/// goes through `dispatch`; stream frames through the channel table.
/// Ends with `on_end(reason)` — the connection's shared shutdown.
pub(super) async fn read_loop(
    mut reader: Reader<ReadHalf>,
    cells: Cells,
    on_end: impl FnOnce(Option<String>),
) {
    let mut reason = None;
    while let Ok(Some(frame)) = reader.read_frame().await {
        if frame.channel == CONTROL_CHANNEL {
            match dispatch(frame, &cells.wire.pending, &cells.attach_calls) {
                Ok(Dispatch::Done) => {}
                Ok(Dispatch::CloseChannel(channel)) => {
                    lock(&cells.channels).end(channel);
                    queue_close(&cells.wire, channel);
                }
                Err(why) => {
                    reason = Some(why);
                    break;
                }
            }
        } else {
            match lock(&cells.channels).route(frame) {
                Route::Ok => {}
                Route::CloseChannel(channel) => queue_close(&cells.wire, channel),
                Route::Fatal(why) => {
                    reason = Some(why);
                    break;
                }
            }
        }
    }
    on_end(reason);
}

/// Writes queued frames until the queues close or the wire fails. Each
/// round drains a bounded batch of control frames, then every live
/// channel's owed credit, then a bounded input batch per channel —
/// control stays first, but a flooding channel starves neither another
/// channel's input nor its CLOSE.
pub(super) async fn write_loop(
    mut writer: Writer<WriteHalf>,
    mut ctrl: mpsc::Receiver<Frame>,
    channels: Arc<Mutex<Channels>>,
    work: Arc<Notify>,
    on_end: impl FnOnce(),
) {
    let mut ctrl_open = true;
    'outer: loop {
        let mut worked = false;
        for _ in 0..CTRL_QUEUE {
            if !ctrl_open {
                break;
            }
            match ctrl.try_recv() {
                Ok(frame) => {
                    worked = true;
                    if writer.write_frame(&frame).await.is_err() {
                        break 'outer;
                    }
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    ctrl_open = false;
                    break;
                }
            }
        }
        // A snapshot, never the lock across a write: a blocked transport
        // must not wedge the reader's routing.
        let live = { lock(&channels).live() };
        for (stream, data) in live {
            while let Some(frame) = stream.take_owed() {
                worked = true;
                if writer.write_frame(&frame).await.is_err() {
                    break 'outer;
                }
            }
            for _ in 0..DATA_PER_CHANNEL {
                let Some(frame) = data.pop() else { break };
                worked = true;
                if writer.write_frame(&frame).await.is_err() {
                    break 'outer;
                }
                data.mark_written();
            }
        }
        if worked {
            continue;
        }
        if !ctrl_open && lock(&channels).drained() {
            break; // producers all gone and drained: the wire is done
        }
        tokio::select! {
            frame = ctrl.recv(), if ctrl_open => {
                match frame {
                    Some(frame) => {
                        if writer.write_frame(&frame).await.is_err() { break 'outer; }
                    }
                    None => ctrl_open = false,
                }
            }
            _ = work.notified() => {}
        }
    }
    // Exiting for any reason means the transport writes stop: release
    // every channel's drain fence instead of leaving waiters parked.
    let datas = { lock(&channels).datas() };
    for data in datas {
        data.mark_dead();
    }
    drop(writer);
    on_end();
}
