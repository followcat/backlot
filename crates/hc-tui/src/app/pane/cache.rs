//! Cached-screen handover: what one pane lends the next across a switch,
//! and the conditions under which an attach may continue it instead of
//! replaying history.
use super::*;

/// A screen handed from one pane to the next: the VT, its post-seed color
/// baseline and its painted-surface cache move together, so the receiver
/// keeps the last paint — cursor shape, palette overrides and all — while
/// its own attach opens. `warm` is `Some` only when the giver's wire
/// stayed live up to the handover: that is the one case where the next
/// attach may ask for the screen alone and continue this VT.
pub(crate) struct Retained {
    pub vt: Vt,
    pub initial: Option<hc_vt::Colors>,
    pub cache: Option<render_cache::PaneRenderCache>,
    pub warm: Option<WarmVt>,
}

/// Which terminal incarnation a cached screen belongs to. A restarted
/// terminal gets a new `(created_ms, pid)`; its predecessor's VT must
/// never be continued into it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WarmVt {
    pub incarnation: (u64, Option<u32>),
}

impl Pane {
    /// The retained screen handed to a new owner: the VT, its `initial`
    /// baseline and its painted-surface cache move together so the
    /// receiver keeps the last paint — cursor shape, palette overrides,
    /// warm cells and all — while its own attach opens. `None` when
    /// there is no screen to hand over; `warm` is set only while this
    /// pane's wire stayed live up to the handover.
    pub(crate) fn take_retained(&mut self) -> Option<Retained> {
        let warm = self.warm_ticket();
        let vt = self.vt.take()?;
        self.vt_current = false;
        Some(Retained {
            vt,
            initial: self.initial.take(),
            cache: self.render_cache.take(),
            warm,
        })
    }

    /// What this pane can lend the next attach: only a VT whose wire
    /// stayed live up to now, for a terminal incarnation we know. A stale
    /// screen still seeds a *display*, but never a warm attach.
    pub(crate) fn warm_ticket(&self) -> Option<WarmVt> {
        let incarnation = self.incarnation?;
        (self.vt_current && self.vt.is_some()).then_some(WarmVt { incarnation })
    }

    /// The warm screen an incoming active wire must take over at its
    /// first paint — `None` unless the VT is live, painted and current.
    pub(crate) fn take_warm(&mut self) -> Option<Retained> {
        self.warm_ticket()?;
        self.take_retained()
    }

    /// Installs a screen another pane handed over. The VT stays on screen
    /// until this pane's first authoritative paint replaces it, and
    /// `warm_expect` carries the incarnation that paint must belong to.
    pub(super) fn install_retained(&mut self, retained: Retained) {
        if let Ok(grid) = retained.vt.size() {
            self.vt_grid = grid;
        }
        self.vt_current = retained.warm.is_some();
        self.initial = retained.initial;
        self.render_cache = retained.cache;
        self.vt = Some(retained.vt);
        self.warm_expect = retained.warm;
    }

    /// Adopts the cached screen at the active wire's first paint: the
    /// paint lands in this VT instead of a fresh one, so the scrollback
    /// the cached pane kept survives the switch.
    pub(crate) fn adopt_warm(&mut self, retained: Retained) {
        self.install_retained(retained);
    }

    /// The app promises a warm VT before this attach's first paint (it
    /// lives in a cached observer that still feeds it). `None` — no
    /// cached screen — means a cold attach.
    pub(crate) fn expect_warm(&mut self, warm: Option<WarmVt>) {
        self.warm_expect = warm;
    }

    /// This attach asked for the screen only and has not painted yet: the
    /// app must hand the cached VT over before the first DATA lands.
    pub(crate) fn awaits_warm_paint(&self) -> bool {
        self.await_warm
            && self.vt.is_none()
            && matches!(&self.state, State::Live(live) if !live.paint_started)
    }

    /// The incarnation the current wire was opened against.
    pub(crate) fn incarnation(&self) -> Option<(u64, Option<u32>)> {
        self.incarnation
    }

    /// The wire ended: its VT may have missed bytes, so it is no longer
    /// current and no attach may continue it.
    pub(super) fn wire_lost(&mut self) {
        self.vt_current = false;
        self.await_warm = false;
        self.warm_expect = None;
    }
}
