//! Every model-side attention reason and its class, plus the rule for
//! an unknown reason and the escalation flag.

use super::AttentionClass;

/// `(reason, class)` for the reasons defined in `event.rs` (and the
/// `folder-trust` dialog a `stage.answered` names).
#[test]
fn every_model_reason_has_its_class() {
    let cases = [
        (crate::event::PERMISSION_PROMPT, AttentionClass::NeedsYou),
        (crate::event::STALLED, AttentionClass::Stuck),
        (crate::event::CANNOT_RESUME, AttentionClass::NeedsYou),
        (crate::event::FOLDER_TRUST, AttentionClass::NeedsYou),
    ];
    for (reason, want) in cases {
        assert_eq!(AttentionClass::of(reason, false), want, "{reason}");
    }
}

/// A stage-side reason that escalates the episode flips it out of
/// `stuck`; an unrecognised reason is never hidden.
#[test]
fn escalation_and_unknown_reasons_are_the_users() {
    assert_eq!(
        AttentionClass::of("rate limited", false),
        AttentionClass::Stuck
    );
    assert_eq!(
        AttentionClass::of("rate limited", true),
        AttentionClass::NeedsYou
    );
    assert_eq!(
        AttentionClass::of("something new", false),
        AttentionClass::NeedsYou
    );
}
