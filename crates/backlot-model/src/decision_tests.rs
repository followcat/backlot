//! Fail-first model tests for shared architectural decisions
//! (`EventKind::MessageQueued { purpose: Some("decision"), .. }`).

use std::collections::BTreeMap;

use crate::event::{EventKind, TaskStateChange};
use crate::state::{Message, StudioState, replay};
use crate::summary::studio_exchange;
use crate::testkit::*;
use crate::validate::{Refusal, validate};
use crate::wanted::wanted_roles;

fn decision_msg(to: &str, file: Option<&str>) -> EventKind {
    EventKind::MessageQueued {
        to: to.into(),
        file: file.map(String::from),
        purpose: Some("decision".into()),
        source_seq: None,
    }
}

#[test]
fn decision_purpose_restricted_to_director_producer_user() {
    let state = running_state();
    let kind = decision_msg("producer", Some("messages/9-ad-1.md"));

    for allowed in ["user", "producer", "director"] {
        assert_eq!(
            validate(&state, &new_ev(allowed, kind.clone())),
            Ok(()),
            "{allowed} should be allowed to record a decision"
        );
    }

    for refused_role in ["actor-t1-c1", "writer", "hollywood", "stage"] {
        assert!(
            matches!(
                validate(&state, &new_ev(refused_role, kind.clone())),
                Err(Refusal::RoleBound { .. })
            ),
            "{refused_role} must be refused from recording a decision"
        );
    }
}

#[test]
fn decision_validate_requires_studio_file_and_rejects_closed_actor() {
    let empty_kind = decision_msg("producer", Some("messages/2-ad-1.md"));
    assert_eq!(
        validate(&StudioState::default(), &new_ev("director", empty_kind)),
        Err(Refusal::NoStudio)
    );

    let state = running_state();
    assert!(
        validate(&state, &new_ev("director", decision_msg("producer", None))).is_err(),
        "decision without a file must be refused"
    );
    assert!(
        validate(
            &state,
            &new_ev("director", decision_msg("producer", Some("   ")))
        )
        .is_err(),
        "decision with an empty file path must be refused"
    );

    let closed = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            (
                "director",
                task_state("t1", TaskStateChange::Abandoned, "cut"),
            ),
        ],
    );
    assert!(matches!(
        validate(
            &closed,
            &new_ev(
                "director",
                decision_msg("actor-t1-c1", Some("messages/9-ad-1.md")),
            ),
        ),
        Err(Refusal::TellToClosedActor { .. })
    ));
}

#[test]
fn decision_never_wakes_closed_role_or_writer() {
    use crate::event::Verdict;

    // Open studio outside plan phase, with t1 accepted (closed actor):
    // queuing `purpose: "decision"` to writer must NOT wake writer, unlike
    // a plain purpose-free tell.
    let base_open = [
        ev(1, "user", studio_kind()),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "first cut".into(),
            },
        ),
        ev(3, "director", task_created("t1", "feature", 1)),
        ev(4, "director", cast("t1", "actor-a", "a")),
        ev(5, "actor-t1-c1", delivered("t1", 1, "a", 1)),
        ev(6, "director", reviewed("t1", 1, Verdict::Accepted, vec![])),
    ];
    let mut with_decision = base_open.to_vec();
    with_decision.push(ev(
        7,
        "director",
        decision_msg("writer", Some("messages/7-ad-1.md")),
    ));
    let open = replay(&with_decision);
    let wanted = wanted_roles(&open);
    assert!(
        !wanted
            .iter()
            .any(|w| w.role == "writer" || w.role == "actor-t1-c1"),
        "decision message must never wake writer or closed actor: {wanted:?}"
    );

    // Contrast: a plain tell (purpose: None) DOES wake writer.
    let mut with_tell = base_open.to_vec();
    with_tell.push(ev(
        7,
        "director",
        EventKind::MessageQueued {
            to: "writer".into(),
            file: Some("messages/7-tell.md".into()),
            purpose: None,
            source_seq: None,
        },
    ));
    assert!(
        wanted_roles(&replay(&with_tell))
            .iter()
            .any(|w| w.role == "writer")
    );

    // Closed (finished) studio: decision queued to fixed roles wakes nobody.
    let closed = replay(&[
        ev(1, "user", studio_kind()),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "first cut".into(),
            },
        ),
        ev(
            3,
            "user",
            EventKind::StudioFinished {
                reason: "done".into(),
            },
        ),
        ev(
            4,
            "director",
            decision_msg("director", Some("messages/4-ad-2.md")),
        ),
        ev(
            5,
            "producer",
            decision_msg("writer", Some("messages/5-ad-3.md")),
        ),
    ]);
    assert!(wanted_roles(&closed).is_empty());
}

#[test]
fn studio_exchange_surfaces_user_decision_not_internal_role_decision() {
    let mut state = base_state();
    state.messages.push(Message {
        seq: 10,
        from: "user".into(),
        to: "producer".into(),
        file: Some("messages/10-ad-1.md".into()),
        purpose: Some("decision".into()),
        source_seq: None,
        delivered: true,
        dropped: false,
        dropped_reason: None,
        ts_ms: 1000,
        delivered_ms: Some(1100),
    });
    state.messages.push(Message {
        seq: 11,
        from: "director".into(),
        to: "producer".into(),
        file: Some("messages/11-ad-2.md".into()),
        purpose: Some("decision".into()),
        source_seq: None,
        delivered: true,
        dropped: false,
        dropped_reason: None,
        ts_ms: 1200,
        delivered_ms: Some(1300),
    });
    let files = BTreeMap::from([
        (10u64, "# AD-1\n\nUse Postgres JSONB.\n".to_string()),
        (11u64, "# AD-2\n\nUse Redis cache.\n".to_string()),
    ]);
    let ex = studio_exchange(&state, &files);
    assert!(
        ex.iter().any(|e| e.seq == 10 && e.who == "user"),
        "user decision must appear in studio_exchange like a tell: {ex:?}"
    );
    assert!(
        !ex.iter().any(|e| e.seq == 11),
        "director -> producer internal decision stays out of user talk rows: {ex:?}"
    );
}
