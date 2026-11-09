//! Tests for the watch pass's pure pieces: trust-dialog reads,
//! session-event timing, discovery throttling and the cross-studio
//! `taken`/`blocked` sets.

use backlot_model::event::{ActorIdentity, EventKind, RoleConfigs, StudioMode};
use backlot_model::state::{Attention, RoleState, StudioConfig, StudioState};
use std::collections::BTreeSet;

use crate::watch::{
    DISCOVERY_STOP_MS, SESSION_NOT_FOUND, SESSION_WAIT_MS, TRUST_KEY_GAP_MS, TRUST_SETTLE_MS,
    TrustPace, WORK_SAMPLE_EVERY_MS, discovery_blocked, discovery_due, session_event, taken_ids,
    trust_dialog, trust_key, trust_step, work_due,
};

fn actor(runner: &str) -> ActorIdentity {
    ActorIdentity {
        runner: runner.into(),
        runner_version: None,
        model: "m".into(),
        effort: "high".into(),
        permission_profile: "standard".into(),
    }
}

/// A minimal studio whose roles only need a repo and entries.
fn studio(repo: &str) -> StudioState {
    StudioState {
        studio: Some(StudioConfig {
            title: "t".into(),
            repo: repo.into(),
            mode: StudioMode::Standard,
            roles: RoleConfigs {
                producer: actor("codex"),
                director: actor("codex"),
                writer: actor("codex"),
            },
            candidates: Vec::new(),
            rework_cap: 2,
        }),
        ..StudioState::default()
    }
}

fn live(runner: &str, session: Option<&str>) -> RoleState {
    RoleState {
        live: true,
        actor: Some(actor(runner)),
        session_id: session.map(str::to_string),
        ..RoleState::default()
    }
}

fn raised(role: &str) -> Attention {
    Attention {
        seq: 1,
        role: role.into(),
        reason: SESSION_NOT_FOUND.into(),
        detail: String::new(),
        open: true,
        ts_ms: 0,
    }
}

/// A live post-launch role with no id blocks its (runner,
/// folder) pair — across studios — until it has the id or hit
/// the attention; chosen-id runners and dead roles never block.
#[test]
fn discovery_blocked_while_an_id_is_outstanding() {
    let mut states = std::collections::BTreeMap::new();
    let mut one = studio("/repo");
    one.roles.insert("producer".into(), live("codex", None));
    states.insert("one".into(), one);
    let blocked = discovery_blocked(&states);
    assert!(
        blocked.contains(&("codex".into(), "/repo".into())),
        "{blocked:?}"
    );
    // A second studio's pending role blocks only its own pair.
    let mut two = studio("/other");
    two.roles.insert("director".into(), live("devin", None));
    states.insert("two".into(), two);
    let blocked = discovery_blocked(&states);
    assert_eq!(blocked.len(), 2, "{blocked:?}");
    assert!(blocked.contains(&("devin".into(), "/other".into())));
    // The id arriving — or the attention giving up — frees it.
    states
        .get_mut("one")
        .unwrap()
        .roles
        .get_mut("producer")
        .unwrap()
        .session_id = Some("s1".into());
    let blocked = discovery_blocked(&states);
    assert!(!blocked.contains(&("codex".into(), "/repo".into())));
    states
        .get_mut("one")
        .unwrap()
        .roles
        .get_mut("producer")
        .unwrap()
        .session_id = None;
    states
        .get_mut("one")
        .unwrap()
        .attention
        .push(raised("producer"));
    let blocked = discovery_blocked(&states);
    assert!(!blocked.contains(&("codex".into(), "/repo".into())));
    // A chosen-id runner or a dead role blocks nothing.
    let mut three = studio("/third");
    three.roles.insert("writer".into(), live("claude", None));
    let mut dead = live("codex", None);
    dead.live = false;
    three.roles.insert("actor-t1-c1".into(), dead);
    states.insert("three".into(), three);
    let blocked = discovery_blocked(&states);
    assert_eq!(blocked.len(), 1, "{blocked:?}");
}

/// A look happens at most every five seconds per role, never
/// once the role's attention exists, and never past ten
/// minutes — the injected clock makes the cadence testable.
#[test]
fn discovery_is_throttled_and_stops() {
    let state = StudioState::default();
    // First look always; again only after five seconds.
    assert!(discovery_due("r", &state, 1_000, None, 1_000));
    assert!(!discovery_due("r", &state, 1_000, Some(1_000), 2_000));
    assert!(!discovery_due("r", &state, 1_000, Some(1_000), 5_999));
    assert!(discovery_due("r", &state, 1_000, Some(1_000), 6_000));
    // The role's attention exists: given up, no more looking.
    let mut state = StudioState::default();
    state.attention.push(raised("r"));
    assert!(!discovery_due("r", &state, 61_000, None, 200_000));
    // And nothing looks past the hard stop.
    let state = StudioState::default();
    assert!(!discovery_due(
        "r",
        &state,
        DISCOVERY_STOP_MS + 1,
        None,
        700_000
    ));
}

/// Claimed ids pool across every studio and the ids this pass
/// already recorded — a session found a second ago is never
/// handed out again.
#[test]
fn taken_ids_pool_every_studio_and_the_pass() {
    let mut states = std::collections::BTreeMap::new();
    let mut one = studio("/a");
    one.roles
        .insert("producer".into(), live("codex", Some("s1")));
    states.insert("one".into(), one);
    let mut two = studio("/b");
    two.roles.insert("writer".into(), live("devin", Some("s2")));
    two.roles.insert("actor-t1-c1".into(), live("codex", None));
    states.insert("two".into(), two);
    let found = BTreeSet::from(["s3".to_string()]);
    let taken = taken_ids(&states, &found);
    assert_eq!(
        taken,
        BTreeSet::from(["s1".into(), "s2".into(), "s3".into()])
    );
}

/// A work sample reads the screen at most once a second per role —
/// always the first time, then only when the gap has passed.
#[test]
fn work_sample_is_throttled_to_once_a_second() {
    assert!(work_due(None, 1_000));
    assert!(!work_due(Some(1_000), 1_999));
    assert!(work_due(Some(1_000), 1_000 + WORK_SAMPLE_EVERY_MS));
    // A clock going backwards never re-samples early.
    assert!(!work_due(Some(5_000), 4_000));
}

/// The dialog is its two marker lines together — either alone,
/// and any other prompt, is left alone.
#[test]
fn trust_dialog_needs_both_markers() {
    let screen = "Quick safety check\nDo you trust the files in this folder?\n  Yes, I trust this folder\n  No, exit\n";
    assert!(trust_dialog(screen));
    assert!(!trust_dialog("Yes, I trust this folder\n"));
    assert!(!trust_dialog("Quick safety check\n"));
    assert!(!trust_dialog("Allow edit to src/main.rs? [y/n]\n"));
    assert!(!trust_dialog(""));
}

/// The key follows the `❯`: on `No, exit` it is `down`, on
/// `Yes, I trust this folder` it is `enter`; an option without
/// the cursor, a cursor elsewhere or another prompt sends
/// nothing — Enter only ever goes out on Yes.
#[test]
fn trust_key_follows_the_cursor() {
    let on_no = "Quick safety check\nDo you trust the files in this folder?\n❯ No, exit\n  Yes, I trust this folder\n";
    assert_eq!(trust_key(on_no), Some("down"));
    let on_yes = "Quick safety check\nDo you trust the files in this folder?\n  No, exit\n❯ Yes, I trust this folder\n";
    assert_eq!(trust_key(on_yes), Some("enter"));
    // Options drawn with the cursor elsewhere, or not at all.
    let cursor_elsewhere =
        "Quick safety check\n❯ Continue\n  No, exit\n  Yes, I trust this folder\n";
    assert_eq!(trust_key(cursor_elsewhere), None);
    let no_cursor = "Quick safety check\n  No, exit\n  Yes, I trust this folder\n";
    assert_eq!(trust_key(no_cursor), None);
    // Not the dialog at all.
    assert_eq!(trust_key("❯ allow edit? [y/n]\n"), None);
    assert_eq!(trust_key(""), None);
}

/// The exchange's clock: nothing goes out before the dialog has
/// been seen for two seconds — an early `down` is lost on the real
/// runner — the first key sends at the settle, the next only after
/// the key gap; the state resets when the dialog leaves the screen
/// and a returning dialog settles again.
#[test]
fn trust_step_paces_the_exchange() {
    // First sighting records the clock, sends nothing.
    let (pace, key) = trust_step(None, Some("down"), 1_000);
    assert_eq!(key, None);
    let pace = pace.unwrap();
    assert_eq!(
        pace,
        TrustPace {
            seen_ms: 1_000,
            sent_ms: None
        }
    );
    // Inside the settle: nothing.
    let (p, key) = trust_step(Some(pace), Some("down"), 1_999);
    assert_eq!(key, None);
    assert_eq!(p, Some(pace));
    // At the settle the first key goes out.
    let (p, key) = trust_step(Some(pace), Some("down"), 1_000 + TRUST_SETTLE_MS);
    assert_eq!(key, Some("down"));
    let sent = p.unwrap().sent_ms.unwrap();
    assert_eq!(sent, 1_000 + TRUST_SETTLE_MS);
    // A key inside the gap waits; after it, whatever the screen asks.
    let (p, key) = trust_step(p, Some("enter"), sent + TRUST_KEY_GAP_MS - 1);
    assert_eq!(key, None);
    assert!(p.is_some());
    let (_, key) = trust_step(p, Some("enter"), sent + TRUST_KEY_GAP_MS);
    assert_eq!(key, Some("enter"));
    // The dialog gone resets the exchange.
    let (pace, key) = trust_step(p, None, sent + 9_999);
    assert_eq!((pace, key), (None, None));
    // And a returning dialog settles again from scratch.
    let (pace, key) = trust_step(None, Some("down"), sent + 10_000);
    assert_eq!(key, None);
    assert!(pace.is_some());
}

/// A found id records `role.session`; under the wait nothing
/// happens; past it, one attention — and never a second, even
/// after the first resolved.
#[test]
fn session_event_timing() {
    let state = StudioState::default();
    let found = session_event("producer", "codex", Some("s1".into()), &state, 5_000);
    assert_eq!(
        found,
        Some(EventKind::RoleSession {
            role: "producer".into(),
            session_id: "s1".into(),
        })
    );
    // Inside the wait a miss stays quiet.
    assert_eq!(
        session_event("producer", "codex", None, &state, SESSION_WAIT_MS - 1),
        None
    );
    // Past it the attention raises, once.
    let raised = session_event("producer", "codex", None, &state, SESSION_WAIT_MS);
    assert!(
        matches!(&raised, Some(EventKind::AttentionRaised { reason, .. }) if reason == SESSION_NOT_FOUND),
        "{raised:?}"
    );
    // A prior attention for the same role and reason — open or
    // resolved — means never again; a different role's does not
    // count.
    let mut state = StudioState::default();
    for (role, open) in [("producer", true), ("other", false)] {
        state.attention.push(Attention {
            seq: 1,
            role: role.into(),
            reason: SESSION_NOT_FOUND.into(),
            detail: String::new(),
            open,
            ts_ms: 0,
        });
    }
    assert_eq!(
        session_event("producer", "codex", None, &state, SESSION_WAIT_MS + 1),
        None
    );
    state.attention[0].open = false;
    assert_eq!(
        session_event("producer", "codex", None, &state, SESSION_WAIT_MS + 1),
        None,
        "resolved stays answered"
    );
    assert!(
        session_event("other", "codex", None, &state, SESSION_WAIT_MS + 1).is_none(),
        "a different role's resolved attention does not fire again"
    );
    assert!(session_event("director", "codex", None, &state, SESSION_WAIT_MS + 1).is_some());
}
