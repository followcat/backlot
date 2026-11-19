//! Test-only helpers shared by the binary-level tests.
#![allow(dead_code)]

#[path = "../../src/test_support.rs"]
mod test_dir;

pub use test_dir::TestDir;
