//! `backlot-stage`: the studio layer's durable side — the state
//! directory, the append-only ledger store, and the `backlot` studio
//! commands. The stage process itself lands in a later milestone.

pub mod store;

#[cfg(test)]
mod store_tests;
