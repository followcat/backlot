//! The redraw scheduler: pure bookkeeping for "draw when dirty, at most
//! ~60 times a second, never on a timer". The event loop marks work dirty
//! as state changes; the scheduler answers whether a draw may happen now
//! or a one-shot deadline should be armed — and it is armed *only* while
//! dirty work waits, so an idle app parks on real events forever.
//!
//! Time comes in as `Instant`s, so tests drive it with plain arithmetic.

use std::time::{Duration, Instant};

/// The redraw cap: one frame every ~16.7 ms.
pub const FRAME: Duration = Duration::from_nanos(16_666_667);

/// Dirty/deadline state plus instrumentation counters for tests.
#[derive(Debug, Default)]
pub struct Scheduler {
    /// When the last draw completed; `None` until the first.
    last_draw: Option<Instant>,
    /// The armed draw deadline; `Some` only while dirty work waits.
    due: Option<Instant>,
    /// Draws performed (probe).
    pub draws: u64,
    /// Times the loop armed a timer to reach `due` (probe).
    pub armed: u64,
}

impl Scheduler {
    /// State changed: schedule the earliest allowed draw. `now` means
    /// "immediately allowed" — still a scheduled draw, not a sleep.
    pub fn mark_dirty(&mut self, now: Instant) {
        if self.due.is_some() {
            return; // coalesced: an armed deadline covers new dirt
        }
        let earliest = self.last_draw.map(|last| last + FRAME).unwrap_or(now);
        self.due = Some(earliest.max(now));
    }

    /// The armed deadline, when dirty work is waiting.
    pub fn deadline(&self) -> Option<Instant> {
        self.due
    }

    /// Whether the armed deadline has been reached.
    pub fn due_now(&self, now: Instant) -> bool {
        self.due.is_some_and(|due| due <= now)
    }

    /// A draw ran: un-arms the deadline and stamps it.
    pub fn drew(&mut self, now: Instant) {
        self.last_draw = Some(now);
        self.due = None;
        self.draws += 1;
    }

    /// A scheduled draw produced no output (identical frame): the dirt
    /// is consumed without stamping `last_draw`, so the next real
    /// change is not pushed a whole frame slot out.
    pub fn unarm(&mut self) {
        self.due = None;
    }

    /// Bookkeeping for the loop arming a one-shot sleep toward `due`.
    pub fn armed_timer(&mut self) {
        self.armed += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(ms: u64) -> Instant {
        Instant::now() + Duration::from_millis(ms)
    }

    /// A burst of events inside one frame produces one deadline and one
    /// draw — coalescing, not one draw per event.
    #[test]
    fn a_burst_coalesces_to_one_draw() {
        let mut sched = Scheduler::default();
        let t0 = t(0);
        for i in 0..50 {
            sched.mark_dirty(t0 + Duration::from_millis(i));
            assert!(sched.due_now(t0 + Duration::from_millis(i)));
        }
        sched.drew(t0);
        assert_eq!(sched.draws, 1);
        assert_eq!(sched.deadline(), None);
    }

    /// Draws are capped: a flood over three frames draws at most one per
    /// frame, even when dirt arrives every millisecond.
    #[test]
    fn frequency_is_capped_at_one_frame() {
        let mut sched = Scheduler::default();
        let t0 = t(0);
        let mut draws = 0;
        for ms in 0..50 {
            let now = t0 + Duration::from_millis(ms);
            sched.mark_dirty(now);
            if sched.due_now(now) {
                sched.drew(now);
                draws += 1;
            }
        }
        // 50 ms of continuous dirt fits at most 4 frame slots.
        assert!(draws <= 4, "drew {draws} times in 50ms");
        assert!(draws >= 3, "expected several draws, got {draws}");
    }

    /// Idle: no dirt means no armed deadline — the loop parks on events.
    #[test]
    fn clean_state_arms_no_deadline() {
        let mut sched = Scheduler::default();
        sched.drew(t(0));
        assert_eq!(sched.deadline(), None);
        assert!(!sched.due_now(t(10_000)));
    }

    /// After drawing, fresh dirt waits for the next frame slot rather
    /// than drawing back-to-back.
    #[test]
    fn fresh_dirt_waits_for_the_next_slot() {
        let mut sched = Scheduler::default();
        let t0 = t(0);
        sched.mark_dirty(t0);
        sched.drew(t0);
        sched.mark_dirty(t0);
        assert_eq!(sched.deadline(), Some(t0 + FRAME));
        assert!(!sched.due_now(t0 + Duration::from_millis(10)));
        assert!(sched.due_now(t0 + FRAME));
    }

    /// A draw that emitted nothing consumes its deadline without
    /// stamping `last_draw`: fresh dirt draws immediately instead of
    /// waiting a frame slot behind a phantom frame.
    #[test]
    fn an_unemitted_draw_unarms_without_stamping_last_draw() {
        let mut sched = Scheduler::default();
        let t0 = t(0);
        sched.mark_dirty(t0);
        assert!(sched.due_now(t0));
        sched.unarm();
        assert_eq!(sched.deadline(), None);
        sched.mark_dirty(t0);
        assert!(sched.due_now(t0));
    }
}
