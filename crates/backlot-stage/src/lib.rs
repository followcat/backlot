//! `backlot-stage`: the studio layer's durable side — the state
//! directory, the append-only ledger store, the `backlot` studio
//! commands and the stage process that schedules role terminals.

mod act;
pub mod args;
mod args_hwood;
mod args_studio;
mod args_task;
mod checkrun;
mod checks;
mod checkwork;
pub mod cli;
pub mod commands;
pub mod config;
mod daemon;
mod deliver;
mod digest;
mod discover;
mod doc;
mod draft;
mod hcard;
mod hcard_new;
mod hdeliver;
mod hlife;
mod hnotice;
pub mod hook;
mod hrender;
mod hsetup;
mod hstage;
mod hwatch;
mod hwood;
mod hwood_base;
mod hwood_read;
mod limits;
pub mod link;
mod notices;
mod nudge;
mod permission;
mod permit;
mod prompts;
mod range;
mod ratelimit;
mod reasons;
pub mod reconcile;
mod relabel;
pub mod render;
mod runners;
mod sample;
pub mod signals;
mod stage;
mod stale;
mod stall;
pub mod store;
mod watch;
mod watchdog;
mod workareas;

#[cfg(test)]
mod act_tests;
#[cfg(test)]
mod budget_cmd_tests;
#[cfg(test)]
mod checkrun_tests;
#[cfg(test)]
mod checks_tests;
#[cfg(test)]
mod checkwork_tests;
#[cfg(test)]
mod cli_state_tests;
#[cfg(test)]
mod cli_testkit;
#[cfg(test)]
mod cli_tests;
#[cfg(test)]
mod daemon_tests;
#[cfg(test)]
mod deliver_tests;
#[cfg(test)]
mod dep_cmd_tests;
#[cfg(test)]
mod digest_tests;
#[cfg(test)]
mod discover_tests;
#[cfg(test)]
mod docs_tests;
#[cfg(test)]
mod draft_tests;

#[cfg(test)]
mod hcard_tests;
#[cfg(test)]
mod hnotice_tests;
#[cfg(test)]
mod hook_tests;
#[cfg(test)]
mod hsetup_tests;
#[cfg(test)]
mod hstage_tests;
#[cfg(test)]
mod hwood_read_tests;
#[cfg(test)]
mod hwood_tests;
#[cfg(test)]
mod limits_cmd_tests;
#[cfg(test)]
mod loop_cmd_tests;
#[cfg(test)]
mod notices_tests;
#[cfg(test)]
mod nudge_tests;
#[cfg(test)]
mod permit_cmd_tests;
#[cfg(test)]
mod permit_tests;
#[cfg(test)]
mod priority_cmd_tests;
#[cfg(test)]
mod prompts_tests;
#[cfg(test)]
mod range_cmd_tests;
#[cfg(test)]
mod range_tests;
#[cfg(test)]
mod render_tests;
#[cfg(test)]
mod reopen_cmd_tests;
#[cfg(test)]
mod role_cmd_tests;
#[cfg(test)]
mod sample_tests;

#[cfg(test)]
mod signals_tests;
#[cfg(test)]
mod stale_tests;
#[cfg(test)]
mod stall_tests;
#[cfg(test)]
mod store_tests;
#[cfg(test)]
mod watch_tests;
#[cfg(test)]
mod workareas_tests;

pub use cli::{Env, Fail, maybe_run};
