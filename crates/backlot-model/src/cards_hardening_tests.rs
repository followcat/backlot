//! The card hardening rules (field notes §1–2): a newer card on one
//! fact supersedes the older open one, Hollywood may withdraw only its
//! own card, and a `reopen-studio` card's base is the studio being
//! finished.

use crate::event::{ActorIdentity, Priority};
use crate::hollywood::*;
use crate::testkit::*;

fn recast(person: &str, model: &str) -> CardAction {
    CardAction::Recast {
        studio: "acme-shop".into(),
        person: person.into(),
        actor: ActorIdentity {
            runner: "claude".into(),
            runner_version: None,
            model: model.into(),
            effort: Some("high".into()),
            permission_profile: "standard".into(),
        },
    }
}

fn hr(action: CardAction) -> HollywoodKind {
    HollywoodKind::CardProposed {
        action,
        why: "because".into(),
        base: CardBase::RoleIdentity {
            actor: tk_actor("dir"),
        },
    }
}

fn tk_actor(model: &str) -> ActorIdentity {
    ActorIdentity {
        runner: "fake".into(),
        runner_version: None,
        model: model.into(),
        effort: Some("high".into()),
        permission_profile: "standard".into(),
    }
}

/// A recast on the same role supersedes the older open recast; a recast
/// on a different role does not.
#[test]
fn a_recast_supersedes_an_open_recast_on_the_same_role() {
    let same = recast("director", "model-b");
    assert!(supersedes(&same, &recast("director", "model-a")));
    assert!(!supersedes(&same, &recast("writer", "model-a")));
}

/// The kinds with one obvious target, and only those, supersede.
#[test]
fn supersede_covers_the_kinds_with_one_target() {
    let a = CardAction::Cast {
        studio: "acme-shop".into(),
        cast: "a.toml".into(),
        snapshot: None,
    };
    let b = CardAction::Cast {
        studio: "acme-shop".into(),
        cast: "b.toml".into(),
        snapshot: None,
    };
    assert!(supersedes(&b, &a), "two cast cards on one studio");
    let elsewhere = CardAction::Cast {
        studio: "other".into(),
        cast: "b.toml".into(),
        snapshot: None,
    };
    assert!(!supersedes(&b, &elsewhere));

    let reopen_a = CardAction::ReopenStudio {
        studio: "acme-shop".into(),
    };
    let reopen_b = CardAction::ReopenStudio {
        studio: "acme-shop".into(),
    };
    assert!(supersedes(&reopen_b, &reopen_a));

    // A pause and a resume on one task are a sequence, not a conflict —
    // a person may want both in order.
    let pause = CardAction::Pause {
        studio: "acme-shop".into(),
        task: "t1".into(),
    };
    let resume = CardAction::Resume {
        studio: "acme-shop".into(),
        task: "t1".into(),
    };
    assert!(!supersedes(&resume, &pause));

    // Two setup cards both change Hollywood's one identity.
    let setup = CardAction::Setup {
        runner: "claude".into(),
        model: "m".into(),
        effort: None,
        permission_profile: None,
    };
    assert!(supersedes(&setup, &setup));
}

/// The state lists exactly the open cards a new action would replace.
#[test]
fn open_cards_superseded_by_reads_only_open_cards() {
    let mut state = HollywoodState::default();
    apply(
        &mut state,
        &hev(1, "hollywood", hr(recast("director", "model-a"))),
    );
    apply(
        &mut state,
        &hev(2, "hollywood", hr(recast("writer", "model-b"))),
    );
    // The director card closed: only the writer card is open.
    apply(
        &mut state,
        &hev(
            3,
            "user",
            HollywoodKind::CardCancelled {
                card: 1,
                reason: String::new(),
            },
        ),
    );
    let superseded = state.open_cards_superseded_by(&recast("writer", "model-c"));
    assert_eq!(superseded, vec![2]);
}

/// Hollywood may withdraw its own open card; the user's card and a
/// closed card are refused, and a different role may not withdraw at
/// all.
#[test]
fn only_hollywood_withdraws_its_own_open_card() {
    let mut state = HollywoodState::default();
    apply(
        &mut state,
        &hev(1, "hollywood", hr(recast("director", "model-a"))),
    );
    let withdraw = HollywoodKind::CardWithdrawn {
        card: 1,
        reason: "quota came back".into(),
    };
    assert!(validate(&state, &nhw("hollywood", withdraw.clone())).is_ok());
    // A role that is not Hollywood cannot withdraw.
    for role in ["user", "stage", "producer"] {
        assert_eq!(
            validate(&state, &nhw(role, withdraw.clone())),
            Err(HRefusal::RoleBound {
                role: role.into(),
                who: "hollywood"
            }),
            "{role}"
        );
    }
    // Once withdrawn, another withdraw is refused — never twice.
    apply(&mut state, &hev(2, "hollywood", withdraw.clone()));
    let again = validate(&state, &nhw("hollywood", withdraw));
    assert!(
        matches!(again, Err(HRefusal::CardClosed { .. })),
        "{again:?}"
    );
}

/// A card someone else proposed is not Hollywood's to withdraw: the
/// stage's outward card is the case the rule exists for.
#[test]
fn hollywood_cannot_withdraw_the_stages_card() {
    let mut state = HollywoodState::default();
    apply(
        &mut state,
        &hev(
            1,
            "stage",
            HollywoodKind::CardProposed {
                action: CardAction::Outward {
                    studio: "acme-shop".into(),
                    role: "actor-t1-c1".into(),
                    source_seq: 9,
                    argv: vec!["git".into(), "push".into()],
                    cwd: "/home/user/acme-shop".into(),
                    repo: "/home/user/acme-shop".into(),
                    remote_url: "git@host-a:shop.git".into(),
                    rule: "push".into(),
                    commits: Vec::new(),
                    commits_total: 0,
                },
                why: "role asked".into(),
                base: CardBase::GitHead {
                    repo: "/home/user/acme-shop".into(),
                    head_oid: "abc".into(),
                    remote_ref: "refs/heads/main".into(),
                    remote_oid: String::new(),
                },
            },
        ),
    );
    let withdraw = HollywoodKind::CardWithdrawn {
        card: 1,
        reason: "gone".into(),
    };
    assert_eq!(
        validate(&state, &nhw("hollywood", withdraw)),
        Err(HRefusal::NotOwnCard(1))
    );
}

/// A `reopen-studio` card bases on the studio being finished.
#[test]
fn reopen_studio_bases_on_the_finished_marker() {
    let action = CardAction::ReopenStudio {
        studio: "acme-shop".into(),
    };
    assert!(stale(&action, &CardBase::StudioFinished, None).is_some());
    let mut state = crate::state::StudioState::default();
    // Not finished: the base does not hold.
    assert_eq!(
        stale(&action, &CardBase::StudioFinished, Some(&state)),
        Some("studio 'acme-shop' is not finished".into())
    );
    state.finished = Some(crate::state::FinishRecord {
        seq: 4,
        reason: "done".into(),
        ts_ms: 4_000,
    });
    assert_eq!(
        stale(&action, &CardBase::StudioFinished, Some(&state)),
        None
    );
}

/// A reopen's base pairs only with a `reopen-studio` action.
#[test]
fn the_finished_base_matches_only_a_reopen() {
    let pause = CardAction::Pause {
        studio: "acme-shop".into(),
        task: "t1".into(),
    };
    assert!(stale(&pause, &CardBase::StudioFinished, None).is_some());
}

/// A snapshot's staleness names the edited file — the cast file changed
/// after the card was proposed.
#[test]
fn an_edited_source_makes_a_snapshot_stale() {
    let snapshot = CardSnapshot {
        cast: Some(SnapshotFile::new(
            "/home/user/cast.toml",
            sha256_hex(b"one"),
        )),
        brief: Some(SnapshotFile::new("/home/user/brief.md", sha256_hex(b"b"))),
    };
    // Unchanged: the same bytes hash the same.
    assert_eq!(
        snapshot.stale(|src| {
            Some(if src.ends_with("cast.toml") {
                b"one".to_vec()
            } else {
                b"b".to_vec()
            })
        }),
        None,
        "an unchanged file is not stale"
    );
    // The cast file edited after the proposal.
    assert_eq!(
        snapshot.stale(|src| {
            if src.ends_with("cast.toml") {
                Some(b"two".to_vec())
            } else {
                Some(b"b".to_vec())
            }
        }),
        Some("the cast file changed after this card was proposed".into())
    );
    // A source that is gone leaves the check silent rather than lying.
    assert_eq!(snapshot.stale(|_| None), None);
}

/// `Priority` is imported so the file's other checks compile; the
/// supersede rule never keys on it.
#[test]
fn priority_is_not_a_supersede_target() {
    let a = CardAction::Priority {
        studio: "acme-shop".into(),
        task: "t1".into(),
        level: Priority::High,
    };
    let b = CardAction::Priority {
        studio: "acme-shop".into(),
        task: "t1".into(),
        level: Priority::Low,
    };
    assert!(!supersedes(&b, &a));
}
