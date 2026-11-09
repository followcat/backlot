//! Tests for the closed-loop events (S5a): `role.approval`,
//! `studio.finished`, `message.queued`'s `source_seq`, the cast's
//! `by` and the attention's `ts_ms`.

use crate::event::*;
use crate::state::{StudioState, apply, replay};
use crate::testkit::*;
use crate::validate::{Refusal, validate};

fn permission(role: &str) -> EventKind {
    EventKind::AttentionRaised {
        role: role.into(),
        reason: PERMISSION_PROMPT.into(),
        detail: String::new(),
    }
}

/// An approval needs the role's open `permission prompt` attention;
/// once recorded it lands in `approvals` with the decision.
#[test]
fn approval_needs_an_open_permission_attention() {
    let state = base_state();
    let approve = |role: &str, decision: ApprovalDecision| {
        new_ev(
            "director",
            EventKind::RoleApproval {
                role: role.into(),
                decision,
            },
        )
    };
    assert_eq!(
        validate(&state, &approve("actor-t1-c1", ApprovalDecision::Allow)),
        Err(Refusal::NoPermissionPrompt("actor-t1-c1".into()))
    );
    // Open the attention — an approval on that role validates.
    let mut open = base_state();
    apply(&mut open, &ev(4, "stage", permission("actor-t1-c1")));
    assert_eq!(
        validate(&open, &approve("actor-t1-c1", ApprovalDecision::Deny)),
        Ok(())
    );
    // Another role's attention does not count.
    assert_eq!(
        validate(&open, &approve("director", ApprovalDecision::Allow)),
        Err(Refusal::NoPermissionPrompt("director".into()))
    );
    // A cleared attention answers nothing.
    let mut cleared = open;
    apply(
        &mut cleared,
        &ev(
            5,
            "stage",
            EventKind::AttentionCleared {
                target: 4,
                reason: None,
            },
        ),
    );
    assert_eq!(
        validate(&cleared, &approve("actor-t1-c1", ApprovalDecision::Allow)),
        Err(Refusal::NoPermissionPrompt("actor-t1-c1".into()))
    );
}

/// The folded record keeps role, who recorded it, decision and
/// time; replay and incremental apply agree.
#[test]
fn approval_folds_into_approvals() {
    let mut log = base_events();
    log.push(ev(4, "stage", permission("actor-t1-c1")));
    log.push(ev(
        5,
        "director",
        EventKind::RoleApproval {
            role: "actor-t1-c1".into(),
            decision: ApprovalDecision::Deny,
        },
    ));
    let state = replay(&log);
    assert_eq!(state.approvals.len(), 1);
    let a = &state.approvals[0];
    assert_eq!(a.seq, 5);
    assert_eq!(a.role, "actor-t1-c1");
    assert_eq!(a.by, "director");
    assert_eq!(a.decision, ApprovalDecision::Deny);
    assert_eq!(a.ts_ms, 5_000);
}

/// Finish refuses while any task is not terminal and names them all;
/// once every task is terminal it records `studio.finished` — and a
/// second finish is refused.
#[test]
fn finish_needs_every_task_terminal() {
    let state = running_state(); // t1 running, t2 created
    let finish = new_ev(
        "producer",
        EventKind::StudioFinished {
            reason: "done".into(),
        },
    );
    assert_eq!(
        validate(&state, &finish),
        Err(Refusal::UnfinishedTasks(vec!["t1".into(), "t2".into()]))
    );
    // Accept t1 (needs a delivery first) and abandon t2, then finish
    // validates.
    let mut done = state;
    let mut seq = done.last_seq;
    for (role, kind) in [
        ("actor-t1-c1", delivered("t1", 1, "a", 1)),
        ("director", reviewed("t1", 1, Verdict::Accepted, vec![])),
        (
            "director",
            EventKind::TaskState {
                task: "t2".into(),
                state: TaskStateChange::Abandoned,
                reason: "dropped".into(),
            },
        ),
    ] {
        seq += 1;
        apply(&mut done, &ev(seq, role, kind));
    }
    assert_eq!(validate(&done, &finish), Ok(()));
    apply(&mut done, &ev(seq + 1, "producer", finish.kind.clone()));
    assert_eq!(
        done.finished.as_ref().map(|f| (f.seq, f.reason.as_str())),
        Some((seq + 1, "done"))
    );
    assert_eq!(validate(&done, &finish), Err(Refusal::AlreadyFinished));
}

/// `source_seq` rides on `message.queued` and survives the log; a
/// plain message serializes without it.
#[test]
fn message_source_seq_round_trips() {
    let mut state = base_state();
    apply(
        &mut state,
        &ev(
            4,
            "stage",
            EventKind::MessageQueued {
                to: "director".into(),
                file: Some("messages/auto2.md".into()),
                purpose: None,
                source_seq: Some(2),
            },
        ),
    );
    let m = state.messages.last().unwrap();
    assert_eq!(m.source_seq, Some(2));
    let json = serde_json::to_string(m).unwrap();
    assert!(json.contains("\"source_seq\":2"), "{json}");
    // A human message carries no field at all.
    let plain = EventKind::MessageQueued {
        to: "director".into(),
        file: None,
        purpose: None,
        source_seq: None,
    };
    let event = new_ev("producer", plain);
    let line = serde_json::to_string(&event).unwrap();
    assert!(!line.contains("source_seq"), "{line}");
}

/// The cast record keeps who cast it: a producer cast reads back as
/// `producer`, a director's as `director`.
#[test]
fn cast_records_who_cast() {
    let state = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            ("producer", task_created("t2", "fix", 1)),
            ("producer", cast("t2", "actor-b", "b")),
        ],
    );
    assert_eq!(state.tasks["t1"].cast.as_ref().unwrap().by, "director");
    assert_eq!(state.tasks["t2"].cast.as_ref().unwrap().by, "producer");
}

/// Attention carries the raise's timestamp — a pre-S5a record with
/// `ts_ms` absent deserializes as 0.
#[test]
fn attention_keeps_its_raise_time() {
    let mut state = StudioState::default();
    apply(&mut state, &ev(1, "stage", permission("director")));
    assert_eq!(state.attention[0].ts_ms, 1_000);
    let old: crate::state::Attention =
        serde_json::from_str(r#"{"seq":1,"role":"d","reason":"x","detail":"","open":true}"#)
            .unwrap();
    assert_eq!(old.ts_ms, 0);
}
