//! Off-tab observer credit pacing. `App::sync_observers` keeps cached
//! `Role::Observe` attaches alive off-tab; unpaced, every `Output::Data`
//! chunk would cross `pane_rx` and wake `App::run`. Coalescing chunks
//! onto a shared 500ms flush grid bounds those wakes to ~2/s total and
//! keeps `Attach::consume` withheld in `Stream::unconsumed` until the
//! flush — capping in-flight bytes per window while dropping nothing.

use std::sync::Arc;
use std::time::Duration;

use hc_proto::client::{Attach, Output};
use tokio::sync::mpsc;

use super::*;

/// The shared off-tab flush grid: every paced observer computes its
/// next flush against the same epoch, so all their flushes land inside
/// one `drain_pane_events` turn.
pub(crate) const OFF_TAB_FLUSH_INTERVAL: Duration = Duration::from_millis(500);

/// Replacement paint after `Output::Resize` bypasses pacing this long
/// or until the `?9001` marker lands — a paced buffer must never stall
/// the pane's bounded repaint wait.
const REPAINT_BYPASS: Duration = Duration::from_millis(250);

/// The `?9001h/l` marker is 8 bytes: 7 trailing bytes of context
/// bridge a marker split across DATA chunk boundaries.
const MARKER_TAIL: usize = 7;

/// A size holder returns credit for its discarded stream in 64 KiB
/// steps (`INITIAL_CREDIT / 4`): per-chunk CREDIT frames only multiply
/// transport writes for bytes nobody renders, while the core's 256 KiB
/// window keeps at least 192 KiB open — a paint never stalls on the
/// withheld share.
const SIZE_LEASE_CREDIT_STEP: usize = hc_proto::frame::INITIAL_CREDIT as usize / 4;

/// The pane's pacing state: `off_tab` arrives from `App::sync_observers`
/// each pass; `tx` carries the effective flag — off-tab AND painted —
/// to the attach actor's forward loop.
pub(super) struct Pacing {
    off_tab: bool,
    tx: tokio::sync::watch::Sender<bool>,
    epoch: tokio::time::Instant,
}

impl Default for Pacing {
    fn default() -> Self {
        let (tx, _rx) = tokio::sync::watch::channel(false);
        Pacing {
            off_tab: false,
            tx,
            epoch: tokio::time::Instant::now(),
        }
    }
}

impl Pacing {
    /// The attach actor's live view of the effective paced flag.
    pub(super) fn subscribe(&self) -> tokio::sync::watch::Receiver<bool> {
        self.tx.subscribe()
    }

    pub(super) fn tick_epoch(&self) -> tokio::time::Instant {
        self.epoch
    }
}

impl Pane {
    /// Set the shared grid epoch used by off-tab observer pacing.
    pub fn set_tick_epoch(&mut self, epoch: tokio::time::Instant) {
        self.pacing.epoch = epoch;
    }

    /// `App::sync_observers` marks observers outside the visible set;
    /// visible observers are rendered surfaces and stay live.
    pub(crate) fn set_off_tab(&mut self, off_tab: bool) {
        self.pacing.off_tab = off_tab;
        self.sync_pacing();
    }

    /// The effective flag: off-tab AND first paint complete. A pane
    /// still waiting on `?9001`/repaint stays unpaced so initial paint
    /// and resize recovery never stall on a flush window.
    pub(super) fn sync_pacing(&self) {
        let want = self.pacing.off_tab && self.has_fresh_paint();
        self.pacing.tx.send_if_modified(|cur| {
            let changed = *cur != want;
            *cur = want;
            changed
        });
    }
}

/// The next shared-grid instant after `now`: every paced observer
/// lands on the same tick.
fn next_off_tab_tick(
    epoch: tokio::time::Instant,
    now: tokio::time::Instant,
) -> tokio::time::Instant {
    let elapsed_ms = now.saturating_duration_since(epoch).as_millis() as u64;
    let step = OFF_TAB_FLUSH_INTERVAL.as_millis() as u64;
    epoch + Duration::from_millis(((elapsed_ms / step) + 1) * step)
}

/// `?9001h`/`?9001l` in `tail + bytes`, carrying the scan tail across
/// DATA chunk boundaries like the paint gate does.
fn marker_seen(tail: &mut Vec<u8>, bytes: &[u8]) -> bool {
    let mut scan = std::mem::take(tail);
    scan.extend_from_slice(bytes);
    if scan
        .windows(8)
        .any(|part| part == b"\x1b[?9001h" || part == b"\x1b[?9001l")
    {
        return true;
    }
    let keep = scan.len().min(MARKER_TAIL);
    tail.extend_from_slice(&scan[scan.len() - keep..]);
    false
}

/// An `Option<Instant>` deadline arm: pending forever while `None`.
async fn sleep_until_opt(at: Option<tokio::time::Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

/// One event onto the pane channel; `false` when the pane is gone.
async fn send_out(ev: &mpsc::Sender<PaneEv>, generation: u64, out: Output) -> bool {
    ev.send(PaneEv::Out { generation, out }).await.is_ok()
}

/// The attach→pane forward loop. A size holder consumes DATA inline
/// and drops Control exactly as before; observers pace off-tab DATA
/// onto the shared grid while every other event keeps wire order —
/// a resize or close flushes the buffer ahead of itself.
pub(super) async fn forward_stream(
    attach: &Arc<Attach>,
    role: Role,
    generation: u64,
    ev: &mpsc::Sender<PaneEv>,
    mut paced: tokio::sync::watch::Receiver<bool>,
    tick_epoch: tokio::time::Instant,
) {
    // Buffered bytes plus their flush instant ride one state: the
    // tick flushes whatever accumulated since the arm.
    let mut buffered: Vec<u8> = Vec::new();
    let mut flush_at: Option<tokio::time::Instant> = None;
    let mut paced_open = true;
    // Post-RESIZE bypass: replacement paint forwards unbuffered until
    // the `?9001` marker or the deadline — the pane's own repaint wait
    // must never lose a race to a 500ms flush window.
    let mut repaint_until: Option<tokio::time::Instant> = None;
    let mut marker_tail: Vec<u8> = Vec::new();
    // Consumed size-lease bytes not yet credited — released once the
    // batch reaches `SIZE_LEASE_CREDIT_STEP`.
    let mut lease_credit = 0usize;
    macro_rules! flush {
        () => {{
            flush_at = None;
            if !buffered.is_empty() {
                let out = Output::Data(std::mem::take(&mut buffered));
                if !send_out(ev, generation, out).await {
                    return;
                }
            }
        }};
    }
    loop {
        tokio::select! {
            biased;
            // Unpacing (visible again, or paint state lost) delivers
            // buffered bytes at once — never behind the next tick.
            changed = paced.changed(), if paced_open && !buffered.is_empty() => {
                match changed {
                    Err(_) => paced_open = false,
                    Ok(()) => {
                        if !*paced.borrow_and_update() {
                            flush!();
                        }
                    }
                }
            }
            _ = sleep_until_opt(flush_at), if flush_at.is_some() => {
                flush!();
            }
            out = attach.recv() => {
                let Some(out) = out else {
                    // Terminal path: drained bytes precede `Ended` even
                    // when the channel is already gone.
                    if !buffered.is_empty() {
                        let out = Output::Data(std::mem::take(&mut buffered));
                        let _ = send_out(ev, generation, out).await;
                    }
                    let _ = ev.send(PaneEv::Ended { generation }).await;
                    return;
                };
                if role == Role::SizeLease {
                    match out {
                        Output::Data(bytes) => {
                            lease_credit += bytes.len();
                            if lease_credit >= SIZE_LEASE_CREDIT_STEP {
                                let step =
                                    lease_credit - lease_credit % SIZE_LEASE_CREDIT_STEP;
                                if let Err(error) = attach.consume(step) {
                                    let out = Output::Failed(format!("credit: {error}"));
                                    let _ = send_out(ev, generation, out).await;
                                    return;
                                }
                                lease_credit -= step;
                            }
                            continue;
                        }
                        Output::Control { .. } => continue,
                        _ => {}
                    }
                    if !send_out(ev, generation, out).await {
                        return;
                    }
                    continue;
                }
                match out {
                    Output::Resize { .. } => {
                        // The buffer precedes the resize in wire order;
                        // replacement paint then bypasses pacing until
                        // the `?9001` marker or the bypass deadline.
                        flush!();
                        if !send_out(ev, generation, out).await {
                            return;
                        }
                        repaint_until = Some(tokio::time::Instant::now() + REPAINT_BYPASS);
                        marker_tail.clear();
                    }
                    Output::Data(bytes) => {
                        // A bypass live when these bytes arrived
                        // forwards them at once — even when their own
                        // `?9001` marker ends the window.
                        let bypass =
                            repaint_until.is_some_and(|dl| tokio::time::Instant::now() < dl);
                        if bypass && marker_seen(&mut marker_tail, &bytes) {
                            repaint_until = None;
                            marker_tail.clear();
                        }
                        if bypass || !*paced.borrow_and_update() {
                            if buffered.is_empty() {
                                if !send_out(ev, generation, Output::Data(bytes)).await {
                                    return;
                                }
                            } else {
                                buffered.extend_from_slice(&bytes);
                                flush!();
                            }
                        } else {
                            buffered.extend_from_slice(&bytes);
                            flush_at.get_or_insert_with(|| {
                                next_off_tab_tick(tick_epoch, tokio::time::Instant::now())
                            });
                            if ev.is_closed() {
                                return;
                            }
                        }
                    }
                    other => {
                        flush!();
                        if !send_out(ev, generation, other).await {
                            return;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "pacing_tests.rs"]
mod tests;
