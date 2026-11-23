//! The attention classes: every reason the stage can raise, plus the
//! unknown-reason and escalation rules the client relies on.

use backlot_model::attention::AttentionClass;

use crate::connerr::CONNECTION_ERROR;
use crate::ratelimit::RATE_LIMITED;
use crate::reasons;

/// Every stage reason has exactly one class, and the table is the
/// mapping in `docs/studio/panel.md`.
#[test]
fn every_stage_reason_is_classified() {
    use AttentionClass::{Info, NeedsYou, Stuck};
    let expected = |reason: &str| match reason {
        reasons::STALLED | RATE_LIMITED | CONNECTION_ERROR => Stuck,
        reasons::SESSION_NOT_FOUND | reasons::CURSOR_DIR_OPEN | reasons::XDG_CONFIG_HOME_SET => {
            Info
        }
        _ => NeedsYou,
    };
    for reason in reasons::ALL {
        assert_eq!(
            AttentionClass::of(reason, false),
            expected(reason),
            "{reason}"
        );
    }
}

/// An escalated episode stops being `stuck`; an unknown reason is never
/// hidden from the person.
#[test]
fn escalation_wins_and_unknowns_need_you() {
    for reason in [RATE_LIMITED, CONNECTION_ERROR] {
        assert_eq!(AttentionClass::of(reason, true), AttentionClass::NeedsYou);
    }
    assert_eq!(
        AttentionClass::of("not a reason anyone wrote", false),
        AttentionClass::NeedsYou
    );
}
