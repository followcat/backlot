//! The new-studio wizard's repository picker (U5): a path input, a
//! live filter and a directory tree, drawn full-screen inside the
//! popup when the wizard's stdio is a terminal. The state machine
//! never touches a terminal or a filesystem — keys go in, rows come
//! out, and every directory or git question goes through [`Seams`],
//! so tests drive it over fake trees.

mod filter;
mod state;
mod tree;

#[cfg(test)]
mod filter_tests;
#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod tree_tests;
