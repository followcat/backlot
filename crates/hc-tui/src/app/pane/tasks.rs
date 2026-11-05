//! The pane's owned tasks: one attach actor per generation (open,
//! announce, forward the wire in order) and one writer task per live
//! attach (ordered sends, bounded progress/failure reports so parked
//! work retries when capacity frees).

use std::sync::Arc;

use hc_proto::client::{Attach, AttachError};
use hc_proto::terminal::AttachParams;
use tokio::sync::mpsc;

use super::{OpenAttach, PaneEv};

/// Work for the writer task, in send order.
#[derive(Clone)]
pub(crate) enum Out {
    /// Human input bytes (encoded keys/paste).
    Input(Vec<u8>),
    /// VT-generated replies (AUTOMATIC_INPUT when negotiated).
    Reply(Vec<u8>),
    /// Active-viewer grid change.
    Resize(u16, u16),
    /// Outer-terminal focus.
    Focus(bool),
    /// Host defaults/cell pitch, never human input or ownership activity.
    Facts(hc_proto::terminal::Display),
}

/// Drains `Out` commands to the attach in order. Parked `send_input`
/// only stalls input — the reader and the app keep moving; a closed
/// stream ends the queue and the task. Every completed send reports
/// progress so parked pending input and coalesced controls can retry;
/// a failure surfaces once, then the task ends.
pub(crate) async fn writer_task(
    attach: Arc<Attach>,
    mut rx: mpsc::Receiver<Out>,
    ev: mpsc::Sender<PaneEv>,
    generation: u64,
) -> Result<(), String> {
    while let Some(out) = rx.recv().await {
        // Controls use the protocol's priority queue. Cross the DATA write
        // fence so an earlier accepted key cannot arrive after its focus or
        // resize event, even while the transport is backpressured.
        let fence = if matches!(out, Out::Resize(..) | Out::Focus(..) | Out::Facts(..)) {
            attach.drain_data().await
        } else {
            Ok(())
        };
        let result = match (fence, out) {
            (Err(error), _) => Err(error),
            (Ok(()), Out::Input(bytes)) => attach.send_input(&bytes).await,
            (Ok(()), Out::Reply(bytes)) => attach.reply(&bytes).await,
            (Ok(()), Out::Resize(c, r)) => attach.resize(c, r).await,
            (Ok(()), Out::Focus(f)) => attach.focus(f).await,
            (Ok(()), Out::Facts(facts)) => attach.facts(&facts).await,
        };
        match result {
            // Progress MUST be reliably delivered: a dropped wake is
            // the last one, and parked input/coalesced controls would
            // never retry. `send().await` waits for queue capacity —
            // bounded by the channel, cancellation-safe via task abort.
            Ok(()) => {
                // Closing the UI receiver on exit releases backpressure here;
                // accepted input still drains before the write fence and CLOSE.
                let _ = ev.send(PaneEv::WriterProgress { generation }).await;
            }
            Err(e) => {
                let _ = ev
                    .send(PaneEv::WriterFailed {
                        generation,
                        why: e.to_string(),
                    })
                    .await;
                return Err(e.to_string());
            }
        }
    }
    Ok(())
}

/// One attach attempt: open, announce, then forward the stream until it
/// ends. Owns the wire; the app only sees `PaneEv`s.
pub async fn attach_task(
    open: OpenAttach,
    params: AttachParams,
    generation: u64,
    ev: mpsc::Sender<PaneEv>,
) {
    let attach = match open(params).await {
        Ok(attach) => Arc::new(attach),
        Err(e) => {
            // Keep `Unsupported` typed through the channel: a missing
            // `terminal.attach` disables the feature instead of arming
            // an endless retry.
            let unsupported = matches!(
                e,
                AttachError::Call(hc_proto::client::CallError::Unsupported { .. })
            );
            let _ = ev
                .send(PaneEv::Failed {
                    generation,
                    why: e.to_string(),
                    unsupported,
                })
                .await;
            return;
        }
    };
    if ev
        .send(PaneEv::Opened {
            generation,
            attach: attach.clone(),
        })
        .await
        .is_err()
    {
        return;
    }
    while let Some(out) = attach.recv().await {
        if ev.send(PaneEv::Out { generation, out }).await.is_err() {
            return;
        }
    }
    let _ = ev.send(PaneEv::Ended { generation }).await;
}
