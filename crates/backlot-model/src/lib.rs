//! `backlot-model`: the studio ledger's event model. Pure data and
//! reductions over it — no I/O, no async, no clocks. The store stamps
//! `seq`/`ts_ms`; everything here is deterministic.
//!
//! - [`event`]: event types as in `docs/studio/ledger.md`.
//! - [`state`]: the reduced [`StudioState`], `apply` and `replay`.
//! - [`validate`]: append-time rules; refused events get a `Refusal`.
//! - [`correct`]: which fields a `correction` event changes in the view.
//! - [`records`]: per-actor records across studios (D4, D8).

pub mod correct;
pub mod event;
pub mod records;
pub mod reduce;
pub mod refusal;
pub mod state;
pub mod summary;
pub mod task_reduce;
pub mod validate;

pub use event::{ActorIdentity, Event, EventBy, EventKind, NewEvent};
pub use records::{ActorRecord, StudioLog, records};
pub use state::{StudioState, TaskStatus, apply, replay};
pub use validate::{Refusal, validate};

/// Pure event/state builders shared by this crate's tests and other
/// crates' unit tests. Not part of the ledger surface.
#[doc(hidden)]
pub mod testkit;

#[cfg(test)]
mod cap_tests;
#[cfg(test)]
mod priority_tests;
#[cfg(test)]
mod records_extra_tests;
#[cfg(test)]
mod records_tests;
#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod summary_tests;
#[cfg(test)]
mod validate_extra_tests;
#[cfg(test)]
mod validate_tests;
