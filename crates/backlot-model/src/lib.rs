//! `backlot-model`: the studio ledger's event model. Pure data and
//! reductions over it — no I/O, no async, no clocks. The store stamps
//! `seq`/`ts_ms`; everything here is deterministic.
//!
//! - [`event`]: event types as in `docs/studio/ledger.md`.
//! - [`state`]: the reduced [`StudioState`], `apply` and `replay`.
//! - [`validate`]: append-time rules; refused events get a `Refusal`.
//! - [`correct`]: which fields a `correction` event changes in the view.
//! - [`records`]: per-actor records across studios (D4, D8).
//! - [`casting`]: candidates joined against those records for one
//!   task's kind — factual history only, never a score (L3, D8).
//! - [`steps`]: the next steps each role owes, derived from state (S9a).
//! - [`hollywood`]: the overseer's own ledger — cards, headlines,
//!   messages — its own kinds so no studio list or rule sees it.

pub mod attention;
pub mod budget;
pub mod caps;
pub mod cast;
mod cast_diff;
mod cast_file;
pub mod casting;
pub mod correct;
pub mod event;
mod event_unknown;
pub mod hollywood;
mod hollywood_snapshot;
mod hollywood_state;
mod hollywood_summary;
pub mod hollywood_switch;
mod hollywood_validate;
pub mod limits;
pub mod progress;
pub mod push_policy;
pub mod recast;
pub mod records;
pub mod reduce;
pub mod refusal;
pub mod req_catalog;
pub mod req_results;
pub mod req_wire;
pub mod roles;
pub mod state;
pub mod steps;
mod studio_exchange;
pub mod summary;
mod summary_build;
pub mod task_reduce;
pub mod usage;
pub mod validate;
pub mod wanted;

pub use event::{ActorIdentity, Event, EventBy, EventKind, NewEvent, Usage, UsageTotals};
pub use records::{ActorRecord, StudioLog, records};
pub use state::{StudioState, TaskStatus, apply, replay};
pub use validate::{Refusal, validate};

/// Pure event/state builders shared by this crate's tests and other
/// crates' unit tests. Not part of the ledger surface.
#[doc(hidden)]
pub mod testkit;

/// The ledger schema version and forward-migration pipeline.
pub mod migrate;

#[cfg(test)]
mod budget_tests;
#[cfg(test)]
mod cap_tests;
#[cfg(test)]
mod cards_hardening_tests;
#[cfg(test)]
mod cast_tests;
#[cfg(test)]
mod casting_tests;
#[cfg(test)]
mod checks_tests;
#[cfg(test)]
mod hollywood_card_tests;
#[cfg(test)]
mod hollywood_setup_tests;
#[cfg(test)]
mod hollywood_summary_tests;
#[cfg(test)]
mod hollywood_switch_tests;
#[cfg(test)]
mod hollywood_tests;
#[cfg(test)]
mod hollywood_validate_tests;
#[cfg(test)]
mod limits_tests;
#[cfg(test)]
mod loop_tests;
#[cfg(test)]
mod migrate_tests;
#[cfg(test)]
mod priority_tests;
#[cfg(test)]
mod progress_tests;
#[cfg(test)]
mod recast_tests;
#[cfg(test)]
mod records_extra_tests;
#[cfg(test)]
mod records_tests;
#[cfg(test)]
mod reopen_tests;
#[cfg(test)]
mod req_wire_tests;
#[cfg(test)]
mod restart_tests;
#[cfg(test)]
mod roles_tests;
#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod steps_tests;
#[cfg(test)]
mod studio_exchange_tests;
#[cfg(test)]
mod summary_standby_tests;
#[cfg(test)]
mod summary_tests;
#[cfg(test)]
mod unknown_tests;
#[cfg(test)]
mod usage_tests;
#[cfg(test)]
mod validate_delete_tests;
#[cfg(test)]
mod validate_extra_tests;
#[cfg(test)]
mod validate_tests;
