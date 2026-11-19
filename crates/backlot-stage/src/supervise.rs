//! The `stage watch --restart` policy: a pure, clock-injected
//! decision that keeps `backlot-stage` alive without letting a crash
//! loop spin forever. At most [`MAX_RESTARTS`] starts inside
//! [`WINDOW_MS`], an exponential backoff between repeated failures,
//! and a pause — announced once per episode — when the budget is
//! spent. A stage that stays live for [`STABLE_MS`] resets the
//! backoff; a stage that comes back on its own ends the pause.

/// Restart attempts allowed inside [`WINDOW_MS`].
pub(crate) const MAX_RESTARTS: usize = 5;
/// The sliding window the attempts are counted in: ten minutes.
pub(crate) const WINDOW_MS: u64 = 600_000;
/// The delay before each successive restart after a failed one; the
/// last value is the cap.
pub(crate) const BACKOFF_MS: [u64; 5] = [2_000, 4_000, 8_000, 16_000, 30_000];
/// A stage live this long counts as stable: the backoff resets.
pub(crate) const STABLE_MS: u64 = 60_000;

/// What one observation of the stage's liveness should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RestartStep {
    /// Nothing to start yet — live, inside a backoff, or paused.
    Wait,
    /// The budget allows it and the backoff ran out: start the stage.
    Start,
    /// The window already holds [`MAX_RESTARTS`] attempts — announce
    /// the pause once and keep waiting for one to fall out.
    Paused,
}

/// The restart decision state, fed one observation per poll round.
#[derive(Debug)]
pub(crate) struct RestartPolicy {
    /// When each restart inside the window was attempted, oldest first.
    attempts: std::collections::VecDeque<u64>,
    /// When the next start is due; `None` while the stage is live.
    next_at: Option<u64>,
    /// Index into [`BACKOFF_MS`] for the next delay.
    backoff: usize,
    /// When the stage was first seen live in the current stretch.
    live_since: Option<u64>,
    /// Whether this pause episode was already announced.
    paused: bool,
}

impl RestartPolicy {
    pub(crate) fn new() -> RestartPolicy {
        RestartPolicy {
            attempts: std::collections::VecDeque::new(),
            next_at: None,
            backoff: 0,
            live_since: None,
            paused: false,
        }
    }

    /// One observation: whether the stage is live right now and the
    /// injected clock. Live ends any pause and, after
    /// [`STABLE_MS`], resets the backoff; dead records an attempt no
    /// more often than the backoff and once the budget allows.
    pub(crate) fn observe(&mut self, live: bool, now_ms: u64) -> RestartStep {
        if live {
            let since = *self.live_since.get_or_insert(now_ms);
            if now_ms.saturating_sub(since) >= STABLE_MS {
                self.backoff = 0;
            }
            self.next_at = None;
            self.paused = false;
            return RestartStep::Wait;
        }
        self.live_since = None;
        let cutoff = now_ms.saturating_sub(WINDOW_MS);
        while self.attempts.front().is_some_and(|at| *at < cutoff) {
            self.attempts.pop_front();
        }
        if self.attempts.len() >= MAX_RESTARTS {
            if self.paused {
                return RestartStep::Wait;
            }
            self.paused = true;
            return RestartStep::Paused;
        }
        // A freed slot ends the pause episode; the old due time is
        // in the past by now, so the next round starts at once.
        self.paused = false;
        let due = *self.next_at.get_or_insert(now_ms);
        if now_ms < due {
            return RestartStep::Wait;
        }
        self.attempts.push_back(now_ms);
        let delay = BACKOFF_MS[self.backoff.min(BACKOFF_MS.len() - 1)];
        self.backoff = (self.backoff + 1).min(BACKOFF_MS.len() - 1);
        self.next_at = Some(now_ms + delay);
        RestartStep::Start
    }
}

#[cfg(test)]
#[path = "supervise_tests.rs"]
mod tests;
