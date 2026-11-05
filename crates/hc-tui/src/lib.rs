//! `hc-tui`: a read-only terminal client for `herdr-core` (`hc`).
//!
//! The binary parses `--session`/`--remote`, resolves the endpoint and
//! runs the current-thread application: a reconnecting watcher feeds
//! the sidebar, one attach at a time paints the displayed terminal
//! through `hc-vt`, and the Ctrl+B prefix drives navigation. Input,
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
pub mod sidebar;
pub mod term;
pub mod terminal_modes;
#[cfg(test)]
mod testcore;

mod mouse_geometry;

mod ansi;

mod keybind_help;

pub mod layout;

pub mod structure;
