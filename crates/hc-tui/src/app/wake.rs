//! Wake-scoped loop hygiene: which select arm produced the turn, the
//! bounded channel drains that keep one turn from hoarding the loop,
//! and the upkeep each wake source actually needs. The point is that a
//! background DATA burst never pays for input bookkeeping and queued
//! UI events never queue behind a reply storm.
use super::*;
use tokio::sync::mpsc::error::TryRecvError;

/// Which select arm produced this turn — housekeeping scopes itself
/// to the wake's source instead of running every pass per wake.
#[derive(Clone, Copy)]
pub(super) enum WakeKind {
    /// A channel-close wake: the arm disarmed itself, nothing to do.
    None,
    /// A UI event — key, text, paste, resize, focus or mouse.
    Input,
    /// Host-terminal replies, folded as one batch.
    Replies,
    /// A watcher publish or a worker report.
    View,
    /// A `herdr.seen` label-write result.
    Seen,
    /// A Waiting probe verdict.
    Waiting,
    /// Attach-actor reports, drained as a bounded batch.
    Pane,
    /// The armed one-shot deadline.
    Deadline,
    /// A studio-action result — the notice or view it lands marks
    /// the scheduler itself, so no housekeeping follows it.
    Panel,
    /// The ActionTracker's reply wake — same no-housekeeping rule
    /// as `Panel`; S3 folded dock and sheet replies into it.
    Hollywood,
}

impl WakeKind {
    /// Key-route reconciliation and input rejection: arms that can
    /// change leases or forward input.
    fn routes(self) -> bool {
        matches!(self, Self::Input | Self::View | Self::Pane | Self::Deadline)
    }

    /// Ticking clocks — notices, notifications, selection, copy
    /// feedback and due tab status — run on real time passing, not on
    /// every channel wake.
    fn clocks(self) -> bool {
        matches!(self, Self::Input | Self::View)
    }

    /// Pane bells and clipboard writes drain where their events land.
    fn drains(self) -> bool {
        matches!(self, Self::Pane | Self::Deadline)
    }

    /// The `herdr.seen` label share follows view, input and results.
    fn shares(self) -> bool {
        matches!(self, Self::View | Self::Input | Self::Seen)
    }

    /// The window title tracks what the user can see or drive.
    fn title(self) -> bool {
        matches!(self, Self::View | Self::Input | Self::Pane | Self::Waiting)
    }

    /// Pane targets and observers re-aim only on view or input —
    /// pane/reply/seen/deadline wakes never re-attach.
    fn retargets(self) -> bool {
        matches!(self, Self::Input | Self::View)
    }

    /// Keyboard reporting and pixel-mouse mode sync almost always.
    fn modes(self) -> bool {
        matches!(
            self,
            Self::Input | Self::View | Self::Pane | Self::Replies | Self::Deadline
        )
    }
}

/// `first` plus every reply already queued, folded into one batch —
/// the startup probe storm is ~258 answers and must apply its facts
/// once, not once per reply. Bounded so an endless producer still
/// yields the turn; a disconnect closes the arm for good.
pub(super) fn drain_replies(
    replies: &mut mpsc::Receiver<crate::host::HostReply>,
    first: crate::host::HostReply,
    open: &mut bool,
) -> Vec<crate::host::HostReply> {
    const LIMIT: usize = 512;
    let mut batch = Vec::with_capacity(LIMIT.min(300));
    batch.push(first);
    while batch.len() < LIMIT {
        match replies.try_recv() {
            Ok(reply) => batch.push(reply),
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                *open = false;
                break;
            }
        }
    }
    batch
}

impl App {
    /// The earliest armed wake across pane retries, the draw
    /// scheduler and every ticking UI clock.
    pub(super) fn wake_deadline(&self) -> Option<Instant> {
        let due = match (self.pane_wake(), self.sched.deadline()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        due.into_iter()
            .chain(self.notice.as_ref().map(|n| n.deadline))
            .chain(self.notification_deadline())
            .chain(self.next_tab_status)
            .chain(self.selection_autoscroll_deadline)
            .chain(self.selection_highlight_clear_deadline)
            .chain(self.panel_anim_deadline)
            .chain(self.opening_anim_deadline)
            .chain(self.opening.cap)
            .chain(self.copy_feedback_deadline)
            .chain(self.waiting_due)
            .chain(self.image_read.as_ref().map(|read| read.deadline))
            .chain(self.action_toast.as_ref().map(|toast| toast.deadline))
            .chain(self.reader_sent_deadline())
            .chain(self.install_tick_deadline())
            .min()
    }

    /// `first` plus a shallow drain of queued attach reports: a DATA
    /// flood stops early for a queued UI event or a passed deadline
    /// and re-races the arms next turn.
    pub(super) fn drain_pane_events(
        &mut self,
        first: PaneEv,
        wake: Option<Instant>,
        events: &mpsc::Receiver<UiEvent>,
        open: &mut bool,
    ) -> bool {
        const LIMIT: usize = 16;
        let mut dirty = self.pane_event(first, self.now());
        for _ in 1..LIMIT {
            if !events.is_empty() || wake.is_some_and(|due| self.now() >= due) {
                break;
            }
            match self.pane_rx.try_recv() {
                Ok(ev) => dirty |= self.pane_event(ev, self.now()),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    *open = false;
                    break;
                }
            }
        }
        dirty
    }

    /// Post-turn upkeep, scoped to the wake's source: only the arms
    /// that can move its state run its clocks, drains and syncs. A
    /// deadline that already passed during the turn is owed its work
    /// here rather than re-arming the select. `arm_dirty` carries the
    /// select arm's own report — one `mark_dirty` covers both.
    pub(super) fn housekeeping(&mut self, kind: WakeKind, wake: Option<Instant>, arm_dirty: bool) {
        let now = self.now();
        let mut dirty = arm_dirty;
        let deadline_due = matches!(kind, WakeKind::Deadline) || wake.is_some_and(|due| now >= due);
        // Anti-starvation: a wake that queued after the pane retry's
        // deadline still owes the due retry now.
        if !matches!(kind, WakeKind::Deadline)
            && deadline_due
            && self.pane_wake().is_some_and(|due| now >= due)
        {
            dirty |= self.pane_retry(now);
        }
        // The image-read hold bound is owed on any wake that lands at
        // or past it, not only the deadline arm itself.
        if deadline_due {
            dirty |= self.expire_image_read(now);
        }
        if kind.routes() {
            self.reconcile_key_routes();
            if let Some(reason) = self.pane.take_input_rejection() {
                self.reject_input(&reason);
            }
        }
        if deadline_due || kind.clocks() {
            dirty |= self.expire_notice(now);
            dirty |= self.expire_action_toast(now);
            dirty |= self.expire_reader_sent(now);
            dirty |= self.tick_notifications(now);
            self.tick_panel(now);
            self.settle_opening(now);
            self.tick_opening(now);
            dirty |= self.tick_selection(now) | self.tick_copy_feedback(now);
            if self.next_tab_status.is_some_and(|due| now >= due) {
                dirty |= self.update_tab_status(now);
            }
        }
        // The armed re-probe cadence fires through the same deadline;
        // publish-driven ticks already ran inside `on_view`.
        if deadline_due {
            self.tick_waiting(now);
        }
        if kind.drains() {
            self.drain_bells();
            dirty |= self.drain_clipboard_writes();
        }
        if kind.shares() {
            self.share_seen();
        }
        if kind.title() {
            self.update_window_title();
        }
        if dirty {
            self.sched.mark_dirty(now);
        }
    }

    /// Wire-side sync after `maybe_draw`: pane re-aiming only happens
    /// on input/view turns (or the first pass that seeds the pre-loop
    /// `on_view`), never on pane/reply/seen/deadline wakes; keyboard
    /// and pixel-mouse modes track everything but idle/seen wakes.
    pub(super) fn post_draw_sync(&mut self, kind: WakeKind, need_pane_sync: &mut bool) {
        if *need_pane_sync || kind.retargets() {
            self.sync_target();
            self.sync_observers();
            *need_pane_sync = false;
        }
        if kind.modes() {
            self.sync_keyboard_report_all();
            self.sync_pixel_mouse();
        }
    }
}
