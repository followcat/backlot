//! `hc-tui`: a terminal client with shared layout for `herdr-core` (`hc`).
//!
//! The binary parses `--session`/`--remote`, resolves the endpoint and
//! runs the current-thread application: a reconnecting watcher feeds
//! the sidebar and shared layout, persistent VTs paint every visible pane
//! through `hc-vt`, and configured bindings drive navigation. Input,
//! geometry and lifecycle code is separated from I/O for tests.

pub mod app;
pub mod chrome;
pub mod cli;
pub mod config;
pub mod host;
pub mod input;
pub mod project;

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
mod testcore;
mod word_select;

mod mouse_geometry;

mod ansi;

mod keybind_help;

pub mod layout;

pub mod operations;
mod panel;
pub mod popup_size;
pub mod structure;
