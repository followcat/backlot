//! Document slots: a queued read is not an answer. `request_doc`
//! creates the slot before its fetch is dispatched, and a consumer that
//! reads `fetching: false` must be able to trust that the slot is
//! answered — U7's entry flow decides the whole screen on it.

use super::*;
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

fn key(name: &str) -> DocKey {
    DocKey {
        machine: None,
        name: name.into(),
    }
}

/// A request creates the slot — and a slot that is still queued must not
/// read as an answered one: no value, no error, but `fetching` set.
#[test]
fn a_requested_document_reads_as_fetching_before_its_fetch_is_dispatched() {
    let mut actor = actor();
    let key = key("backlot");
    actor.request_doc(key.clone());
    let doc = actor
        .docs
        .get(&key)
        .expect("the request creates the slot")
        .clone();
    assert!(doc.fetching, "a queued slot reads as fetching");
    assert!(
        doc.value.is_null() && doc.error.is_none() && doc.version == 0,
        "and carries no answer yet"
    );
    assert!(actor.doc_pending.contains(&key), "it is queued");
}

/// The state a publish can catch it in: this machine's one fetch slot is
/// already taken, so the request waits in the queue with its slot made.
#[test]
fn a_slot_queued_behind_the_per_machine_limit_reads_as_fetching() {
    let mut actor = actor();
    actor.doc_inflight.insert(None, DOC_PER_MACHINE);
    let key = key("backlot");
    actor.request_doc(key.clone());
    assert!(actor.doc_pending.contains(&key), "still queued");
    assert!(
        actor.docs[&key].fetching,
        "a slot waiting for a fetch slot is not an answer"
    );
}

/// A refresh of an already answered document keeps that answer readable:
/// the flag comes back only when the fetch is dispatched.
#[test]
fn a_repeat_request_keeps_the_last_answer_readable() {
    let mut actor = actor();
    let key = key("backlot");
    actor.request_doc(key.clone());
    {
        let slot = Arc::make_mut(Arc::make_mut(&mut actor.docs).get_mut(&key).unwrap());
        slot.fetching = false;
        slot.value = json!({ "studios": [] });
        slot.version = 4;
    }
    actor.request_doc(key.clone());
    let doc = actor.docs[&key].clone();
    assert!(!doc.fetching, "the last answer stays readable");
    assert_eq!(doc.version, 4);
    assert!(actor.doc_pending.contains(&key), "the refresh is queued");
}

/// A dead wire kills the fetches it was carrying. A slot with an answer
/// stops claiming the wire; one that never got an answer keeps the flag,
/// because the next reconnect still owes it a read.
#[test]
fn a_dead_wire_keeps_an_unanswered_slot_fetching() {
    let mut actor = actor();
    actor.request_doc(key("backlot"));
    actor.request_doc(key("hc-tui"));
    {
        let slot = Arc::make_mut(
            Arc::make_mut(&mut actor.docs)
                .get_mut(&key("hc-tui"))
                .unwrap(),
        );
        slot.value = json!({ "terminals": [] });
        slot.version = 3;
    }
    actor.clear_aborted_fetches();
    assert!(
        actor.docs[&key("backlot")].fetching,
        "never answered: still on the wire"
    );
    assert!(
        !actor.docs[&key("hc-tui")].fetching,
        "answered: nothing on the wire"
    );
}
