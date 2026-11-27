use super::*;
use crate::hollywood::Conn;

/// The dedup key's own join — tests spell keys part by part so no
/// literal ever needs a `\0` escape.
fn key(parts: &[&str]) -> String {
    parts.join("\0")
}

fn up(_: Option<&str>) -> (Conn, bool) {
    (Conn::Online, true)
}

/// Stage up, or a stopped stage — a plain fn item, so the
/// higher-ranked `Status` bound accepts it.
fn stopped_stage(_: Option<&str>) -> (Conn, bool) {
    (Conn::Online, false)
}

fn unreachable(_: Option<&str>) -> (Conn, bool) {
    (Conn::Offline, false)
}

fn summary(value: serde_json::Value) -> MachineSummary {
    MachineSummary {
        machine: None,
        summary: serde_json::from_value(value).unwrap(),
    }
}

/// A document with one `acme-shop` studio: `roles` and `studio` go
/// on the studio, `doc` on the document itself (Hollywood lives at
/// the root, not on a studio).
fn doc_json(
    roles: serde_json::Value,
    studio: serde_json::Value,
    doc: serde_json::Value,
) -> serde_json::Value {
    let mut entry = serde_json::json!({
        "id": "acme-shop", "title": "Acme shop", "state": "live",
        "requirement_version": 1, "mode": "standard",
        "open_reviews": 0, "waiting": 0, "roles": roles,
    });
    for (key, value) in studio.as_object().unwrap() {
        entry[key] = value.clone();
    }
    let mut out = serde_json::json!({
        "schema": "backlot/1", "updated_ms": 1, "studios": [entry],
    });
    for (key, value) in doc.as_object().unwrap() {
        out[key] = value.clone();
    }
    out
}

fn studio_json(roles: serde_json::Value, studio: serde_json::Value) -> serde_json::Value {
    doc_json(roles, studio, serde_json::json!({}))
}

#[test]
fn an_ask_keys_on_its_ledger_seq_and_names_the_question() {
    let machines = [summary(studio_json(
        serde_json::json!([{ "role": "producer", "name": "producer",
            "state": "live", "asks": "which shop?" }]),
        serde_json::json!({ "exchange": [
            { "seq": 4, "who": "producer", "text": "which shop?" }
        ]}),
    ))];
    let events = derive(&machines, &up);
    let asks = events
        .iter()
        .find(|e| e.kind == StudioEventKind::Asks)
        .expect("the ask");
    assert_eq!(asks.title, "producer asks: which shop?");
    assert_eq!(asks.key, key(&["asks", "", "acme-shop", "producer", "4"]));
}

#[test]
fn a_user_prompt_and_a_needs_you_attention_read_differently() {
    let machines = [summary(studio_json(
        serde_json::json!([
            { "role": "producer", "name": "producer", "state": "live",
              "attention": "permission prompt — needs the user",
              "attention_user": true, "attention_seq": 7,
              "attention_detail": "run npm test" },
            { "role": "writer", "name": "writer", "state": "live",
              "attention": "over budget", "attention_class": "needs_you",
              "attention_seq": 9, "attention_detail": "budget exhausted" }
        ]),
        serde_json::json!({}),
    ))];
    let events = derive(&machines, &up);
    let prompt = events
        .iter()
        .find(|e| e.kind == StudioEventKind::RolePrompt)
        .expect("the prompt");
    assert_eq!(prompt.title, "producer needs you: run npm test");
    assert_eq!(
        prompt.key,
        key(&["rprompt", "", "acme-shop", "producer", "7"])
    );
    assert!(matches!(
        prompt.target,
        crate::hollywood::Target::Prompt(..)
    ));
    let needs = events
        .iter()
        .find(|e| e.kind == StudioEventKind::RoleNeedsYou)
        .expect("the needs-you attention");
    assert_eq!(needs.title, "writer needs you: budget exhausted");
    assert!(matches!(needs.target, crate::hollywood::Target::Role(..)));
}

#[test]
fn a_reply_a_card_a_ready_studio_and_a_stopped_stage_each_fire() {
    let machines = [summary(doc_json(
        serde_json::json!([{ "role": "producer", "name": "producer", "state": "closed" }]),
        serde_json::json!({
            "ready_to_finish": true, "tasks_done": 2, "tasks_total": 2,
        }),
        serde_json::json!({
            "hollywood": {
                "state": "live",
                "headline": "all quiet", "headline_seq": 12,
                "exchange": [{ "seq": 12, "who": "hollywood", "text": "all quiet" }],
                "cards": [{ "id": "c3", "studio": "acme-shop",
                            "action": "pause t1", "why": "blocked" }]
            }
        }),
    ))];
    let events = derive(&machines, &stopped_stage);
    let by = |kind: StudioEventKind| {
        events
            .iter()
            .find(|e| e.kind == kind)
            .unwrap_or_else(|| panic!("{kind:?}"))
    };
    assert_eq!(
        by(StudioEventKind::HollywoodReply).key,
        key(&["hreply", "12"])
    );
    assert_eq!(by(StudioEventKind::Card).key, key(&["card", "c3"]));
    assert_eq!(by(StudioEventKind::Card).title, "card waiting: pause t1");
    assert_eq!(
        by(StudioEventKind::Ready).title,
        "acme-shop is ready to finish"
    );
    assert_eq!(
        by(StudioEventKind::Ready).key,
        key(&["ready", "", "acme-shop"])
    );
    assert_eq!(
        by(StudioEventKind::StageStopped).title,
        "stage stopped on Local"
    );
}

#[test]
fn a_blocked_hollywood_an_unreachable_machine_and_no_requirement_stay_quiet() {
    let machines = [summary(serde_json::json!({
        "schema": "backlot/1", "updated_ms": 1,
        "studios": [{
            "id": "acme-shop", "title": "Acme shop", "state": "live",
            "requirement_version": 0, "mode": "standard",
            "open_reviews": 0, "waiting": 0,
            "roles": [{ "role": "producer", "name": "producer", "state": "live" }]
        }],
        "hollywood": { "state": "live", "work": "blocked" }
    }))];
    assert!(derive(&machines, &unreachable).is_empty());
}

/// Item 7: the ping text. A question pings `<studio> · <role> asks: <first
/// line>`, a card `<studio> · card c<n>: <summary>`, and every other kind
/// carries no ping.
#[test]
fn the_ping_names_the_question_and_the_card() {
    let machines = [summary(studio_json(
        serde_json::json!([{ "role": "producer", "name": "producer",
            "state": "live", "asks": "which shop?\nand which region?" }]),
        serde_json::json!({
            "exchange": [{ "seq": 4, "who": "producer", "text": "which shop?" }],
        }),
    ))];
    let events = derive(&machines, &up);
    let asks = events
        .iter()
        .find(|e| e.kind == StudioEventKind::Asks)
        .expect("the ask");
    assert_eq!(
        asks.ping.as_deref(),
        Some("acme-shop · producer asks: which shop?"),
        "a multi-line question pings its first line only"
    );

    let machines = [summary(doc_json(
        serde_json::json!([{ "role": "producer", "name": "producer", "state": "closed" }]),
        serde_json::json!({}),
        serde_json::json!({
            "hollywood": {
                "state": "live",
                "headline": "all quiet", "headline_seq": 12,
                "exchange": [{ "seq": 12, "who": "hollywood", "text": "all quiet" }],
                "cards": [{ "id": "c4", "studio": "acme-shop",
                            "action": "resume t2", "why": "blocked" }]
            }
        }),
    ))];
    let events = derive(&machines, &up);
    let card = events
        .iter()
        .find(|e| e.kind == StudioEventKind::Card)
        .expect("the card");
    assert_eq!(card.ping.as_deref(), Some("acme-shop · card c4: resume t2"));

    // A reply is not a ping: only a question or a card waits on the person.
    let reply = events
        .iter()
        .find(|e| e.kind == StudioEventKind::HollywoodReply)
        .expect("the reply");
    assert!(reply.ping.is_none());
}
