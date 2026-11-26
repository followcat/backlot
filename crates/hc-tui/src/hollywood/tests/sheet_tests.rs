//! `sheet.rs` — the Call Sheet's waiting items: the urgency order
//! across machines, the D9 director filter, machine-aware stage and
//! connectivity reads.

use super::*;
use crate::hollywood::rows::Target;
use crate::hollywood::tests::{machines, remote, summary, with_user_prompt};
use crate::studio::StudioKey;

/// A status closure over named facts: `online` machines are
/// `Conn::Online`, `down` machines have a stopped stage; this core
/// is online with a live stage.
fn status<'a>(online: &'a [&'a str], down: &'a [&'a str]) -> Box<Status<'a>> {
    Box::new(move |machine| {
        let Some(name) = machine else {
            return (Conn::Online, true);
        };
        if !online.contains(&name) {
            (Conn::Offline, false)
        } else {
            (Conn::Online, !down.contains(&name))
        }
    })
}

#[test]
fn prompts_sort_before_stage_cards_and_ready() {
    let local = with_user_prompt("acme-shop", "actor-alice");
    let mut away = summary();
    away.studios[0].ready_to_finish = true;
    let all = vec![
        crate::panel::data::MachineSummary {
            machine: None,
            summary: local,
        },
        remote(&away),
    ];
    let status = status(&["host-b"], &["host-b"]);
    let waits = waiting_items(&all, &status);
    let kinds: Vec<&str> = waits
        .iter()
        .map(|w| match w {
            Wait::Prompt(_) => "prompt",
            Wait::Role(item) if item.asks => "asks",
            Wait::Role(_) => "role",
            Wait::Hollywood(_) => "hollywood",
            Wait::NoRequirement(_) => "new",
            Wait::Stage(_) => "stage",
            Wait::Card(_) | Wait::Outward(_) => "card",
            Wait::Ready(_) => "ready",
        })
        .collect();
    // alice's prompt, host-b's down stage, the two cards, then the
    // remote ready studio.
    assert_eq!(kinds, ["prompt", "stage", "card", "card", "ready"]);
}

#[test]
fn a_director_owned_prompt_is_not_listed() {
    // `attention` without `attention_user` stays Hollywood's to
    // answer (D9) — the sheet is empty of it.
    let all = machines(Some(&summary()));
    let waits = waiting_items(&all, &status(&[], &[]));
    assert!(waits.iter().all(|w| !matches!(w, Wait::Prompt(_))));
}

#[test]
fn an_offline_machine_with_live_studios_lists_a_stage_row() {
    let mut all = machines(Some(&summary()));
    all.push(remote(&summary()));
    let status = status(&[], &[]); // host-b is unreachable
    let waits = waiting_items(&all, &status);
    let stage = waits.iter().any(|w| {
        matches!(
            w,
            Wait::Stage(StageItem {
                offline: true,
                machine: Some("host-b"),
                ..
            })
        )
    });
    assert!(
        stage,
        "{:?}",
        waits.iter().map(|w| w.target()).collect::<Vec<_>>()
    );
    // The offline machine's own prompts and ready studios still list.
    assert_eq!(
        waits.iter().filter(|w| matches!(w, Wait::Stage(_))).count(),
        1
    );
}

#[test]
fn a_stopped_stage_lists_a_restart_row() {
    let all = vec![remote(&summary())];
    let status = status(&["host-b"], &["host-b"]);
    let waits = waiting_items(&all, &status);
    assert!(waits.iter().any(|w| matches!(
        w,
        Wait::Stage(StageItem {
            offline: false,
            machine: Some("host-b"),
            studios: 3
        })
    )));
}

#[test]
fn an_empty_machine_never_lists_a_stage_row() {
    let mut empty = remote(&summary());
    empty.summary.studios.clear();
    let all = vec![empty];
    let waits = waiting_items(&all, &status(&[], &[]));
    assert!(waits.iter().all(|w| !matches!(w, Wait::Stage(_))));
}

/// R3-3: a READY row earns its second line only while the wrap
/// report is still owed — the dim `report pending` is what the line
/// holds; prompts and cards always detail, a stage row never does.
#[test]
fn a_ready_row_details_only_while_the_report_pends() {
    let mut pending = summary();
    pending.studios[0].ready_to_finish = true;
    pending.studios[0].next = Some("node report — director".into());
    let all = machines(Some(&pending));
    let waits = waiting_items(&all, &status(&[], &[]));
    let ready = waits
        .iter()
        .find(|w| matches!(w, Wait::Ready(_)))
        .expect("the ready studio lists");
    assert!(ready.detailable());

    let mut landed = summary();
    landed.studios[0].ready_to_finish = true;
    let all = machines(Some(&landed));
    let waits = waiting_items(&all, &status(&[], &[]));
    let ready = waits
        .iter()
        .find(|w| matches!(w, Wait::Ready(_)))
        .expect("the ready studio still lists");
    assert!(!ready.detailable());
    // The other kinds keep their own rule.
    assert!(
        waits
            .iter()
            .all(|w| matches!(w, Wait::Card(_)) == w.detailable())
    );
}

#[test]
fn wait_targets_are_machine_qualified() {
    let mut all = machines(Some(&with_user_prompt("acme-shop", "actor-alice")));
    all.push(remote(&with_user_prompt("acme-shop", "actor-alice")));
    let waits = waiting_items(&all, &status(&["host-b"], &[]));
    let targets: Vec<Target> = waits.iter().map(|w| w.target()).collect();
    assert_eq!(
        &targets[..2],
        &[
            Target::Prompt(StudioKey::local("acme-shop"), "actor-alice".into()),
            Target::Prompt(
                StudioKey {
                    machine: Some("host-b".into()),
                    id: "acme-shop".into()
                },
                "actor-alice".into()
            ),
        ]
    );
}

#[test]
fn ask_answered_drops_asks_and_waiting_count_while_exchange_keeps_question() {
    use backlot_model::event::EventKind;
    use backlot_model::state::replay;
    use backlot_model::summary::summary_with_exchange;
    use backlot_model::testkit::{base_events, ev, role_started};
    use std::collections::BTreeMap;

    let mut events = base_events();
    events.push(ev(4, "stage", role_started("producer")));
    events.push(ev(
        10,
        "producer",
        EventKind::MessageQueued {
            to: "user".into(),
            file: Some("messages/10-q.md".into()),
            purpose: Some("ask".into()),
            source_seq: None,
        },
    ));
    let mut snippets = BTreeMap::new();
    snippets.insert(("acme-shop".into(), 10), "what should we build?".into());

    let before = summary_with_exchange(
        &[("acme-shop".into(), replay(&events))],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &snippets,
        1,
    );
    let before_machines = machines(Some(&before));
    let waits_before = waiting_items(&before_machines, &status(&[], &[]));
    assert!(
        waits_before
            .iter()
            .any(|w| matches!(w, Wait::Role(r) if r.asks && r.role.role == "producer"))
    );

    events.push(ev(
        11,
        "stage",
        EventKind::AskAnswered {
            role: "producer".into(),
            target: 10,
        },
    ));
    let after = summary_with_exchange(
        &[("acme-shop".into(), replay(&events))],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &snippets,
        2,
    );
    let after_machines = machines(Some(&after));
    let waits_after = waiting_items(&after_machines, &status(&[], &[]));
    assert!(
        !waits_after
            .iter()
            .any(|w| matches!(w, Wait::Role(r) if r.role.role == "producer" && r.asks))
    );
    assert_eq!(after.studios[0].exchange.len(), 1);
    assert_eq!(after.studios[0].exchange[0].text, "what should we build?");
}
