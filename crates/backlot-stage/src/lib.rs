//! `backlot-stage`: the studio layer's durable side — the state
//! directory, the append-only ledger store, the `backlot` studio
//! commands and the stage process that schedules role terminals.

mod act;
pub mod args;
mod args_hwood;
mod args_source;
mod args_studio;
mod args_task;
mod args_usage;
mod checkrun;
mod checks;
mod checkwork;
pub mod cli;
mod cli_run;
pub mod commands;
pub mod config;
mod connerr;
mod daemon;
mod deliver;
mod digest;
mod discover;
mod doc;
mod draft;
mod exchange_read;
mod gitcmd;
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
mod hwood_show;
mod limits;
pub mod link;
mod migrate;
mod notices;
mod nudge;
mod paths;
#[cfg(test)]
mod paths_tests;
mod permission;
mod permit;
mod prompts;
mod prune;
mod prune_paths;
mod prune_render;
mod range;
mod ratelimit;
mod reasons;
pub mod reconcile;
mod relabel;
pub mod render;
mod repos;
mod runners;
mod sample;
mod sendfail;
mod service;
pub mod signals;
mod stage;
mod stagelog;
mod stale;
mod stall;
pub mod store;
mod studio_list;
mod supervise;
mod watch;
mod watchdog;
mod wizard;
mod wizard_cast;
mod wizard_paths;
mod wizard_picker;
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
mod cli_list_tests;
#[cfg(test)]
mod cli_mode_tests;
#[cfg(test)]
mod cli_session_tests;
#[cfg(test)]
mod cli_state_tests;
#[cfg(test)]
mod cli_suggest_tests;
#[cfg(test)]
mod cli_testkit;
#[cfg(test)]
mod cli_tests;
#[cfg(test)]
mod daemon_tests;
#[cfg(test)]
mod deliver_refuse_tests;
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
mod exchange_read_tests;
#[cfg(test)]
mod finish_sweep_tests;
#[cfg(test)]
mod gitcmd_tests;

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
mod hwatch_tests;
#[cfg(test)]
mod hwood_read_tests;
#[cfg(test)]
mod hwood_show_tests;
#[cfg(test)]
mod hwood_tests;
#[cfg(test)]
mod limits_cmd_tests;
#[cfg(test)]
mod loop_cmd_tests;
#[cfg(test)]
mod migrate_tests;
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
mod prune_cli_tests;
#[cfg(test)]
mod prune_tests;
#[cfg(test)]
mod range_cmd_tests;
#[cfg(test)]
mod range_tests;
#[cfg(test)]
mod render_tests;
#[cfg(test)]
mod reopen_cmd_tests;
#[cfg(test)]
mod repos_tests;
#[cfg(test)]
mod role_cmd_tests;
#[cfg(test)]
mod sample_tests;

#[cfg(test)]
mod signals_tests;
#[cfg(test)]
mod stagelog_tests;
#[cfg(test)]
mod stale_tests;
#[cfg(test)]
mod stall_tests;
#[cfg(test)]
mod store_tests;
#[cfg(test)]
mod trust_tests;

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod text_src_tests;
#[cfg(test)]
mod usage_tests;
#[cfg(test)]
mod watch_tests;
#[cfg(test)]
mod wizard_paths_tests;
#[cfg(test)]
mod wizard_tests;
#[cfg(test)]
mod workareas_tests;

pub use cli::{Env, Fail, maybe_run};
pub use commands::validate_cast_file;
pub use runners::detect;
