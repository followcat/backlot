//! `hc-tui`: a terminal client with shared layout for `herdr-core` (`hc`).
//!
//! The binary parses `--session`/`--remote`, resolves the endpoint and
//! runs the current-thread application: a reconnecting watcher feeds
//! the sidebar and shared layout, persistent VTs paint every visible pane
//! through `hc-vt`, and configured bindings drive navigation. Input,
//! geometry and lifecycle code is separated from I/O for tests.

pub mod app;
mod cast;
pub mod chrome;
pub mod cli;
mod clipboard;
mod clipdrop;
pub mod config;
pub(crate) mod copy_buffer;
pub mod copy_mode;
pub(crate) mod copy_window;
pub mod core_start;
pub mod diag;
pub mod host;
pub mod input;
mod live_exe;
pub mod preferences;
pub mod project;
pub mod restart;

pub(crate) mod install_source;
pub mod raw_events;
pub mod raw_frames;
pub mod raw_input;
#[cfg(unix)]
pub mod reader;
pub mod selection;
pub mod sidebar;
pub mod term;
pub mod terminal_modes;
pub mod terminal_notify;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod testcore;
pub mod update_check;
pub(crate) mod update_install;
pub(crate) mod update_markdown;
pub(crate) mod update_notes;
pub(crate) mod update_run;
pub mod update_state;
pub(crate) mod waiting;
mod word_select;

mod mouse_geometry;

mod action;
mod ansi;

mod help_launcher;
mod keybind_help;

pub mod layout;

mod hollywood;
mod hw_setup;
mod machine_install;
mod machine_versions;
mod new_studio;
mod opening;
pub mod operations;
pub mod own_theme;
mod panel;
pub mod popup_size;
mod sheet;
mod shells;
pub mod structure;
mod studio;
mod studio_cmds;
mod studio_dialog;
mod studio_index;
mod studio_menu;
