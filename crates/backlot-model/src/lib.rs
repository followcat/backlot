//! `backlot-model`: the studio ledger's event model. Pure data and
//! reductions over it — no I/O, no async, no clocks. The store stamps
//! `seq`/`ts_ms`; everything here is deterministic.
//!
//! - [`event`]: event types as in `docs/studio/ledger.md`.
//! - [`state`]: the reduced [`StudioState`], `apply` and `replay`.
//! - [`correct`]: which fields a `correction` event changes in the view.

pub mod correct;
pub mod event;
pub mod reduce;
pub mod state;

pub use event::{ActorIdentity, Event, EventBy, EventKind, NewEvent};
pub use state::{StudioState, TaskStatus, apply, replay};

#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod testkit;
