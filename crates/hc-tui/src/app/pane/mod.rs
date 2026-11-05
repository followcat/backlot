// Derived in part from herdr src/pane/terminal.rs (Apache-2.0).
//! The one displayed terminal: attach lifecycle, role (active/observer),
//! VT state, credit, input backpressure and reattach policy — all owned
//! here so the app loop only sees `PaneEv` events and drawing state.
//!
//! Wire truths this encodes (see the attach docs in `hc-proto` and
//! `docs/runtime.md`):
//! - `observe` is fixed at attach time; a role change is a fresh attach.
//! - `resumed` marks recovery attaches so they don't stamp fresh
//!   ownership activity; a user-initiated switch is `resumed: false`.
//! - RESIZE-before-paint is only guaranteed when the reply confirms
//!   `resize_notifications`; otherwise the core grid comes from
//!   inventory sizes (a documented legacy path — cross-connection
//!   ordering is not guaranteed there, so a resize may apply after
//!   some bytes it precedes).
//! - There is no owner identity on the wire: a core-grid report that
//!   persistently disagrees with our requested size only *suggests*
//!   another client owns it — surfaced as such, never a tug-of-war.
//! - Input is never replayed across terminals or attach incarnations:
//!   keys/paste queue only for the attach generation they were typed
//!   under, and encode only under the fresh post-paint VT.

mod close;
mod held;
mod notices;
mod open;
mod pending_lifecycle;
mod pending_queue;
mod tasks;

pub use tasks::attach_task;

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hc_proto::client::{Attach, AttachError, Output};
use hc_proto::terminal::AttachParams;
use hc_proto::watch::View;
use hc_vt::{Rgb, Vt};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::host::HostFacts;
use crate::sidebar::{Life, classify};

pub(crate) use tasks::{Out, writer_task};

pub use state::*;

/// Human input queued while a pane attaches or its writer is full.
enum Pending {
    /// Ordered viewer control, including state sync before initial input.
    Control(Out),
    /// A key needing the live VT for encoding — the TerminalKey's
    /// text/shifted metadata travels with it until the fresh post-paint
    /// VT does the encode (one route, never a key-plus-text double send).
    Key(crate::app::input::TerminalKey),
    /// Paste text needing the VT's bracketed-paste mode.
    Paste(Vec<u8>),
    /// Already-encoded input bytes (a writer queue retry) — bound to
    /// the attach generation that encoded them; never replayed.
    Encoded(Vec<u8>, Option<u64>),
    /// Frozen key encoding retains semantic teardown tracking on writer retry.
    Prepared(Vec<u8>, crate::app::input::TerminalKey),
}

/// A negotiated attach being used.
struct Live {
    held: held::Held,
    spec: Spec,
    attach: Arc<Attach>,
    out: mpsc::Sender<Out>,
    reader: JoinHandle<()>,
    writer: JoinHandle<Result<(), String>>,
    /// Core grid (attach reply → RESIZE events → inventory).
    core_grid: (u16, u16),
    /// The reply's `resize_notifications` — the whole path sends
    /// ordered RESIZE-before-paint. Without it the core grid comes
    /// from inventory sizes (legacy: no cross-connection ordering).
    negotiated: bool,
    /// No DATA yet on this attach: the retained screen still shows and
    /// input waits — encoding under the old VT's modes would be wrong.
    await_paint: bool,
    paint_started: bool,
    paint_tail: Vec<u8>,
    /// The size we last successfully asked the core for.
    sent_size: Option<(u16, u16)>,
    /// Latest allocated size awaiting a send (coalesced on Full).
    desired_size: Option<(u16, u16)>,
    /// Focus awaiting a send (coalesced on Full).
    desired_focus: Option<bool>,
    /// Measured display facts coalesce independently of size ownership.
    desired_facts: Option<hc_proto::terminal::Display>,
    sent_facts: Option<hc_proto::terminal::Display>,
    /// Automatic replies the writer queue couldn't take yet.
    pending_reply: VecDeque<Vec<u8>>,
    /// A core-grid report disagreed with our request — another client's
    /// activity may own the size (the wire can't name an owner).
    mismatched_grid: bool,
    /// A VT failure worth surfacing (never fatal).
    vt_broken: Option<String>,
}

/// The pane's attach state.
enum State {
    /// Nothing selected.
    Idle,
    /// Open in flight; input already buffers. The `out` channel waits
    /// here for the writer task that `Opened` spawns. `attempts` keeps
    /// the backoff count across the attempt itself; `resumed` tells
    /// `Opened` this is wire recovery, not user-initiated ownership.
    Opening {
        generation: u64,
        spec: Spec,
        task: JoinHandle<()>,
        out: mpsc::Sender<Out>,
        out_rx: mpsc::Receiver<Out>,
        attempts: u32,
        resumed: bool,
    },
    /// Attached.
    Live(Box<Live>),
    /// Attach/stream failed; a retry is armed at `retry_at`.
    Down {
        spec: Spec,
        reason: String,
        retry_at: Instant,
        attempts: u32,
    },
    /// The target is not attachable (lost, finished, removed): waiting
    /// on the inventory, no retry.
    Gone {
        spec: Spec,
        reason: String,
        /// `true` when nothing on the wire can fix this (the core has
        /// no `terminal.attach`): view publishes must not reattach.
        permanent: bool,
    },
}

/// First attach retry delay; doubles to the cap.
const RETRY_MIN: Duration = Duration::from_millis(250);
const RETRY_MAX: Duration = Duration::from_secs(8);
/// Writer command queue bound.
const OUT_QUEUE: usize = 64;
/// Pending human input bound, in items and bytes.
const PENDING_MAX_ITEMS: usize = 128;
const PENDING_MAX_BYTES: usize = crate::app::input::MAX_INPUT_BYTES;
/// VT scrollback kept for the future copy mode.
const SCROLLBACK: usize = 256 * 1024;
/// First-paint history lines requested on a fresh attach.
const HISTORY: u64 = 200;

/// A swapped-out attach parked to drain forwarded input leases: the
/// program saw presses on this wire, so physical key-up releases must
/// return to it. Its tasks keep running — output events stay routed to
/// this struct by generation so its VT tracks live modes (a post-switch
/// `CSI =0u` resets what a release encodes) and credit keeps flowing —
/// while paint, cursor, focus and replies stay isolated to this wire.
struct Parked {
    held: held::Held,
    /// The attach generation this route binds — a terminal reincarnation
    /// gets a new generation, so an old wire is never confused with a
    /// replacement for the same address.
    route: u64,
    attach: Arc<Attach>,
    out: mpsc::Sender<Out>,
    /// The live VT for this wire — fed parked DATA so releases encode
    /// under the program's current modes.
    vt: Vt,
    /// Input accepted but held behind a Full writer — close drains it
    /// in order before the wire drops; bounded like a live queue.
    pending_input: VecDeque<Out>,
    pending_reply: VecDeque<Vec<u8>>,
    reader: JoinHandle<()>,
    writer: JoinHandle<Result<(), String>>,
}

/// A parked route mid-close, owned whole: the `done` task performs the
/// ordered shutdown — pending input through the writer's channel in
/// order, `out` dropped, writer awaited, the transport-write fence
/// crossed, the wire detached, and a `RouteClosed` wake sent. The
/// writer's `AbortHandle` stays here — `done` owns the JoinHandle it
/// awaits, so aborting `done` mid-await would only detach a blocked
/// writer; `writer_abort` cancels it for real.
struct Closing {
    /// The parked generation being closed — `RouteClosed` retires
    /// exactly this entry while idle.
    route: u64,
    done: JoinHandle<Result<(), String>>,
    writer_abort: tokio::task::AbortHandle,
}

/// The displayed terminal's whole state. `vt` lives outside the state:
/// the last screen stays painted (marked offline/down) until a fresh
/// attach's authoritative paint replaces it.
pub struct Pane {
    state: State,
    close_error: Option<String>,
    input_rejection: Option<String>,
    /// Swapped attaches still draining held-key releases (max a few).
    parked: Vec<Parked>,
    /// Parked routes mid-close: their drain+writer+attach stay owned —
    /// a close feeds accepted pending input through the writer in
    /// order, lets the writer finish queued sends, then drops the wire.
    closing: Vec<Closing>,
    /// Keys actually dispatched to a wire and the generation that took
    /// them (bounded FIFO): a press queued while attaching flushes onto
    /// a reincarnated attach, so the lease's true route is where the
    /// press was sent — never guessed.
    pub(crate) dispatched: VecDeque<(u64, u64)>,
    /// Generation counter: each spawn bumps it; stale events die.
    generation: u64,
    /// The app's pane-event channel (actors clone the sender).
    ev: mpsc::Sender<PaneEv>,
    open: OpenAttach,
    /// The VT/screen — retained across Down and re-Opening for the same
    /// spec; `None` on Idle or after a spec change.
    vt: Option<Vt>,
    /// The core grid the retained VT renders, when known.
    vt_grid: (u16, u16),
    /// Allocated inner size as last reported by the layout.
    allocated: (u16, u16),
    outer_focus: Option<bool>,
    focus_changed_disconnected: bool,
    allocation_changed_disconnected: bool,
    incarnation: Option<(u64, Option<u32>)>,
    /// Human input held while attaching / behind a full writer queue.
    /// Bound to this attach generation: a new incarnation drains any
    /// already-encoded leftovers (they belong to the dead wire).
    pending: VecDeque<Pending>,
    pending_bytes: usize,
    /// Input dropped because `pending` was full — surfaced, never silent.
    dropped_input: u64,
    /// The probed outer-terminal facts: seeded into every fresh VT so
    /// `CellColor::Default` and palette entries resolve against the
    /// host's real defaults rather than the VT's built-ins.
    host: HostFacts,
    /// The VT's post-seed colors at creation — the baseline the draw
    /// layer compares against to tell "program changed the default"
    /// from "default equals the host's". `None` until the first VT.
    initial: Option<hc_vt::Colors>,
    /// Attach attempts made (probe).
    pub attaches: u64,
}

/// What `cell_style` needs to reproduce the reference's
/// `ghostty_default_fg/bg` + `PaletteOverrides`: the VT's defaults at
/// creation (detects a program `OSC 10/11/4` change) and the host's
/// reported defaults (detects "unchanged = transparent over host").
#[derive(Clone, Copy)]
pub struct Paint {
    pub initial_fg: Option<Rgb>,
    pub initial_bg: Option<Rgb>,
    /// Post-seed palette: entries still equal mean "not overridden"
    /// and forward as `Indexed` so the host palette resolves them.
    pub initial_palette: Option<[Rgb; 256]>,
    pub host_fg: Option<Rgb>,
    pub host_bg: Option<Rgb>,
}

/// The byte charge of a pending item — keys count their real payload
/// (generated text + raw sequence + base), so the 256KiB bound still
/// limits the bytes actually retained.
fn charge(item: &Pending) -> usize {
    match item {
        Pending::Control(_) => 0,
        Pending::Key(k) => 32 + k.text_commit.as_deref().map_or(0, str::len) + k.vt_bytes.len(),
        Pending::Paste(t) | Pending::Encoded(t, _) => t.len(),
        Pending::Prepared(bytes, key) => bytes.len() + charge(&Pending::Key(key.clone())),
    }
}

impl Pane {
    pub fn new(ev: mpsc::Sender<PaneEv>, open: OpenAttach) -> Pane {
        Pane {
            state: State::Idle,
            close_error: None,
            input_rejection: None,
            parked: Vec::new(),
            closing: Vec::new(),
            dispatched: VecDeque::new(),
            generation: 0,
            ev,
            open,
            vt: None,
            vt_grid: (80, 24),
            allocated: (80, 24),
            outer_focus: None,
            focus_changed_disconnected: false,
            allocation_changed_disconnected: false,
            incarnation: None,
            pending: VecDeque::new(),
            pending_bytes: 0,
            dropped_input: 0,
            host: HostFacts::default(),
            initial: None,
            attaches: 0,
        }
    }
}

/// Aborts a Live attach's tasks — every path that leaves Live calls it
/// (teardown, shutdown, end-of-wire, core Closed/Failed) so no task
/// outlives its generation.
fn leave_live(live: &Live) {
    live.reader.abort();
    live.writer.abort();
}

/// Owned work never outlives the pane: a dropped pane aborts its
/// attach reader and writer even when an explicit shutdown never ran
/// (cancelled futures, panics). Attach teardown itself is bounded —
/// `Attach::drop` detaches without killing the terminal.
impl Drop for Pane {
    fn drop(&mut self) {
        match &self.state {
            State::Opening { task, .. } => task.abort(),
            State::Live(live) => leave_live(live),
            _ => {}
        }
        for parked in &self.parked {
            parked.reader.abort();
            parked.writer.abort();
        }
        for closing in &self.closing {
            closing.writer_abort.abort();
            closing.done.abort();
        }
    }
}

mod events;
pub(crate) mod input;
mod lifecycle;
mod state;

#[cfg(test)]
mod tests;

mod surface;
