//! The notices Hollywood is owed (R23): which studio facts become
//! a notice, which label each carries, how `source_seq` dedups a
//! restart, and when a queued notice drops instead of sending.

use std::collections::BTreeMap;

use backlot_model::event::{CANNOT_RESUME, PERMISSION_PROMPT, STALLED, Verdict};
use backlot_model::hollywood::{self, CardState, HMessage, HollywoodKind, HollywoodState};
use backlot_model::state::{Attention, FinishRecord, ReviewRecord, RoleState, StudioState};
use backlot_model::testkit as tk;

use crate::hnotice::{self, HNotice};

fn attention(seq: u64, role: &str, reason: &str, needs_user: bool) -> Attention {
    Attention {
        seq,
        role: role.into(),
        reason: reason.into(),
        detail: "d".into(),
        open: true,
        ts_ms: 0,
        needs_user,
        cleared_reason: None,
        account: None,
        reset_at_ms: None,
    }
}

fn one(studios: &[(&str, StudioState)]) -> Vec<HNotice> {
    let map: BTreeMap<String, StudioState> = studios
        .iter()
        .map(|(n, s)| (n.to_string(), s.clone()))
        .collect();
    hnotice::owed(&map, &BTreeMap::new(), &HollywoodState::default(), 0)
}

/// An open `Stuck` actor prompt stays silent when fresh and reaches
/// Hollywood past the threshold, naming its studio and raising seq.
#[test]
fn an_attention_names_its_studio() {
    let mut s = StudioState::default();
    s.attention
        .push(attention(7, "actor-t1-c1", PERMISSION_PROMPT, false));
    assert!(one(&[("acme-shop", s.clone())]).is_empty());

    let map: BTreeMap<String, StudioState> = [("acme-shop".to_string(), s)].into();
    let n = hnotice::owed(
        &map,
        &BTreeMap::new(),
        &HollywoodState::default(),
        hnotice::stuck_threshold_ms(),
    );
    assert_eq!(n.len(), 1);
    assert_eq!(n[0].label, "attention");
    assert_eq!(n[0].studio.as_deref(), Some("acme-shop"));
    assert_eq!(n[0].source_seq, Some(7));
    assert!(n[0].body.contains("acme-shop"), "{}", n[0].body);
}

/// A prompt only the user may answer — `needs_user` or a role with
/// no task — is the `prompt` notice immediately; an overdue stall
/// nobody routes is `stalled`.
#[test]
fn the_prompt_and_unrouted_stall_labels() {
    let mut s = StudioState::default();
    s.attention
        .push(attention(3, "producer", PERMISSION_PROMPT, true));
    s.attention.push(attention(4, "producer", STALLED, false));
    let n = hnotice::owed(
        &[("acme".to_string(), s)].into(),
        &BTreeMap::new(),
        &HollywoodState::default(),
        hnotice::stuck_threshold_ms(),
    );
    let labels: Vec<&str> = n.iter().map(|x| x.label).collect();
    assert_eq!(labels, vec!["prompt", "stalled"]);

    // A `cannot resume` the producer owns is `NeedsYou` at once.
    let mut s = StudioState::default();
    s.attention
        .push(attention(5, "producer", CANNOT_RESUME, false));
    let n = one(&[("acme", s)]);
    assert_eq!(n[0].label, "stalled");
}

/// An overdue producer stall or `cannot resume` leads Hollywood's
/// `stalled` notice, and duplicate `(studio, role, label)` attentions
/// coalesce to the newest one.
#[test]
fn a_producer_down_leads_hollywoods_notice() {
    let mut s = StudioState::default();
    s.roles.insert(
        "director".into(),
        RoleState {
            live: true,
            ..RoleState::default()
        },
    );
    s.attention.push(attention(4, "producer", STALLED, false));
    let n = hnotice::owed(
        &[("acme".to_string(), s.clone())].into(),
        &BTreeMap::new(),
        &HollywoodState::default(),
        hnotice::stuck_threshold_ms(),
    );
    assert_eq!(n.len(), 1);
    assert_eq!(n[0].label, "stalled");
    assert!(
        n[0].body
            .contains("# acme: producer stalled — needs attention"),
        "{}",
        n[0].body
    );

    s.attention
        .push(attention(5, "producer", CANNOT_RESUME, false));
    let n = hnotice::owed(
        &[("acme".to_string(), s)].into(),
        &BTreeMap::new(),
        &HollywoodState::default(),
        hnotice::stuck_threshold_ms(),
    );
    assert_eq!(
        n.len(),
        1,
        "same (studio, role, label) must coalesce: {n:?}"
    );
    assert_eq!(n[0].label, "stalled");
    assert_eq!(n[0].source_seq, Some(5));
    assert!(
        n[0].body
            .contains("# acme: producer cannot resume — needs attention"),
        "{}",
        n[0].body
    );
}

/// `studio.finished` and a stage error owe notices; `accepted` stays
/// passive context and never wakes Hollywood. When finished, prior studio
/// errors/attentions are suppressed.
#[test]
fn finished_accepted_and_failed() {
    let mut s = StudioState::default();
    let mut task = tk::base_state().tasks["t1"].clone();
    task.reviews.push(ReviewRecord {
        delivery: 1,
        reviewer: "director".into(),
        verdict: Verdict::Accepted,
        conclusion: "done".into(),
        findings: Vec::new(),
        upstream_changes: Vec::new(),
        seq: 12,
        ts_ms: 0,
    });
    s.tasks.insert("t1".into(), task);
    let mut errors = BTreeMap::new();
    errors.insert("acme".to_string(), "ledger: corrupt".to_string());
    let map: BTreeMap<String, StudioState> = [("acme".to_string(), s.clone())].into();
    let h = HollywoodState::default();
    let n = hnotice::owed(&map, &errors, &h, 0);
    let labels: Vec<&str> = n.iter().map(|x| x.label).collect();
    assert_eq!(labels, vec!["failed"], "{labels:?}");
    assert_eq!(n[0].source_seq, None);
    assert_eq!(n[0].token.as_deref(), Some("ledger: corrupt"));

    let passive = hnotice::passive_items(&map, &h, &Default::default());
    assert_eq!(passive.len(), 1);
    assert_eq!(passive[0].text, "acme t1 accepted");

    let mut finished_s = s;
    finished_s.finished = Some(FinishRecord {
        seq: 9,
        reason: "all accepted".into(),
        ts_ms: 0,
    });
    let map: BTreeMap<String, StudioState> = [("acme".to_string(), finished_s)].into();
    let n = hnotice::owed(&map, &errors, &h, 0);
    let labels: Vec<&str> = n.iter().map(|x| x.label).collect();
    assert_eq!(labels, vec!["finished"], "{labels:?}");
}

/// `recovered` and `card.failed`/`stale`/`cancelled` owe wake-up notices;
/// `card.applied` emits nothing and `card.done` stays passive context.
#[test]
fn the_recovered_word_and_card_outcomes() {
    let mut h = HollywoodState::default();
    h.role.recovered_seq = Some(5);
    hollywood::apply(&mut h, &tk::hev(1, "hollywood", tk::hpropose("t1")));
    hollywood::apply(
        &mut h,
        &tk::hev(2, "user", HollywoodKind::CardApplied { card: 1 }),
    );
    let n = hnotice::owed(&BTreeMap::new(), &BTreeMap::new(), &h, 0);
    let labels: Vec<&str> = n.iter().map(|x| x.label).collect();
    assert_eq!(labels, vec!["recovered"], "{labels:?}");
    assert_eq!(n[0].source_seq, Some(5));

    hollywood::apply(
        &mut h,
        &tk::hev(
            3,
            "stage",
            HollywoodKind::CardDone {
                card: 1,
                note: String::new(),
            },
        ),
    );
    let n = hnotice::owed(&BTreeMap::new(), &BTreeMap::new(), &h, 0);
    assert_eq!(n.iter().filter(|x| x.label == "card").count(), 0);
    assert_eq!(h.cards[&1].state, CardState::Done);
    let passive = hnotice::passive_items(&BTreeMap::new(), &h, &Default::default());
    assert_eq!(passive.len(), 1);
    assert_eq!(passive[0].text, "c1 done");

    hollywood::apply(&mut h, &tk::hev(4, "hollywood", tk::hpropose("t1")));
    hollywood::apply(
        &mut h,
        &tk::hev(5, "user", HollywoodKind::CardApplied { card: 4 }),
    );
    hollywood::apply(
        &mut h,
        &tk::hev(
            6,
            "stage",
            HollywoodKind::CardFailed {
                card: 4,
                reason: "nope".into(),
            },
        ),
    );
    let n = hnotice::owed(&BTreeMap::new(), &BTreeMap::new(), &h, 0);
    let card_notices: Vec<&HNotice> = n.iter().filter(|x| x.label == "card").collect();
    assert_eq!(card_notices.len(), 1);
    assert_eq!(card_notices[0].source_seq, Some(6));
    assert!(
        card_notices[0].body.contains("failed"),
        "{}",
        card_notices[0].body
    );
}

/// A notice already in the inbox — queued, delivered or dropped —
/// is never owed again: `source_seq` is the dedup key across a
/// stage restart.
#[test]
fn told_notices_are_never_owed_again() {
    let mut s = StudioState::default();
    s.attention
        .push(attention(7, "actor-t1-c1", PERMISSION_PROMPT, false));
    let map: BTreeMap<String, StudioState> = [("acme".to_string(), s)].into();
    for (delivered, dropped) in [(false, false), (true, false), (false, true)] {
        let mut h = HollywoodState::default();
        h.messages.push(HMessage {
            seq: 3,
            ts_ms: 0,
            from: "stage".into(),
            to: "hollywood".into(),
            studio: Some("acme".into()),
            text: None,
            file: Some("messages/3-attention.md".into()),
            source_seq: Some(7),
            delivered,
            dropped,
            seen: false,
        });
        assert!(
            hnotice::owed(&map, &BTreeMap::new(), &h, 0).is_empty(),
            "delivered={delivered} dropped={dropped} was owed again"
        );
    }
    // A `failed` dedups on its error text, the same one told stays
    // told; a changed error is a new notice. The attention's own
    // told-marker rides along — it stays told as well.
    let mut errors = BTreeMap::new();
    errors.insert("acme".to_string(), "e1".to_string());
    let mut h = HollywoodState::default();
    h.messages.push(HMessage {
        seq: 3,
        ts_ms: 0,
        from: "stage".into(),
        to: "hollywood".into(),
        studio: Some("acme".into()),
        text: None,
        file: Some("messages/3-attention.md".into()),
        source_seq: Some(7),
        delivered: true,
        dropped: false,
        seen: false,
    });
    h.messages.push(HMessage {
        seq: 4,
        ts_ms: 0,
        from: "stage".into(),
        to: "hollywood".into(),
        studio: Some("acme".into()),
        text: Some("e1".into()),
        file: Some("messages/4-failed.md".into()),
        source_seq: None,
        delivered: false,
        dropped: false,
        seen: false,
    });
    assert!(hnotice::owed(&map, &errors, &h, 0).is_empty());
    errors.insert("acme".to_string(), "e2".to_string());
    let owed = hnotice::owed(&map, &errors, &h, 0);
    assert_eq!(owed.len(), 1);
    assert_eq!(owed[0].label, "failed");
}

/// `info` attentions never wake Hollywood; actionable attentions carry
/// class, age, and snapshot warning.
#[test]
fn an_attention_notice_carries_its_class_and_age() {
    let mut s = StudioState::default();
    let mut info = attention(7, "producer", "session id not found", false);
    info.ts_ms = 0;
    info.detail = "no conversation recorded".into();
    s.attention.push(info);
    let map: BTreeMap<String, StudioState> = [("acme".to_string(), s)].into();
    let n = hnotice::owed(
        &map,
        &BTreeMap::new(),
        &HollywoodState::default(),
        60 * 60 * 1000,
    );
    assert!(n.is_empty(), "info attention must not emit: {n:?}");

    // A snapshot attention names the screen's time and stale warning.
    let mut s = StudioState::default();
    let mut snap = attention(8, "producer", PERMISSION_PROMPT, true);
    snap.ts_ms = 0;
    snap.detail = "Bash: ls; screen saved to attention/8.txt".into();
    s.attention.push(snap);
    let map: BTreeMap<String, StudioState> = [("acme".to_string(), s)].into();
    let n = hnotice::owed(
        &map,
        &BTreeMap::new(),
        &HollywoodState::default(),
        90 * 60 * 1000,
    );
    assert!(
        n[0].body.contains("fact: needs you · raised 1h ago"),
        "{}",
        n[0].body
    );
    assert!(n[0].body.contains("screen at "), "{}", n[0].body);
    assert!(n[0].body.contains("may be stale"), "{}", n[0].body);
}

#[test]
fn director_handled_prompt_waits_for_threshold_while_needs_you_fires_at_once() {
    let t0 = 1_000_000;
    let threshold_ms = u64::from(backlot_model::event::default_stall_limit_s()) * 1000;
    let mut s = StudioState::default();
    s.roles.insert(
        "director".into(),
        RoleState {
            live: true,
            ..RoleState::default()
        },
    );
    let mut prompt = attention(7, "actor-t1-c1", PERMISSION_PROMPT, false);
    prompt.ts_ms = t0;
    s.attention.push(prompt);
    let map: BTreeMap<String, StudioState> = [("acme-shop".to_string(), s.clone())].into();

    // Fresh director-handled prompt emits no Hollywood notice.
    let fresh = hnotice::owed(
        &map,
        &BTreeMap::new(),
        &HollywoodState::default(),
        t0 + 60_000,
    );
    assert!(
        fresh.is_empty(),
        "fresh director prompt should hold: {fresh:?}"
    );

    // Same prompt past the threshold reaches Hollywood with wait duration and why.
    let overdue = hnotice::owed(
        &map,
        &BTreeMap::new(),
        &HollywoodState::default(),
        t0 + threshold_ms,
    );
    assert_eq!(overdue.len(), 1, "{overdue:?}");
    assert!(
        overdue[0].body.contains("10m") && overdue[0].body.contains("not answering"),
        "{}",
        overdue[0].body
    );

    // Rate-limited director past the threshold names rate limit as why.
    let mut s_rl = s.clone();
    let mut rl = attention(8, "director", crate::ratelimit::RATE_LIMITED, false);
    rl.ts_ms = t0;
    s_rl.attention.push(rl);
    let map_rl: BTreeMap<String, StudioState> = [("acme-shop".to_string(), s_rl)].into();
    let overdue_rl = hnotice::owed(
        &map_rl,
        &BTreeMap::new(),
        &HollywoodState::default(),
        t0 + threshold_ms,
    );
    assert!(
        overdue_rl.iter().any(|n| n.source_seq == Some(7)
            && n.body.contains("10m")
            && n.body.contains("rate limited")),
        "{overdue_rl:?}"
    );

    // NeedsYou prompt sends a notice at once at ts_ms.
    let mut s_user = StudioState::default();
    let mut urgent = attention(9, "actor-t1-c1", PERMISSION_PROMPT, true);
    urgent.ts_ms = t0;
    s_user.attention.push(urgent);
    let map_user: BTreeMap<String, StudioState> = [("acme-shop".to_string(), s_user)].into();
    let immediate = hnotice::owed(&map_user, &BTreeMap::new(), &HollywoodState::default(), t0);
    assert_eq!(immediate.len(), 1, "{immediate:?}");
    assert_eq!(immediate[0].label, "prompt");
}
