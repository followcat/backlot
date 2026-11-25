//! Escalations that no longer need the person: an `owed` attention
//! whose owner is working again, and an `outward action` refusal whose
//! role has since been handed new work. Both stayed "needs you" until
//! something else cleared them.

use super::*;

fn cleared_with(dir: &Path, wanted: &str) -> bool {
    store::load_events(dir).unwrap().0.iter().any(|e| {
        matches!(
            &e.kind,
            EventKind::AttentionCleared { reason, .. } if reason.as_deref() == Some(wanted)
        )
    })
}

fn open_owed(dir: &Path) -> Vec<backlot_model::state::Attention> {
    attentions(dir)
        .into_iter()
        .filter(|a| a.open && a.reason == reasons::OWED)
        .collect()
}

/// The producer owes `dispatch`, is nudged, sits idle past the
/// escalation window and is escalated; returns the delivery time of
/// the nudge.
async fn escalated_producer(dir: &Path, stage: &Stage, live: &[LiveTerminal]) -> u64 {
    record(dir, "user", studio_kind());
    record(dir, "producer", req_v1());
    record(
        dir,
        "stage",
        role_started("producer", "bl-acme-shop-producer"),
    );
    record(dir, "producer", plan_request());
    record(dir, "writer", publish(PlanKind::Plan, 4));
    let last = record(dir, "writer", publish(PlanKind::Acceptance, 5));
    let t0 = last.ts_ms;
    work(stage, "acme-shop", "producer", Work::Waiting);
    let state = store::load_state(dir).unwrap();
    stage.nudge("acme-shop", dir, &state, live, t0).unwrap();
    stage
        .nudge("acme-shop", dir, &state, live, t0 + 180_000)
        .unwrap();
    let nudge = queued(dir, "nudge").remove(0);
    record(
        dir,
        "stage",
        EventKind::MessageDelivered { target: nudge.seq },
    );
    let delivered_at = now(dir);
    let state = store::load_state(dir).unwrap();
    stage
        .nudge("acme-shop", dir, &state, live, delivered_at + 600_000)
        .unwrap();
    assert_eq!(open_owed(dir).len(), 1, "escalated");
    delivered_at
}

#[tokio::test]
async fn an_owed_attention_clears_once_its_owner_works_again() {
    let dir = tmp();
    let stage = new_stage().await;
    let live = vec![term("acme-shop", "producer")];
    let delivered_at = escalated_producer(&dir, &stage, &live).await;

    // The person talks to the producer: it works and writes progress.
    record(
        &dir,
        "producer",
        EventKind::RoleProgress {
            role: "producer".into(),
            task: None,
            text: "comparing the plans with the person".into(),
        },
    );
    work(&stage, "acme-shop", "producer", Work::Working);
    let t1 = delivered_at + 700_000;
    let state = store::load_state(&dir).unwrap();
    stage.nudge("acme-shop", &dir, &state, &live, t1).unwrap();
    assert!(
        open_owed(&dir).is_empty(),
        "working again: not the person's step"
    );
    assert!(cleared_with(&dir, crate::nudge::ROLE_ACTIVE));

    // Idle again: nothing until a whole window has passed since the
    // producer last wrote to the ledger.
    let acted = store::load_state(&dir).unwrap().last_by_role["producer"].ts_ms;
    work(&stage, "acme-shop", "producer", Work::Waiting);
    let state = store::load_state(&dir).unwrap();
    stage
        .nudge("acme-shop", &dir, &state, &live, acted + 599_000)
        .unwrap();
    assert!(open_owed(&dir).is_empty(), "inside the new window");
    let state = store::load_state(&dir).unwrap();
    stage
        .nudge("acme-shop", &dir, &state, &live, acted + 600_000)
        .unwrap();
    assert_eq!(
        open_owed(&dir).len(),
        1,
        "idle a full window again: re-escalated"
    );
    // And it stands: the owner wrote nothing since this raise.
    let state = store::load_state(&dir).unwrap();
    stage
        .nudge("acme-shop", &dir, &state, &live, acted + 900_000)
        .unwrap();
    assert_eq!(open_owed(&dir).len(), 1);
}

#[tokio::test]
async fn an_owed_attention_stands_while_its_owner_stays_idle() {
    let dir = tmp();
    let stage = new_stage().await;
    let live = vec![term("acme-shop", "producer")];
    let delivered_at = escalated_producer(&dir, &stage, &live).await;
    let state = store::load_state(&dir).unwrap();
    stage
        .nudge("acme-shop", &dir, &state, &live, delivered_at + 2_000_000)
        .unwrap();
    assert_eq!(open_owed(&dir).len(), 1);
}

fn refused(dir: &Path, role: &str) -> backlot_model::Event {
    record(
        dir,
        "stage",
        EventKind::AttentionRaised {
            role: role.into(),
            reason: backlot_model::event::OUTWARD_ACTION.into(),
            detail: "gh issue list -R acme/acme-shop".into(),
            needs_user: true,
        },
    )
}

fn open_outward(dir: &Path) -> usize {
    attentions(dir)
        .iter()
        .filter(|a| a.open && a.reason == backlot_model::event::OUTWARD_ACTION)
        .count()
}

#[tokio::test]
async fn an_outward_refusal_clears_once_its_role_gets_new_work() {
    let dir = tmp();
    record(&dir, "user", studio_kind());
    record(&dir, "stage", role_started("writer", "bl-acme-shop-writer"));
    refused(&dir, "writer");
    let stage = new_stage().await;
    let live = vec![term("acme-shop", "writer")];

    // The refused turn goes on; the person still sees the refusal.
    record(
        &dir,
        "writer",
        EventKind::RoleProgress {
            role: "writer".into(),
            task: None,
            text: "checking closed issues another way".into(),
        },
    );
    let state = store::load_state(&dir).unwrap();
    stage
        .nudge("acme-shop", &dir, &state, &live, now(&dir))
        .unwrap();
    assert_eq!(
        open_outward(&dir),
        1,
        "same turn: still the person's to see"
    );

    // The stage hands the idle writer a new message: the turn that
    // hit the refusal is over.
    let msg = record(
        &dir,
        "producer",
        EventKind::MessageQueued {
            to: "writer".into(),
            file: Some("messages/9-tell.md".into()),
            purpose: None,
            source_seq: None,
        },
    );
    record(
        &dir,
        "stage",
        EventKind::MessageDelivered { target: msg.seq },
    );
    let state = store::load_state(&dir).unwrap();
    stage
        .nudge("acme-shop", &dir, &state, &live, now(&dir))
        .unwrap();
    assert_eq!(open_outward(&dir), 0);
    assert!(cleared_with(&dir, crate::nudge::ROLE_MOVED_ON));
}
