//! Hollywood ledger tests: the event envelope's wire shape and the
//! replay-equals-apply promise the studio ledger makes too.

use crate::event::ActorIdentity;
use crate::hollywood::*;
use crate::testkit::*;

/// Every kind serializes with its `kind` tag and its own fields.
#[test]
fn the_envelope_matches_the_studio_ledger() {
    let e = hev(3, "hollywood", hpropose("t1"));
    let v = serde_json::to_value(&e).unwrap();
    assert_eq!(v["seq"], 3);
    assert_eq!(v["ts_ms"], 3000);
    assert_eq!(v["by"]["role"], "hollywood");
    assert_eq!(v["kind"], "card.proposed");
    assert_eq!(v["action"]["action"], "pause");
    assert_eq!(v["action"]["studio"], "acme-shop");
    assert_eq!(v["base"]["base"], "task_status");
    assert_eq!(v["base"]["was"], "running");
    assert_eq!(v["why"], "actor is looping");
    for (kind, tag) in [
        (HollywoodKind::CardApplied { card: 3 }, "card.applied"),
        (
            HollywoodKind::CardCancelled {
                card: 3,
                reason: "no".into(),
            },
            "card.cancelled",
        ),
        (
            HollywoodKind::CardDone {
                card: 3,
                note: String::new(),
            },
            "card.done",
        ),
        (
            HollywoodKind::CardFailed {
                card: 3,
                reason: "boom".into(),
            },
            "card.failed",
        ),
        (
            HollywoodKind::CardStale {
                card: 3,
                reason: "moved".into(),
            },
            "card.stale",
        ),
        (
            HollywoodKind::Reported {
                headline: "all quiet".into(),
                body: Some("reports/3.md".into()),
            },
            "hollywood.reported",
        ),
        (
            HollywoodKind::MessageSent {
                to: "hollywood".into(),
                studio: None,
                text: Some("hello".into()),
                file: None,
                source_seq: None,
            },
            "message.sent",
        ),
    ] {
        let v = serde_json::to_value(hev(9, "user", kind)).unwrap();
        assert_eq!(v["kind"], tag);
    }
}

/// A restart of Hollywood is a request the close keeps and the start
/// clears, the same shape a studio's `role.restart` has.
#[test]
fn a_restart_is_a_request_the_start_clears() {
    let started = |session: Option<&str>, reason: Option<&str>| HollywoodKind::RoleState {
        state: RoleMotion::Started,
        terminal: Some("bl-hollywood".into()),
        actor: None,
        session_id: session.map(String::from),
        reason: reason.map(String::from),
    };
    let resumed = |session: Option<&str>, reason: &str| HollywoodKind::RoleState {
        state: RoleMotion::Resumed,
        terminal: Some("bl-hollywood".into()),
        actor: None,
        session_id: session.map(String::from),
        reason: Some(reason.into()),
    };
    let mut state = replay(&[hev(1, "stage", started(Some("sess-h"), None))]);
    apply(
        &mut state,
        &hev(
            2,
            "user",
            HollywoodKind::RoleRestart {
                fresh: false,
                reason: Some("stuck".into()),
            },
        ),
    );
    assert_eq!(state.role.restart.as_ref().map(|r| r.seq), Some(2));

    // The close the restart asked for keeps the request and session.
    apply(
        &mut state,
        &hev(
            3,
            "stage",
            HollywoodKind::RoleState {
                state: RoleMotion::Closed,
                terminal: None,
                actor: None,
                session_id: None,
                reason: Some(crate::event::RESTARTED.into()),
            },
        ),
    );
    assert!(state.role.restart.is_some(), "the restart's close keeps it");
    assert_eq!(state.role.session_id.as_deref(), Some("sess-h"));

    // The resumed start clears it and stamps `restarted_ms`.
    apply(
        &mut state,
        &hev(4, "stage", resumed(Some("sess-h"), crate::event::RESTARTED)),
    );
    assert!(state.role.restart.is_none());
    assert_eq!(state.role.restarted_ms, Some(4000));
    assert_eq!(state.role.session_id.as_deref(), Some("sess-h"));

    // A plain close ends a request outright.
    apply(
        &mut state,
        &hev(
            5,
            "user",
            HollywoodKind::RoleRestart {
                fresh: true,
                reason: None,
            },
        ),
    );
    apply(
        &mut state,
        &hev(
            6,
            "stage",
            HollywoodKind::RoleState {
                state: RoleMotion::Closed,
                terminal: None,
                actor: None,
                session_id: None,
                reason: Some("not configured".into()),
            },
        ),
    );
    assert!(state.role.restart.is_none());
}

/// Replaying the log is applying each event in order — the same
/// promise the studio ledger makes.
#[test]
fn replay_equals_incremental_apply() {
    let events = vec![
        hev(1, "hollywood", hpropose("t1")),
        hev(2, "hollywood", hpropose("t2")),
        hev(3, "user", HollywoodKind::CardApplied { card: 1 }),
        hev(
            4,
            "stage",
            HollywoodKind::CardFailed {
                card: 1,
                reason: "stage down".into(),
            },
        ),
        hev(
            5,
            "user",
            HollywoodKind::CardCancelled {
                card: 2,
                reason: "changed my mind".into(),
            },
        ),
        hev(
            6,
            "hollywood",
            HollywoodKind::Reported {
                headline: "one done, one dropped".into(),
                body: None,
            },
        ),
        hev(
            7,
            "user",
            HollywoodKind::MessageSent {
                to: "hollywood".into(),
                studio: None,
                text: Some("thanks".into()),
                file: None,
                source_seq: None,
            },
        ),
    ];
    let mut inc = HollywoodState::default();
    for e in &events {
        apply(&mut inc, e);
    }
    let state = replay(&events);
    assert_eq!(state, inc);
    assert_eq!(state.last_seq, 7);
    assert_eq!(state.cards[&1].state, CardState::Failed);
    assert_eq!(state.cards[&1].detail, "stage down");
    assert_eq!(state.cards[&1].closed_seq, Some(4));
    assert_eq!(state.cards[&2].state, CardState::Cancelled);
    assert_eq!(state.reports.len(), 1);
    assert_eq!(state.last_headline(), Some("one done, one dropped"));
    assert_eq!(state.messages.len(), 1);
    assert_eq!(state.messages[0].from, "user");
    assert_eq!(state.messages[0].to, "hollywood");
}

/// R1-4: a `closed` stamps `since_ms` with its own ts and clears the
/// loss history — the stage's revive check reads "was I born after
/// this close", and losses from the closed lifecycle never count
/// toward a new one's guard.
#[test]
fn a_close_stamps_since_and_ends_the_loss_count() {
    let role_state = |state: RoleMotion, reason: Option<&str>| HollywoodKind::RoleState {
        state,
        terminal: None,
        actor: None,
        session_id: None,
        reason: reason.map(String::from),
    };
    let events = vec![
        hev(1, "stage", role_state(RoleMotion::Started, None)),
        hev(2, "stage", role_state(RoleMotion::Lost, None)),
        hev(3, "stage", role_state(RoleMotion::Lost, None)),
        hev(
            4,
            "stage",
            role_state(RoleMotion::Closed, Some("cannot resume")),
        ),
    ];
    let state = replay(&events);
    assert!(state.role.closed);
    assert!(!state.role.live);
    assert_eq!(state.role.closed_reason.as_deref(), Some("cannot resume"));
    assert_eq!(state.role.since_ms, 4000);
    assert!(state.role.losses.is_empty());
    // A later `lost` starts a fresh count from zero.
    let state = replay(&[
        events[0].clone(),
        events[1].clone(),
        events[3].clone(),
        hev(5, "stage", role_state(RoleMotion::Lost, None)),
    ]);
    assert_eq!(state.role.losses, vec![5000]);
}

/// A close ends the conversation, and a start begins a fresh one: a
/// session id recorded under the old runner never survives either.
/// Otherwise a new runner's resume would be handed a session it never
/// minted — `cmd -r <devin id>` is `No session named …`.
#[test]
fn a_close_or_a_session_less_start_clears_the_old_session() {
    let actor = |runner: &str| ActorIdentity {
        runner: runner.into(),
        runner_version: None,
        model: "m".into(),
        effort: None,
        permission_profile: "standard".into(),
    };
    let role_state =
        |state: RoleMotion, actor: Option<ActorIdentity>, session: Option<&str>| -> HollywoodKind {
            HollywoodKind::RoleState {
                state,
                terminal: None,
                actor,
                session_id: session.map(String::from),
                reason: None,
            }
        };
    // devin started → closed ("runner changed") → cmd started, no
    // session yet: the devin id is gone.
    let state = replay(&[
        hev(
            1,
            "stage",
            role_state(
                RoleMotion::Started,
                Some(actor("devin")),
                Some("beaded-harp"),
            ),
        ),
        hev(2, "stage", role_state(RoleMotion::Closed, None, None)),
        hev(
            3,
            "stage",
            role_state(RoleMotion::Started, Some(actor("cmd")), None),
        ),
    ]);
    assert_eq!(state.role.session_id, None);
    assert_eq!(state.role.actor.as_ref().unwrap().runner, "cmd");
    // A close alone clears it too.
    let state = replay(&[
        hev(
            1,
            "stage",
            role_state(
                RoleMotion::Started,
                Some(actor("devin")),
                Some("beaded-harp"),
            ),
        ),
        hev(2, "stage", role_state(RoleMotion::Closed, None, None)),
    ]);
    assert_eq!(state.role.session_id, None);
    // A resumed record that carries no id keeps the one it had.
    let state = replay(&[
        hev(
            1,
            "stage",
            role_state(
                RoleMotion::Started,
                Some(actor("devin")),
                Some("beaded-harp"),
            ),
        ),
        hev(2, "stage", role_state(RoleMotion::Resumed, None, None)),
    ]);
    assert_eq!(state.role.session_id.as_deref(), Some("beaded-harp"));
}
