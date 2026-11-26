//! Wire compatibility, additive `queue` / `handoff` fields on
//! `HollywoodSummary`, and ledger backward-compatibility checks for
//! make-before-break Hollywood agent switches.

use crate::event::ActorIdentity;
use crate::hollywood::{HOLLYWOOD_TERMINAL, HollywoodKind, HollywoodState, RoleMotion, apply};
use crate::summary::{HandoffSummary, QueueReason, QueueStatus, hollywood_section};
use crate::testkit::*;

#[test]
fn queue_status_and_handoff_summary_round_trip_and_skip_when_none() {
    let state = HollywoodState::default();
    let mut s = hollywood_section(&state, "live", Some("claude"), Some("sonnet"), Some("high"));
    let plain = serde_json::to_value(&s).unwrap();
    assert!(plain.get("queue").is_none());
    assert!(plain.get("handoff").is_none());

    s.queue = Some(QueueStatus {
        count: 2,
        reason: QueueReason::OutOfQuota,
        detail: Some("weekly limit reached".into()),
    });
    s.handoff = Some(HandoffSummary {
        id: "h-1".into(),
        phase: "verifying".into(),
        target_runner: "codex".into(),
        target_model: "gpt-6-sol".into(),
        target_effort: Some("high".into()),
        candidate_terminal: Some("bl-hollywood-next".into()),
        started_ms: 1000,
        timeout_ms: 90_000,
        error: None,
    });

    let encoded = serde_json::to_value(&s).unwrap();
    assert_eq!(encoded["queue"]["count"], 2);
    assert_eq!(encoded["queue"]["reason"], "out_of_quota");
    assert_eq!(encoded["queue"]["detail"], "weekly limit reached");
    assert_eq!(encoded["handoff"]["phase"], "verifying");
    assert_eq!(
        encoded["handoff"]["candidate_terminal"],
        "bl-hollywood-next"
    );

    let decoded: crate::summary::HollywoodSummary = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, s);

    // Older consumer structs without `queue`/`handoff` ignore additive fields safely.
    #[derive(serde::Deserialize)]
    struct LegacyHollywoodSummary {
        state: String,
    }
    let legacy: LegacyHollywoodSummary =
        serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(legacy.state, "live");
}

#[test]
fn role_state_handoff_events_replay_cleanly_without_new_event_kinds() {
    let mut state = HollywoodState::default();
    let old_actor = ActorIdentity {
        runner: "claude".into(),
        runner_version: None,
        model: "sonnet".into(),
        effort: Some("high".into()),
        permission_profile: "standard".into(),
    };
    let new_actor = ActorIdentity {
        runner: "codex".into(),
        runner_version: None,
        model: "gpt-6-sol".into(),
        effort: Some("high".into()),
        permission_profile: "standard".into(),
    };
    apply(
        &mut state,
        &hev(
            1,
            "stage",
            HollywoodKind::RoleState {
                state: RoleMotion::Started,
                terminal: Some(HOLLYWOOD_TERMINAL.into()),
                actor: Some(old_actor.clone()),
                session_id: Some("sess-old".into()),
                reason: None,
            },
        ),
    );
    apply(
        &mut state,
        &hev(
            2,
            "stage",
            HollywoodKind::RoleState {
                state: RoleMotion::Closed,
                terminal: Some(HOLLYWOOD_TERMINAL.into()),
                actor: Some(old_actor),
                session_id: None,
                reason: Some("handoff -> codex/gpt-6-sol/high".into()),
            },
        ),
    );
    apply(
        &mut state,
        &hev(
            3,
            "stage",
            HollywoodKind::RoleState {
                state: RoleMotion::Started,
                terminal: Some("bl-hollywood-next".into()),
                actor: Some(new_actor.clone()),
                session_id: Some("sess-new".into()),
                reason: Some("handoff from claude/sonnet/high".into()),
            },
        ),
    );
    assert!(state.role.live);
    assert_eq!(state.role.terminal.as_deref(), Some("bl-hollywood-next"));
    assert_eq!(state.role.actor.as_ref(), Some(&new_actor));
    assert_eq!(state.role.session_id.as_deref(), Some("sess-new"));
}
