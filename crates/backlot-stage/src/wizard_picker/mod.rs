//! The new-studio wizard's repository picker (U5): a path input, a
//! live filter and a directory tree, drawn full-screen inside the
//! popup when the wizard's stdio is a terminal. The state machine
//! never touches a terminal or a filesystem — keys go in, rows come
//! out, and every directory or git question goes through [`Seams`],
//! so tests drive it over fake trees.
//!
//! - `filter` — the `~` input, the root/filter split, match ranking;
//! - `state` — the [`Picker`]: keys in, [`Outcome`] out;
//! - `tree` — the rows and the [`Seams`] the world answers through;
//! - `render` — the `Buffer` drawing, terminal-free;
//! - `fs`/`term` — the live seams and the crossterm loop.

mod filter;
pub(crate) mod fs;
pub(crate) mod in_use;
mod render;
mod state;
pub(crate) mod term;
mod tree;

pub(crate) use term::pick;

#[cfg(test)]
mod filter_tests;
#[cfg(test)]
mod fs_tests;
#[cfg(test)]
mod in_use_tests;
#[cfg(test)]
mod render_tests;
#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod term_tests;
#[cfg(test)]
mod tree_tests;
