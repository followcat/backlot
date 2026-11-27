//! Opening design tests for `nudge`, `notices`, and `stale`.

use std::collections::BTreeSet;
use std::path::Path;

use backlot_model::design::{DesignGate, DesignSlot, SignMode};
use backlot_model::event::{Event, EventKind, PlanKind};
use backlot_model::state::{Message, StudioState, replay};
use backlot_model::testkit::*;

use crate::notices;
use crate::nudge::{self, Move};
use crate::stale::stale_reason;

const DIR: &str = "/state/studios/acme-shop";

fn dir() -> &'static Path {
    Path::new(DIR)
}

fn studio_required() -> EventKind {
    let mut k = studio_kind();
    if let EventKind::StudioCreated { design_gate, .. } = &mut k {
        *design_gate = DesignGate::Required;
    }
    k
}

fn req(seq: u64, version: u32) -> Event {
    ev(
        seq,
        "producer",
        EventKind::ReqVersion {
            version,
            file: format!("req/v{version}.md"),
            summary: format!("req v{version}"),
        },
    )
}

fn opened(seq: u64, role: &str, version: u32, round: u32, suggest_skip: bool) -> Event {
    ev(
        seq,
        role,
        EventKind::DesignOpened {
            version,
            round,
            suggest_skip,
            reason: suggest_skip.then(|| "small fix".into()),
            doc_file: format!("design/v{version}.md"),
        },
    )
}

fn entry(seq: u64, role: &str, version: u32, round: u32, slot: DesignSlot) -> Event {
    ev(
        seq,
        role,
        EventKind::DesignEntry {
            version,
            round,
            slot,
            file: format!("design/v{version}/r{round}-{seq}.md"),
            source: None,
            refs: if slot == DesignSlot::Supplement {
                vec![seq - 1]
            } else {
                Vec::new()
            },
            questions: Vec::new(),
            checks: Vec::new(),
        },
    )
}

fn signed(seq: u64, version: u32, round: u32, mode: SignMode) -> Event {
    ev(
        seq,
        "user",
        EventKind::DesignSigned {
            version,
            round,
            mode,
            doc_file: if mode == SignMode::Approved {
                format!("design/v{version}-final.md")
            } else {
                String::new()
            },
            note: Some("ship it".into()),
        },
    )
}

fn publish(seq: u64, kind: PlanKind, version: u32) -> Event {
    ev(
        seq,
        "writer",
        EventKind::PlanPublished {
            plan_kind: kind,
            file: format!("plan/v{version}-{seq}.md"),
            requirement_version: version,
        },
    )
}

fn wanted_set(state: &StudioState) -> BTreeSet<String> {
    backlot_model::wanted::wanted_roles(state)
        .into_iter()
        .map(|w| w.role)
        .collect()
}

#[test]
fn waiting_user_and_suggest_skip_are_never_nudged_while_role_slots_nudge() {
    let moves_at =
        |s: &StudioState, ms: u64| nudge::plan(s, &wanted_set(s), |_| Some(0), |_| None, ms);
    let mut events = vec![
        ev(1, "user", studio_required()),
        req(2, 1),
        ev(3, "stage", role_started("producer")),
        ev(4, "stage", role_started("director")),
    ];
    let s_idle = replay(&events);
    let moves = moves_at(&s_idle, 200_000);
    assert!(
        matches!(moves.as_slice(), [Move::Nudge { step }] if step.owner == "producer" && step.label == "open design"),
        "{moves:?}"
    );

    // SuggestSkip: owner == user -> never nudged or escalated.
    let s_skip = replay(&[
        ev(1, "user", studio_required()),
        req(2, 1),
        ev(3, "stage", role_started("producer")),
        opened(4, "producer", 1, 0, true),
    ]);
    assert!(moves_at(&s_skip, 10_000_000).is_empty());

    // WaitingProposal -> writer; WaitingBuildability -> director; WaitingSummary -> producer.
    events.push(opened(5, "producer", 1, 1, false));
    events.push(ev(6, "stage", role_started("writer")));
    let s_prop = replay(&events);
    assert!(
        matches!(moves_at(&s_prop, 200_000).as_slice(), [Move::Nudge { step }] if step.owner == "writer" && step.label == "write design proposal")
    );

    events.push(entry(7, "writer", 1, 1, DesignSlot::Proposal));
    let s_build = replay(&events);
    assert!(
        matches!(moves_at(&s_build, 200_000).as_slice(), [Move::Nudge { step }] if step.owner == "director" && step.label == "review buildability")
    );

    events.push(entry(8, "director", 1, 1, DesignSlot::Buildability));
    let s_sum = replay(&events);
    assert!(
        matches!(moves_at(&s_sum, 200_000).as_slice(), [Move::Nudge { step }] if step.owner == "producer" && step.label == "summarize design round")
    );

    // WaitingUser: nobody is ever nudged or escalated regardless of elapsed idle time.
    events.push(entry(9, "producer", 1, 1, DesignSlot::Summary));
    let s_wait = replay(&events);
    let moves = moves_at(&s_wait, 10_000_000);
    assert!(moves.is_empty(), "WaitingUser must never nudge: {moves:?}");

    // Stale design-open nudge drops once SuggestSkip or WaitingProposal lands.
    let stale_open_nudge = Message {
        seq: 50,
        from: "stage".into(),
        to: "producer".into(),
        file: Some("messages/2-nudge.md".into()),
        purpose: Some("nudge".into()),
        source_seq: Some(2),
        delivered: false,
        dropped: false,
        dropped_reason: None,
        ts_ms: 0,
        delivered_ms: None,
    };
    assert_eq!(stale_reason(&s_skip, &stale_open_nudge), Some("step done"));
    assert_eq!(stale_reason(&s_prop, &stale_open_nudge), Some("step done"));
}

#[test]
fn notices_drive_opening_design_rounds_and_sign_off_paths() {
    let find_notice = |st: &StudioState, to: &str, seq: u64| {
        notices::owed(st, dir())
            .into_iter()
            .find(|n| n.to == to && n.source_seq == seq)
            .unwrap_or_else(|| panic!("missing notice for {to} at seq {seq}"))
    };
    // 1. req v1 with gate required -> producer gets design-open, director gets req (wait, no tasks).
    let mut events = vec![ev(1, "user", studio_required()), req(2, 1)];
    let st_idle = replay(&events);
    let p_open = find_notice(&st_idle, "producer", 2);
    assert!(p_open.body.contains("backlot design open") && p_open.body.contains("--suggest-skip"));
    let d_req = find_notice(&st_idle, "director", 2);
    assert!(d_req.body.contains("writer") && d_req.body.contains("do not create"));

    // 2-5. DesignOpened -> Proposal -> Buildability -> Summary -> Supplement notices.
    events.push(opened(3, "producer", 1, 1, false));
    assert!(
        find_notice(&replay(&events), "writer", 3)
            .body
            .contains("--slot proposal")
    );
    events.push(entry(4, "writer", 1, 1, DesignSlot::Proposal));
    assert!(
        find_notice(&replay(&events), "director", 4)
            .body
            .contains("--slot buildability")
    );
    events.push(entry(5, "director", 1, 1, DesignSlot::Buildability));
    assert!(
        find_notice(&replay(&events), "producer", 5)
            .body
            .contains("--slot summary")
    );
    events.push(entry(6, "producer", 1, 1, DesignSlot::Summary));
    events.push(entry(7, "writer", 1, 1, DesignSlot::Supplement));
    assert!(
        find_notice(&replay(&events), "producer", 7)
            .body
            .contains("supplement")
    );

    // 6. DesignSigned(Approved) -> writer notified to derive plan + acceptance citing design sections.
    events.push(signed(8, 1, 1, SignMode::Approved));
    let w_signed = find_notice(&replay(&events), "writer", 8);
    assert!(
        w_signed.body.contains("design/v1-final.md")
            && w_signed.body.contains("backlot plan publish")
            && w_signed.body.contains("design v1 §"),
        "{}",
        w_signed.body
    );

    // 7. DesignSigned(Skipped) without existing plan -> writer notified to write plan.
    let skip_no_plan = replay(&[
        ev(1, "user", studio_required()),
        req(2, 1),
        opened(3, "producer", 1, 0, true),
        signed(4, 1, 0, SignMode::Skipped),
    ]);
    assert!(
        find_notice(&skip_no_plan, "writer", 4)
            .body
            .contains("backlot plan publish")
    );

    // 8. DesignSigned(Skipped) with existing v1 plan on v2 -> producer notified to dispatch directly.
    events.push(publish(9, PlanKind::Plan, 1));
    events.push(publish(10, PlanKind::Acceptance, 1));
    events.push(req(11, 2));
    events.push(opened(12, "producer", 2, 0, true));
    events.push(signed(13, 2, 0, SignMode::Skipped));
    let owed_skip_with_plan = notices::owed(&replay(&events), dir());
    let p_skip = owed_skip_with_plan
        .iter()
        .find(|n| n.to == "producer" && n.source_seq == 13)
        .expect("producer direct-dispatch notice");
    assert!(p_skip.body.contains("backlot dispatch"), "{}", p_skip.body);
    assert!(
        owed_skip_with_plan
            .iter()
            .all(|n| !(n.to == "writer" && n.source_seq == 13)),
        "{owed_skip_with_plan:?}"
    );
}

#[test]
fn gate_off_notices_and_nudges_are_byte_identical_to_baseline() {
    let v1 = replay(&[ev(1, "user", studio_kind()), req(2, 1)]);
    assert!(notices::owed(&v1, dir()).is_empty());

    let v2 = replay(&[
        ev(1, "user", studio_kind()),
        req(2, 1),
        ev(3, "director", task_created("t1", "feature", 1)),
        req(4, 2),
    ]);
    let owed = notices::owed(&v2, dir());
    assert_eq!(owed.len(), 1, "{owed:?}");
    assert_eq!(owed[0].to, "director");
    assert_eq!(owed[0].label, "req");
    assert_eq!(owed[0].source_seq, 4);
    assert!(!owed[0].body.contains("design"), "{}", owed[0].body);

    // Stale design notices drop once the design moves past their phase.
    let stale_msg = |to: &str, source_seq: u64, label: &str| Message {
        seq: 90,
        from: "stage".into(),
        to: to.into(),
        file: Some(format!("messages/{source_seq}-{label}.md")),
        purpose: None,
        source_seq: Some(source_seq),
        delivered: false,
        dropped: false,
        dropped_reason: None,
        ts_ms: 0,
        delivered_ms: None,
    };
    let moved = replay(&[
        ev(1, "user", studio_required()),
        req(2, 1),
        opened(3, "producer", 1, 1, false),
        entry(4, "writer", 1, 1, DesignSlot::Proposal),
        entry(5, "director", 1, 1, DesignSlot::Buildability),
        entry(6, "producer", 1, 1, DesignSlot::Summary),
    ]);
    let check_stale = |st: &StudioState, to: &str, src: u64, lbl: &str| {
        assert_eq!(
            stale_reason(st, &stale_msg(to, src, lbl)),
            Some("design moved on")
        );
    };
    check_stale(&moved, "producer", 2, "design-open");
    check_stale(&moved, "writer", 3, "design-proposal");
    check_stale(&moved, "director", 4, "design-buildability");
    check_stale(&moved, "producer", 5, "design-summary");
    let signed_state = replay(&[
        ev(1, "user", studio_required()),
        req(2, 1),
        opened(3, "producer", 1, 1, false),
        entry(4, "writer", 1, 1, DesignSlot::Proposal),
        entry(5, "director", 1, 1, DesignSlot::Buildability),
        entry(6, "producer", 1, 1, DesignSlot::Summary),
        signed(7, 1, 1, SignMode::Approved),
        publish(8, PlanKind::Plan, 1),
        publish(9, PlanKind::Acceptance, 1),
    ]);
    check_stale(&signed_state, "writer", 7, "design-signed");
}
