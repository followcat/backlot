//! `forward_stream` credit pacing over real `Attach` duplex wires:
//! off-tab DATA coalesces onto the shared tick, unpacing flushes
//! immediately, and RESIZE/CLOSE keep exact wire order around the
//! buffered bytes.

use std::sync::Arc;
use std::time::Duration;

use hc_proto::client::Attach;
use hc_proto::frame::{Frame, Kind};
use hc_proto::terminal::AttachParams;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;

use super::*;
use crate::testcore;

const CH: u32 = 9;
const T: Duration = testcore::DEADLINE;

fn params() -> AttachParams {
    AttachParams {
        terminal: "t7".into(),
        observe: true,
        resize_notifications: true,
        quiet_human_release: false,
        history: 0,
        cols: 80,
        rows: 24,
        display: None,
        resumed: false,
        caller: None,
    }
}

struct Rig {
    core: testcore::FakeCore,
    rx: mpsc::Receiver<PaneEv>,
    paced: watch::Sender<bool>,
    attach: Arc<Attach>,
    task: tokio::task::JoinHandle<()>,
    epoch: tokio::time::Instant,
}

async fn rig(paced: bool) -> Rig {
    let (mut core, read, write) = testcore::pair(64 * 1024);
    let params = params();
    let server = tokio::spawn(async move {
        core.serve_attach(testcore::attach_result(
            CH,
            testcore::minimal_terminal(),
            true,
        ))
        .await;
        core
    });
    let attach = Arc::new(
        Attach::open(read, write, None, &params)
            .await
            .expect("attach open"),
    );
    let (ev_tx, rx) = mpsc::channel(64);
    let (paced_tx, paced_rx) = watch::channel(paced);
    let epoch = tokio::time::Instant::now();
    let task_attach = attach.clone();
    let task = tokio::spawn(async move {
        forward_stream(&task_attach, Role::Observe, 7, &ev_tx, paced_rx, epoch).await;
    });
    let core = server.await.expect("fake core serve_attach");
    Rig {
        core,
        rx,
        paced: paced_tx,
        attach,
        task,
        epoch,
    }
}

/// The next `PaneEv::Out` DATA payload, or panic.
async fn data(rig: &mut Rig, within: Duration) -> Vec<u8> {
    match timeout(within, rig.rx.recv()).await {
        Ok(Some(PaneEv::Out {
            out: Output::Data(bytes),
            ..
        })) => bytes,
        Ok(Some(PaneEv::Out { out, .. })) => panic!("wanted Data, got {out:?}"),
        _ => panic!("wanted Data, stream ended or stalled"),
    }
}

/// Unpaced observers forward chunks without waiting on any tick: a
/// merged-or-split DATA event must arrive well inside one window.
#[tokio::test(start_paused = true)]
async fn unpaced_observe_forwards_data_immediately() {
    let mut rig = rig(false).await;
    rig.core.send_stream(CH, Kind::Data, b"alpha").await;
    rig.core.send_stream(CH, Kind::Data, b"beta").await;
    let mut got = Vec::new();
    while got != b"alphabeta" {
        got.extend_from_slice(&data(&mut rig, T).await);
    }
    assert_eq!(got, b"alphabeta");
    rig.task.abort();
}

/// A paced burst lands inside one grid window and flushes as ONE
/// merged `PaneEv::Out` — credit stays withheld in `unconsumed` until
/// the consumer's `attach.consume`.
#[tokio::test(start_paused = true)]
async fn paced_observe_coalesces_a_burst_on_the_shared_tick() {
    let mut rig = rig(true).await;
    // Land the whole burst just after a grid edge so the next flush
    // is ~500ms out and every chunk merges into one event.
    let edge = next_off_tab_tick(rig.epoch, tokio::time::Instant::now());
    tokio::time::sleep_until(edge + Duration::from_millis(10)).await;
    let mut want = Vec::new();
    for i in 0..20u8 {
        let chunk = format!("chunk-{i:02}|");
        want.extend_from_slice(chunk.as_bytes());
        rig.core.send_stream(CH, Kind::Data, chunk.as_bytes()).await;
    }
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(
        rig.rx.try_recv().is_err(),
        "paced data crossed pane_rx before the tick"
    );
    let got = data(&mut rig, Duration::from_secs(2)).await;
    assert_eq!(got, want);
    assert!(
        rig.rx.try_recv().is_err(),
        "one paced burst produced multiple flushes"
    );
    assert_eq!(
        rig.attach.unconsumed(),
        want.len(),
        "paced credit leaked before the consumer's consume"
    );
    rig.attach.consume(want.len()).expect("consume");
    assert_eq!(rig.attach.unconsumed(), 0);
    rig.task.abort();
}

/// Flipping `paced` true→false mid-window flushes the buffer
/// immediately — a pane coming back on-screen never waits out the
/// 500ms tick for already-buffered bytes.
#[tokio::test(start_paused = true)]
async fn unpacing_flushes_buffered_data_immediately() {
    let mut rig = rig(true).await;
    // This chunk's flush marks a grid edge just passed: the next tick
    // is ~500ms out, so `held` must stay buffered until the flip.
    rig.core.send_stream(CH, Kind::Data, b"first").await;
    let got = data(&mut rig, Duration::from_secs(2)).await;
    assert_eq!(got, b"first");
    rig.attach.consume(got.len()).expect("consume");
    rig.core.send_stream(CH, Kind::Data, b"held").await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        rig.rx.try_recv().is_err(),
        "buffered data flushed without a tick or unpacing"
    );
    rig.paced.send(false).expect("paced channel closed");
    let got = data(&mut rig, Duration::from_millis(100)).await;
    assert_eq!(got, b"held");
    rig.task.abort();
}

/// RESIZE and CLOSE flush buffered DATA ahead of themselves in exact
/// wire order; the replacement paint after RESIZE forwards unbuffered
/// until the `?9001` marker — even across a split marker — while
/// `paced` still reads true.
#[tokio::test(start_paused = true)]
async fn resize_and_close_flush_buffered_data_in_wire_order() {
    let mut rig = rig(true).await;
    rig.core.send_stream(CH, Kind::Data, b"pre").await;
    rig.core.send(&Frame::resize(CH, 100, 40).encode()).await;
    assert_eq!(data(&mut rig, Duration::from_secs(2)).await, b"pre");
    match timeout(T, rig.rx.recv()).await {
        Ok(Some(PaneEv::Out {
            out: Output::Resize {
                cols: 100,
                rows: 40,
            },
            ..
        })) => {}
        _ => panic!("buffered DATA must precede the RESIZE it was flushed by"),
    }
    // Repaint bypass: bytes forward unbuffered until `?9001` lands.
    rig.core
        .send_stream(CH, Kind::Data, b"post-\x1b[?900")
        .await;
    rig.core.send_stream(CH, Kind::Data, b"1l-tail").await;
    let mut paint = Vec::new();
    while paint != b"post-\x1b[?9001l-tail" {
        paint.extend_from_slice(&data(&mut rig, T).await);
    }
    assert_eq!(paint, b"post-\x1b[?9001l-tail");
    // The marker ended the bypass: `held` paces again and rides out
    // ahead of CLOSE, in wire order.
    rig.core.send_stream(CH, Kind::Data, b"held").await;
    rig.core.send(&Frame::close(CH, Vec::new()).encode()).await;
    let mut tail = Vec::new();
    let closed = loop {
        match timeout(T, rig.rx.recv()).await {
            Ok(Some(PaneEv::Out {
                out: Output::Data(bytes),
                ..
            })) => tail.extend_from_slice(&bytes),
            Ok(Some(PaneEv::Out {
                out: Output::Closed(_),
                ..
            })) => break true,
            _ => break false,
        }
    };
    assert!(closed, "CLOSE never landed after the buffered DATA");
    assert_eq!(tail, b"held");
    match timeout(T, rig.rx.recv()).await {
        Ok(Some(PaneEv::Ended { .. })) => {}
        _ => panic!("expected PaneEv::Ended after the drained stream"),
    }
    rig.task.abort();
}
