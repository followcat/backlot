//! `terminal.list`'s `machines`: an empty list is an answer.
//!
//! Core 0.1.21 sends `machines` always (`[]` without machines), so the
//! static fallback records that answer instead of leaving the machine
//! facts unknown. Older cores omit the member when empty; their `[]`
//! reads the same, which is also the truth for them.

use super::*;
use crate::terminal::ListResult;
use serde_json::json;

fn actor() -> Actor {
    let (version, _) = watch::channel(0);
    let (_, commands) = mpsc::channel(1);
    let (_, stop) = watch::channel(false);
    Actor::new(
        WatchParams::default(),
        Arc::new(Mutex::new(View::default())),
        version,
        commands,
        stop,
        Arc::new(|| Box::pin(async { panic!("no RPC transport is opened") })),
        Arc::new(|_| Box::pin(async { panic!("no subscription transport is opened") })),
    )
}

fn list(machines: serde_json::Value) -> ListResult {
    serde_json::from_value(json!({
        "terminals": [],
        "hidden_finished": 0,
        "machines": machines,
    }))
    .unwrap()
}

#[test]
fn an_empty_machines_list_is_the_answer_not_missing_data() {
    let mut actor = actor();
    assert!(actor.machines.is_none(), "unknown before any list");
    actor.static_done(Ok((0, Ok(list(json!([]))))));
    assert_eq!(
        actor.machines.as_deref(),
        Some(&[][..]),
        "an empty list means this core has no machines"
    );
}

#[test]
fn an_older_core_that_omits_machines_reads_the_same() {
    let mut actor = actor();
    let omitted: ListResult = serde_json::from_value(json!({
        "terminals": [],
        "hidden_finished": 0,
    }))
    .unwrap();
    actor.static_done(Ok((0, Ok(omitted))));
    assert_eq!(actor.machines.as_deref(), Some(&[][..]));
}
