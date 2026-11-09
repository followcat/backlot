//! `backlot-stage`: the studio layer's durable side — the state
//! directory, the append-only ledger store, the `backlot` studio
//! commands and the stage process that schedules role terminals.

pub mod args;
mod args_studio;
mod args_task;
pub mod cli;
pub mod commands;
mod daemon;
pub mod link;
pub mod reconcile;
pub mod render;
mod runners;
mod stage;
pub mod store;

#[cfg(test)]
mod cli_state_tests;
#[cfg(test)]
mod cli_testkit;
#[cfg(test)]
mod cli_tests;
#[cfg(test)]
mod store_tests;

pub use cli::{Env, Fail, maybe_run};
