//! Item 7: the desktop ping a studio raises when it starts waiting on the
//! person. A new question pings the outer terminal once, a restart with the
//! same ask open does not, a card pings, an acknowledged ask does not, and
//! `[studio] notify_asks = false` silences it. Every case installs a
//! terminal backend and captures the sink's bytes. Neutral fixtures only
//! (`acme-shop`, `alice`, `host-a`).

use super::m4s2::captured;
use super::studio_notify::{
    app_with, card, doc, hollywood, publish, roles, roles_with_ask, seeded_doc,
};
use super::*;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

/// The OSC 9 sequences the sink received, as UTF-8 — one per ping.
fn pings(captured: &Arc<Mutex<Vec<Vec<u8>>>>) -> Vec<String> {
    captured
        .lock()
        .unwrap()
        .iter()
        .filter(|bytes| bytes.starts_with(b"\x1b]9;"))
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .collect()
}

/// Studio mode with a Ghostty notification backend, a fresh capture, and
/// the given document seeded silently.
fn ping_app(value: Value) -> (App, Arc<Mutex<Vec<Vec<u8>>>>) {
    let mut app = app_with(value);
    app.notify_backend = Some(crate::terminal_notify::TerminalNotificationBackend::Ghostty);
    let (captured, sink) = captured();
    app.set_host_control(sink);
    (app, captured)
}

#[tokio::test]
async fn a_new_ask_pings_once_and_a_republish_does_not() {
    let (mut app, captured) = ping_app(seeded_doc());
    assert!(pings(&captured).is_empty(), "the seed must not ping");
    publish(
        &mut app,
        doc(
            json!({
                "roles": roles_with_ask("which colour?"),
                "exchange": [{ "seq": 6, "who": "producer", "text": "which colour?" }],
            }),
            hollywood("all quiet", 12, card("c3", "pause t1")),
        ),
    );
    let fired = pings(&captured);
    assert_eq!(fired.len(), 1, "one ping per new ask: {fired:?}");
    assert!(
        fired[0].contains("acme-shop · producer asks: which colour?"),
        "the ping names the studio and the question: {fired:?}"
    );
    // The identical document repeats nothing.
    publish(
        &mut app,
        doc(
            json!({
                "roles": roles_with_ask("which colour?"),
                "exchange": [{ "seq": 6, "who": "producer", "text": "which colour?" }],
            }),
            hollywood("all quiet", 12, card("c3", "pause t1")),
        ),
    );
    assert_eq!(pings(&captured).len(), 1, "a republish must not ping");
}

#[tokio::test]
async fn a_restart_with_the_ask_open_does_not_ping() {
    // The same document is the seed on start: what predates the client
    // is not news, so no ping fires.
    let open = doc(
        json!({
            "roles": roles_with_ask("which colour?"),
            "exchange": [{ "seq": 6, "who": "producer", "text": "which colour?" }],
        }),
        hollywood("all quiet", 12, card("c3", "pause t1")),
    );
    let (_app, captured) = ping_app(open);
    assert!(
        pings(&captured).is_empty(),
        "an open ask at start is not news"
    );
}

#[tokio::test]
async fn a_card_pings() {
    let (mut app, captured) = ping_app(seeded_doc());
    publish(
        &mut app,
        doc(
            json!({ "roles": roles() }),
            hollywood("all quiet", 12, card("c4", "resume t2")),
        ),
    );
    let fired = pings(&captured);
    assert_eq!(fired.len(), 1, "a card pings: {fired:?}");
    assert!(
        fired[0].contains("acme-shop · card c4: resume t2"),
        "the ping names the card and its action: {fired:?}"
    );
}

#[tokio::test]
async fn an_acknowledged_ask_does_not_ping() {
    // The ask opens (one ping), then is answered — the role no longer
    // asks, so nothing new pings on the answering publish.
    let (mut app, captured) = ping_app(seeded_doc());
    publish(
        &mut app,
        doc(
            json!({
                "roles": roles_with_ask("which colour?"),
                "exchange": [{ "seq": 6, "who": "producer", "text": "which colour?" }],
            }),
            hollywood("all quiet", 12, card("c3", "pause t1")),
        ),
    );
    assert_eq!(pings(&captured).len(), 1, "the ask pinged once");
    publish(
        &mut app,
        doc(
            json!({
                "roles": roles(),
                "exchange": [{ "seq": 6, "who": "producer", "text": "which colour?" }],
            }),
            hollywood("all quiet", 12, card("c3", "pause t1")),
        ),
    );
    assert_eq!(
        pings(&captured).len(),
        1,
        "answering an ask never pings: {:?}",
        pings(&captured)
    );
}

#[tokio::test]
async fn the_setting_off_means_no_ping() {
    let (mut app, captured) = ping_app(seeded_doc());
    app.shell.studio_notify_asks = false;
    publish(
        &mut app,
        doc(
            json!({
                "roles": roles_with_ask("which colour?"),
                "exchange": [{ "seq": 6, "who": "producer", "text": "which colour?" }],
            }),
            hollywood("all quiet", 12, card("c3", "pause t1")),
        ),
    );
    assert!(
        pings(&captured).is_empty(),
        "setting off: {:?}",
        pings(&captured)
    );
}
