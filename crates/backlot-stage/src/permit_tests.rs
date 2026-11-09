//! The D9 pure pieces: which key a decision sends per runner, the
//! open-episode and pending-approval lookups, the per-episode
//! dedup, and who a permission attention notifies.

use std::path::Path;

use backlot_model::event::{ApprovalDecision, PERMISSION_PROMPT};
use backlot_model::state::{ApprovalRecord, Attention, StudioState};

use crate::notices;
use crate::permit::{
    approval_keys, director_notice, episode_raised, is_actor, open_episode, pending_approval,
};

fn attn(seq: u64, role: &str, open: bool, ts_ms: u64) -> Attention {
    Attention {
        seq,
        role: role.into(),
        reason: PERMISSION_PROMPT.into(),
        detail: format!("screen saved to attention/{seq}.txt"),
        open,
        ts_ms,
    }
}

/// `allow` presses enter and `deny` presses esc on the runners whose
/// pickers share that convention; an unmeasured runner answers
/// nothing — the attention stays open for the user.
#[test]
fn keys_per_runner() {
    for runner in ["fake", "claude", "codex"] {
        assert_eq!(
            approval_keys(runner, ApprovalDecision::Allow),
            Some("enter"),
            "{runner}"
        );
        assert_eq!(
            approval_keys(runner, ApprovalDecision::Deny),
            Some("esc"),
            "{runner}"
        );
    }
    for runner in ["devin", "opencode", "unknown"] {
        assert_eq!(
            approval_keys(runner, ApprovalDecision::Allow),
            None,
            "{runner}"
        );
        assert_eq!(
            approval_keys(runner, ApprovalDecision::Deny),
            None,
            "{runner}"
        );
    }
}

/// `open_episode` finds only an open `permission prompt` for the
/// role — a cleared attention or another reason does not count.
#[test]
fn open_episode_is_open_and_reasoned() {
    let mut state = StudioState::default();
    assert!(open_episode(&state, "actor-t1-c1").is_none());
    state.attention.push(attn(4, "actor-t1-c1", false, 10));
    assert!(open_episode(&state, "actor-t1-c1").is_none());
    state.attention.push(attn(6, "actor-t1-c1", true, 20));
    assert_eq!(open_episode(&state, "actor-t1-c1").unwrap().seq, 6);
    assert!(open_episode(&state, "director").is_none());
    let mut other = attn(8, "writer", true, 30);
    other.reason = "rate limit".into();
    state.attention.push(other);
    assert!(open_episode(&state, "writer").is_none());
}

/// The latest recorded decision is the one the stage acts on.
#[test]
fn pending_approval_is_latest() {
    let mut state = StudioState::default();
    assert!(pending_approval(&state, "actor-t1-c1").is_none());
    state.approvals.push(ApprovalRecord {
        seq: 3,
        role: "actor-t1-c1".into(),
        decision: ApprovalDecision::Allow,
        ts_ms: 1,
    });
    state.approvals.push(ApprovalRecord {
        seq: 5,
        role: "actor-t1-c1".into(),
        decision: ApprovalDecision::Deny,
        ts_ms: 2,
    });
    assert_eq!(
        pending_approval(&state, "actor-t1-c1").unwrap().decision,
        ApprovalDecision::Deny
    );
    assert!(pending_approval(&state, "writer").is_none());
}

/// An episode is raised once: an attention at or after the episode
/// start covers it, open or cleared; one from before it does not.
#[test]
fn episode_dedups_on_attention_time() {
    let mut state = StudioState::default();
    state.attention.push(attn(4, "actor-t1-c1", false, 100));
    // Episode starting after the old attention: not covered.
    assert!(!episode_raised(&state, "actor-t1-c1", 200));
    state.attention.push(attn(6, "actor-t1-c1", true, 250));
    assert!(episode_raised(&state, "actor-t1-c1", 200));
    // A different role's attention never covers this episode.
    state.attention.push(attn(8, "writer", true, 300));
    assert!(episode_raised(&state, "actor-t1-c1", 200));
    assert!(!episode_raised(&state, "writer", 400));
}

/// Only `actor-<task>-c<n>` keys are actors — a director or writer
/// at a prompt is the user's concern.
#[test]
fn actor_keys() {
    assert!(is_actor("actor-t1-c1"));
    assert!(is_actor("actor-big-job-c12"));
    assert!(!is_actor("director"));
    assert!(!is_actor("writer"));
    assert!(!is_actor("actor-")); // empty task
}

/// The director is told about an actor's episode with the saved
/// screen's path and the approve command; a non-actor gets none.
#[test]
fn director_notice_is_actor_only() {
    let a = attn(9, "actor-t1-c1", true, 10);
    let body = director_notice(&a, "/state/studios/acme-shop/attention/9.txt").unwrap();
    assert!(body.contains("actor-t1-c1"));
    assert!(body.contains("/state/studios/acme-shop/attention/9.txt"));
    assert!(body.contains("backlot approve --person actor-t1-c1 --decision allow"));
    let w = attn(11, "writer", true, 10);
    assert!(director_notice(&w, "/state/studios/acme-shop/attention/11.txt").is_none());
}

/// Owed notices: an actor's permission attention owes the director
/// one notice, deduped on `source_seq`; a non-actor's owes none.
#[test]
fn attention_notice_owed_once() {
    let mut state = StudioState::default();
    state.attention.push(attn(7, "actor-t1-c1", true, 10));
    state.attention.push(attn(8, "writer", true, 10));
    let dir = Path::new("/state/studios/acme-shop");
    let got = notices::owed(&state, dir);
    let actor_notices: Vec<_> = got.iter().filter(|n| n.source_seq == 7).collect();
    assert_eq!(actor_notices.len(), 1);
    assert_eq!(actor_notices[0].to, "director");
    assert!(actor_notices[0].body.contains("attention/7.txt"));
    assert!(got.iter().all(|n| n.source_seq != 8));
    // Once a message carries that source_seq, nothing is owed.
    state.messages.push(backlot_model::state::Message {
        seq: 9,
        from: "stage".into(),
        to: "director".into(),
        file: Some("messages/9-attention.md".into()),
        purpose: None,
        source_seq: Some(7),
        delivered: false,
    });
    assert!(notices::owed(&state, dir).iter().all(|n| n.source_seq != 7));
}
