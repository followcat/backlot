//! Tests for `CardAction::Setup` and `CardBase::HollywoodIdentity`.

use crate::event::ActorIdentity;
use crate::hollywood::{
    CardAction, CardBase, HollywoodKind, format_setup_transition, stale, stale_hollywood,
};
use crate::hollywood_state::replay;
use crate::hollywood_summary::hollywood_section_with_activity;
use crate::hollywood_validate::validate;
use crate::testkit::{hev, nhw};

fn actor(runner: &str, model: &str, effort: Option<&str>) -> ActorIdentity {
    ActorIdentity {
        runner: runner.into(),
        runner_version: None,
        model: model.into(),
        effort: effort.map(str::to_string),
        permission_profile: "standard".into(),
    }
}

#[test]
fn setup_serde_defaults_optional_fields() {
    let raw = r#"{"action":"setup","runner":"cmd","model":"deepseek-v4.1-flash"}"#;
    let act: CardAction = serde_json::from_str(raw).unwrap();
    assert_eq!(
        act,
        CardAction::Setup {
            runner: "cmd".into(),
            model: "deepseek-v4.1-flash".into(),
            effort: None,
            permission_profile: None,
        }
    );
    assert_eq!(act.studio(), "hollywood");
    assert_eq!(
        act.setup_actor(),
        Some(actor("cmd", "deepseek-v4.1-flash", None))
    );
}

#[test]
fn setup_base_validation_matches_hollywood_identity_only() {
    let setup = CardAction::Setup {
        runner: "claude".into(),
        model: "claude-opus-5-5".into(),
        effort: Some("xhigh".into()),
        permission_profile: None,
    };
    let good = CardBase::HollywoodIdentity {
        actor: Some(actor("cmd", "deepseek-v4.1-flash", None)),
    };
    let mut state = replay(&[]);
    let ok = nhw(
        "hollywood",
        HollywoodKind::CardProposed {
            action: setup.clone(),
            why: "person asked".into(),
            base: good.clone(),
        },
    );
    assert!(validate(&state, &ok).is_ok());

    let unset = CardBase::HollywoodIdentity { actor: None };
    let ok_unset = nhw(
        "hollywood",
        HollywoodKind::CardProposed {
            action: setup.clone(),
            why: "initial config".into(),
            base: unset,
        },
    );
    assert!(validate(&state, &ok_unset).is_ok());

    for bad in [
        CardBase::StudioAbsent,
        CardBase::RoleIdentity {
            actor: actor("cmd", "deepseek-v4.1-flash", None),
        },
    ] {
        let ev = nhw(
            "hollywood",
            HollywoodKind::CardProposed {
                action: setup.clone(),
                why: "mismatch".into(),
                base: bad,
            },
        );
        assert!(validate(&state, &ev).is_err());
    }

    let recast_with_hw_base = nhw(
        "hollywood",
        HollywoodKind::CardProposed {
            action: CardAction::Recast {
                studio: "acme-shop".into(),
                person: "director".into(),
                actor: actor("fake", "m2", None),
            },
            why: "mismatch".into(),
            base: good,
        },
    );
    assert!(validate(&state, &recast_with_hw_base).is_err());

    let e1 = hev(1, "hollywood", ok.kind);
    let e2 = hev(2, "user", HollywoodKind::CardApplied { card: 1 });
    let e3 = hev(
        3,
        "stage",
        HollywoodKind::CardDone {
            card: 1,
            note: String::new(),
        },
    );
    state = replay(&[e1, e2, e3]);
    assert_eq!(state.open_cards().count(), 0);
}

#[test]
fn setup_transition_and_summary_show_both_identities() {
    let old = actor("cmd", "deepseek-v4.1-flash", None);
    let action = CardAction::Setup {
        runner: "claude".into(),
        model: "claude-opus-5-5".into(),
        effort: Some("xhigh".into()),
        permission_profile: None,
    };
    let base = CardBase::HollywoodIdentity {
        actor: Some(old.clone()),
    };
    assert_eq!(
        format_setup_transition(Some(&old), "claude", "claude-opus-5-5", Some("xhigh")),
        "Hollywood: cmd / deepseek-v4.1-flash → claude / claude-opus-5-5 / xhigh"
    );
    assert_eq!(
        format_setup_transition(None, "fake", "m2", None),
        "Hollywood: (unset) → fake / m2"
    );

    let state = replay(&[hev(
        1,
        "hollywood",
        HollywoodKind::CardProposed {
            action,
            why: "switch agent".into(),
            base,
        },
    )]);
    let summary = hollywood_section_with_activity(
        &state,
        "live",
        Some("cmd"),
        Some("deepseek-v4.1-flash"),
        None,
        None,
        None,
        None,
    );
    assert_eq!(summary.cards.len(), 1);
    assert_eq!(summary.cards[0].studio, "hollywood");
    assert_eq!(
        summary.cards[0].action,
        "Hollywood: cmd / deepseek-v4.1-flash → claude / claude-opus-5-5 / xhigh"
    );
}

#[test]
fn stale_hollywood_detects_config_drift() {
    let old = actor("fake", "m1", None);
    let new = actor("fake", "m2", Some("high"));
    let base = CardBase::HollywoodIdentity {
        actor: Some(old.clone()),
    };
    let action = CardAction::Setup {
        runner: "fake".into(),
        model: "m2".into(),
        effort: Some("high".into()),
        permission_profile: None,
    };
    assert_eq!(stale(&action, &base, None), None);
    assert_eq!(stale_hollywood(&base, Some(&old)), None);
    assert!(stale_hollywood(&base, Some(&new)).is_some());
    assert!(stale_hollywood(&base, None).is_some());

    let unset = CardBase::HollywoodIdentity { actor: None };
    assert_eq!(stale_hollywood(&unset, None), None);
    assert!(stale_hollywood(&unset, Some(&old)).is_some());
}
