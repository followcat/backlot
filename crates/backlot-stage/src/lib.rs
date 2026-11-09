//! `backlot-stage`: the studio layer's durable side — the state
//! directory, the append-only ledger store, the `backlot` studio
//! commands and the stage process that schedules role terminals.

mod act;
pub mod args;
mod args_studio;
mod args_task;
pub mod cli;
pub mod commands;
mod config;
mod daemon;
mod discover;
mod doc;
pub mod link;
pub mod reconcile;
pub mod render;
mod runners;
mod stage;
pub mod store;
mod watch;
mod workareas;

#[cfg(test)]
mod act_tests;
#[cfg(test)]
mod cli_state_tests;
#[cfg(test)]
mod cli_testkit;
#[cfg(test)]
mod cli_tests;
#[cfg(test)]
mod discover_tests;
#[cfg(test)]
mod priority_cmd_tests;
#[cfg(test)]
mod reopen_cmd_tests;
#[cfg(test)]
mod store_tests;
#[cfg(test)]
mod watch_tests;
#[cfg(test)]
mod workareas_tests;

pub use cli::{Env, Fail, maybe_run};
