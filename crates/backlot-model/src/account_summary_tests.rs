//! Backward-compatibility and quiet-diff tests for account summary and attention fields.

use serde::Deserialize;

use crate::event::{Event, EventKind};
use crate::hollywood_summary::HollywoodSummary;
use crate::state::{StudioState, apply};
use crate::summary::{AccountRoleSummary, AccountSummary, QuotaWindow, SUMMARY_SCHEMA, Summary};

/// Simulated v0.4.20 reader ignoring additive Part 1 account fields.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct FrozenV0420Summary {
    schema: String,
    updated_ms: u64,
    #[serde(default)]
    studios: Vec<FrozenV0420Studio>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct FrozenV0420Studio {
    id: String,
    #[serde(default)]
    roles: Vec<FrozenV0420Role>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct FrozenV0420Role {
    role: String,
    state: String,
}

#[test]
fn attention_raised_account_and_reset_round_trip_and_old_logs_default_none() {
    let old_json = r#"{"v":1,"seq":4,"ts_ms":1000,"by":{"role":"stage"},"kind":"attention.raised","role":"actor-t1-c1","reason":"stalled","detail":"no output"}"#;
    let old_ev: Event = serde_json::from_str(old_json).unwrap();
    match &old_ev.kind {
        EventKind::AttentionRaised {
            account,
            reset_at_ms,
            ..
        } => {
            assert_eq!(*account, None);
            assert_eq!(*reset_at_ms, None);
        }
        other => panic!("unexpected {other:?}"),
    }
    let reserialized = serde_json::to_string(&old_ev).unwrap();
    assert!(!reserialized.contains("\"account\""));
    assert!(!reserialized.contains("\"reset_at_ms\""));

    let new_ev = Event {
        v: 1,
        seq: 5,
        ts_ms: 2000,
        by: crate::EventBy::role("stage"),
        kind: EventKind::AttentionRaised {
            role: "actor-t2-c1".into(),
            reason: "rate limited".into(),
            detail: "waiting until 14:10".into(),
            needs_user: false,
            account: Some("claude:default".into()),
            reset_at_ms: Some(1_795_669_200_000),
        },
    };
    let line = serde_json::to_string(&new_ev).unwrap();
    let mut state = StudioState::default();
    let parsed: Event = serde_json::from_str(&line).unwrap();
    apply(&mut state, &parsed);
    let attn = state.attention.last().unwrap();
    assert_eq!(attn.account.as_deref(), Some("claude:default"));
    assert_eq!(attn.reset_at_ms, Some(1_795_669_200_000));
}

#[test]
fn summary_accounts_stay_quiet_across_same_as_and_old_readers_ignore_them() {
    let a = Summary {
        schema: SUMMARY_SCHEMA.into(),
        updated_ms: 10_000,
        studios: Vec::new(),
        accounts: vec![AccountSummary {
            id: "claude:default".into(),
            runner: "claude".into(),
            source: "default".into(),
            active: 2,
            working: 1,
            max_active: None,
            room: None,
            queued: 0,
            stalled: true,
            last_stall_ms: Some(9_000),
            reset_at_ms: Some(27_000),
            window: Some(QuotaWindow::Short),
            roles: vec![AccountRoleSummary {
                studio: "acme-shop".into(),
                role: "actor-t1-c1".into(),
                model: "sonnet-5".into(),
                active: true,
                working: true,
                held: false,
                stalled: true,
            }],
        }],
        hollywood: Some(HollywoodSummary {
            state: "live".into(),
            runner: Some("claude".into()),
            model: Some("opus-5-5".into()),
            effort: None,
            terminal: Some("hw_1".into()),
            work: None,
            pending: false,
            doing: None,
            attention: None,
            attention_user: false,
            attention_detail: None,
            can_answer_prompt: true,
            headline: None,
            restarted_ms: None,
            headline_seq: None,
            cards: Vec::new(),
            exchange: Vec::new(),
            queue: None,
            handoff: None,
            account: Some("claude:default".into()),
        }),
        backlot_version: None,
        backlot_build: None,
    };
    let mut b = a.clone();
    b.updated_ms = 25_000;
    assert!(a.same_as(&b));

    b.accounts[0].stalled = false;
    assert!(!a.same_as(&b));

    let json = serde_json::to_string(&a).unwrap();
    let frozen: FrozenV0420Summary = serde_json::from_str(&json).unwrap();
    assert_eq!(frozen.schema, SUMMARY_SCHEMA);

    // Old `backlot/1` summary without `accounts` or `role.account` deserializes cleanly.
    let old_summary_json = r#"{"schema":"backlot/1","updated_ms":1,"studios":[]}"#;
    let parsed_old: Summary = serde_json::from_str(old_summary_json).unwrap();
    assert!(parsed_old.accounts.is_empty());
}
