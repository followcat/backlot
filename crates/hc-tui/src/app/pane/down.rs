//! Down parking: attach open failures and mid-stream ends park the
//! spec in `Down` with bounded backoff — or `Gone` when the wire can
//! never work — never a hot retry loop.
use super::*;

impl Pane {
    /// The open failed: bounded backoff (carrying prior attempts) before
    /// the next try. An `unsupported` answer means the core cannot
    /// attach at all — the pane parks Gone with the reason visible
    /// instead of retrying forever; the inventory view reconciles it.
    pub(super) fn on_failed(&mut self, why: String, unsupported: bool, now: Instant) {
        let State::Opening { spec, attempts, .. } = &self.state else {
            return;
        };
        let (spec, attempts) = (spec.clone(), *attempts);
        self.state = State::Idle;
        if unsupported {
            self.state = State::Gone {
                spec,
                reason: why,
                permanent: true,
            };
        } else {
            self.arm_down(spec, why, attempts, now);
        }
    }

    /// The wire ended mid-stream: the next retry reattaches with
    /// `resumed` so recovery doesn't stamp fresh size-ownership
    /// activity. Owned tasks are aborted so nothing outlives the pane.
    pub(super) fn on_ended(&mut self, now: Instant) {
        let State::Live(live) = &self.state else {
            return; // already Down/Gone — keep the recorded reason
        };
        let spec = live.spec.clone();
        leave_live(live);
        self.arm_down(spec, "the attach stream ended".into(), 0, now);
    }

    /// Parks `spec` in `Down` with exponential backoff.
    pub(super) fn arm_down(&mut self, spec: Spec, reason: String, attempts: u32, now: Instant) {
        if matches!(self.state, State::Live(_)) {
            self.discard_pending_incarnation();
        }
        let delay = RETRY_MIN
            .saturating_mul(1u32 << attempts.min(10))
            .min(RETRY_MAX);
        self.state = State::Down {
            spec,
            reason,
            retry_at: now + delay,
            attempts: attempts + 1,
        };
    }
}
