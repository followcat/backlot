//! `backlot-stage`: the studio layer's durable side — the state
//! directory, the append-only ledger store, and the `backlot` studio
//! commands. The stage process itself lands in a later milestone.

pub mod args;
mod args_studio;
mod args_task;
pub mod cli;
pub mod commands;
pub mod render;
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
