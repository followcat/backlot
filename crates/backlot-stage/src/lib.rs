//! `backlot-stage`: the studio layer's durable side — the state
//! directory, the append-only ledger store, the `backlot` studio
//! commands and the stage process that schedules role terminals.

mod accounts;
mod accounts_store;
mod accounts_summary;
mod act;
pub mod args;
mod args_card;
mod args_decide;
mod args_design;
mod args_hwood;
mod args_source;
mod args_studio;
mod args_task;
mod args_usage;
mod ask_answer;
pub(crate) mod catalog_doc;
mod checkrun;
mod checks;
mod checkwork;
pub mod cli;
mod cli_run;
pub mod commands;
pub mod config;
mod confirm;
mod connerr;
mod daemon;
mod decision_doc;
mod delete;
mod delete_ops;
mod delete_render;
mod deliver;
mod design_capture;
mod design_doc;
mod design_notices;
mod design_watch;
mod digest;
mod discover;
mod doc;
pub mod doc_client;
pub(crate) mod doc_dedupe;
mod doc_dispatch;
mod doc_dispatch_studio;
pub(crate) mod doc_req;
mod draft;
mod driver;
mod exchange_read;
mod gitcmd;
mod guard;
mod guard_git;
mod guard_git_env;
mod guard_git_parse;
mod guard_git_probe;
mod guard_git_push;
mod guard_git_push_opts;
mod guard_prep;
mod guard_probes;
mod guard_rules;
mod guard_shell;
mod guard_shell_lex;
mod guide;
mod handoff;
mod handoff_commit;
mod handoff_pkg;
mod handoff_run;
mod handoff_term;
mod handoff_verify;
mod hcard;
mod hcard_cast;
mod hcard_new;
mod hcard_outward;
mod hcard_outward_env;
mod hcard_outward_exec;
mod hcard_outward_git;
mod hcard_snapshot;
mod hdeliver;
mod hdeliver_fmt;
mod hfinished;
mod hlife;
mod hnotice;
mod hnotice_stale;
pub mod hook;
mod hrender;
mod hrender_accounts;
mod hsetup;
mod hstage;
mod hswitch_verbs;
mod hwatch;
mod hwcli;
mod hwood;
mod hwood_answers;
mod hwood_base;
mod hwood_read;
mod hwood_show;
mod limits;
pub mod link;
mod live;
mod liveness;
mod liveness_diff;
mod liveness_probe;
mod liveness_work;
mod metrics_cli;
mod metrics_digest_render;
mod metrics_render;
mod migrate;
mod msg_show;
mod msg_show_fmt;
mod notices;
mod nudge;
mod paths;
#[cfg(test)]
mod paths_tests;
mod permission;
mod permit;
mod proc_guard;
#[cfg(test)]
#[cfg(target_os = "linux")]
mod proc_guard_detach_tests;
#[cfg(test)]
mod proc_guard_tests;
mod prompts;
mod prune;
mod prune_paths;
mod prune_render;
mod prune_report;
mod question;
mod queue_status;
mod range;
mod ratelimit;
mod reasons;
pub mod reconcile;
mod relabel;
pub mod render;
mod repo_note;
mod repos;
mod restore;
mod route;
mod runners;
mod sample;
mod sendfail;
mod server_adopt;
mod server_adopt_fs;
mod server_lock;
mod server_mode;
mod server_service;
mod service;
pub mod signals;
mod signin;
mod stage;
mod stage_accounts;
mod stagelog;
mod stale;
mod stall;
pub mod store;
mod studio_cast;
mod studio_fmt;
mod studio_list;
mod supervise;
mod tell_guard;
mod transcript;
mod transcript_ext;
mod transcript_runners;
mod verbs;
mod verbs_card_show;
mod verbs_decide;
mod verbs_design;
mod verbs_fmt;
mod verbs_hwood;
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
mod ask_answer_guards_tests;
#[cfg(test)]
mod ask_answer_tests;
#[cfg(test)]
mod ask_tests;
#[cfg(test)]
mod budget_cmd_tests;
#[cfg(test)]
mod cards_hardening_steps_tests;
#[cfg(test)]
mod cards_hardening_tests;
#[cfg(test)]
mod cast_card_tests;
#[cfg(test)]
mod cast_effect_tests;
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
mod closed_actor_mail_tests;
#[cfg(test)]
mod daemon_tests;
#[cfg(test)]
mod decide_tests;
#[cfg(test)]
mod delete_tests;
#[cfg(test)]
mod deliver_refuse_tests;
#[cfg(test)]
mod deliver_tests;
#[cfg(test)]
mod dep_cmd_tests;
#[cfg(test)]
mod design_capture_gate_tests;
#[cfg(test)]
mod design_capture_hook_tests;
#[cfg(test)]
mod design_capture_tests;
#[cfg(test)]
#[cfg(test)]
mod design_cli_tests;
#[cfg(test)]
mod design_doc_tests;
#[cfg(test)]
#[cfg(test)]
mod design_gate_nudge_tests;
#[cfg(test)]
mod design_gate_stage_tests;
#[cfg(test)]
mod design_remote_tests;
#[cfg(test)]
mod digest_tests;
#[cfg(test)]
mod discover_tests;
#[cfg(test)]
mod doc_req_cap_tests;
#[cfg(test)]
mod doc_req_card_tests;
#[cfg(test)]
mod doc_req_cast_tests;
#[cfg(test)]
mod doc_req_fake_core;
#[cfg(test)]
mod doc_req_studio_json_tests;
#[cfg(test)]
mod doc_req_studio_seed;
#[cfg(test)]
mod doc_req_studio_tests;
#[cfg(test)]
mod doc_req_tests;
#[cfg(test)]
mod doc_req_verbs_seed;
#[cfg(test)]
mod doc_req_verbs_tests;
#[cfg(test)]
mod docs_tests;
#[cfg(test)]
mod draft_tests;
#[cfg(test)]
mod driver_record_tests;
#[cfg(test)]
mod exchange_read_tests;
#[cfg(test)]
mod facts_tests;
#[cfg(test)]
mod finish_guard_tests;
#[cfg(test)]
mod finish_sweep_tests;
#[cfg(test)]
mod gitcmd_tests;
#[cfg(test)]
mod guard_git_review1_tests;
#[cfg(test)]
mod guard_git_tests;
#[cfg(test)]
mod guard_line_tests;
#[cfg(test)]
mod guard_tests;
#[cfg(test)]
mod guide_tests;

#[cfg(test)]
mod hcard_tests;
#[cfg(test)]
mod hdeliver_extra_tests;
#[cfg(test)]
mod hdeliver_passive_tests;
#[cfg(test)]
mod hdeliver_tests;
#[cfg(test)]
mod hnotice_stale_tests;
#[cfg(test)]
mod hnotice_tests;
#[cfg(test)]
mod hook_agy_tests;
#[cfg(test)]
mod hook_tests;
#[cfg(test)]
mod hsetup_tests;
#[cfg(test)]
mod hstage_tests;
#[cfg(test)]
mod hwatch_tests;
#[cfg(test)]
mod hwood_read_design_tests;
#[cfg(test)]
mod hwood_read_tests;
#[cfg(test)]
mod hwood_show_tests;
#[cfg(test)]
mod hwood_tests;
#[cfg(test)]
mod limits_cmd_tests;
#[cfg(test)]
mod live_tests;
#[cfg(test)]
mod liveness_probe_tests;
#[cfg(test)]
mod liveness_tests;
#[cfg(test)]
mod loop_cmd_tests;
#[cfg(test)]
mod metrics_cli_tests;
#[cfg(test)]
mod metrics_digest_cli_tests;
#[cfg(test)]
mod migrate_tests;
#[cfg(test)]
mod misreads_tests;
#[cfg(test)]
mod msg_show_extra_tests;
#[cfg(test)]
mod msg_show_tests;
#[cfg(test)]
mod notices_tests;
#[cfg(test)]
mod nudge_tests;
#[cfg(test)]
mod outward_card_tests;
#[cfg(test)]
mod outward_exec_tests;
#[cfg(test)]
mod permit_cmd_tests;
#[cfg(test)]
mod permit_tests;
#[cfg(test)]
mod priority_cmd_tests;
#[cfg(test)]
mod progress_cmd_tests;
#[cfg(test)]
mod prompt_fixtures;
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
mod readonly_audit_tests;
#[cfg(test)]
mod recast_cmd_tests;
#[cfg(test)]
mod recast_standby_tests;
#[cfg(test)]
mod refused_noise_tests;
#[cfg(test)]
mod render_tests;
#[cfg(test)]
mod reopen_cmd_tests;
#[cfg(test)]
mod repo_note_tests;
#[cfg(test)]
mod repos_tests;
#[cfg(test)]
mod restart_cmd_tests;
#[cfg(test)]
mod restore_tests;
#[cfg(test)]
mod role_cmd_tests;
#[cfg(test)]
mod route_tests;
#[cfg(test)]
mod sample_tests;

#[cfg(test)]
mod server_adopt_tests;

#[cfg(test)]
mod server_fake_core;

#[cfg(test)]
mod server_mode_tests;

#[cfg(test)]
mod server_service_tests;

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
mod studio_cast_tests;
#[cfg(test)]
mod tell_guard_tests;
#[cfg(test)]
mod trust_tests;

#[cfg(test)]
mod handoff_extra_tests;
#[cfg(test)]
mod handoff_pkg_tests;
#[cfg(test)]
mod handoff_tests;
#[cfg(test)]
mod hcard_setup_tests;
#[cfg(test)]
mod hswitch_cli_tests;
#[cfg(test)]
mod hwood_setup_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod text_src_tests;
#[cfg(test)]
mod transcript_tests;
#[cfg(test)]
mod unknown_stage_tests;
#[cfg(test)]
mod unknown_tests;
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

pub mod remote_doc_cli;
#[cfg(test)]
mod remote_doc_cli_tests;
#[cfg(test)]
mod remote_doc_studio_tests;
pub mod server_cfg;
#[cfg(test)]
mod server_cfg_tests;

pub use cli::{Env, Fail, maybe_run};
pub use commands::validate_cast_file;
pub use delete::DeleteReport;
pub use runners::detect;
