//! `sheet.rs` — the Call Sheet's waiting items: the urgency order
//! across machines, the D9 director filter, machine-aware stage and
//! connectivity reads.

use super::*;
use crate::hollywood::rows::Target;
use crate::hollywood::tests::{machines, remote, summary, summary_json};
use crate::studio::StudioKey;
use backlot_model::summary::Summary;

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

/// The fixture with `attention_user` on `role` of `studio`.
fn with_user_prompt(studio: &str, role: &str) -> Summary {
    let mut value = summary_json();
    for s in value["studios"].as_array_mut().unwrap() {
        if s["id"] == studio {
            for r in s["roles"].as_array_mut().unwrap() {
                if r["role"] == role {
                    r["attention_user"] = true.into();
                    r["attention_seq"] = 7.into();
                    r["attention_detail"] = "may I run the tests?".into();
                }
            }
        }
    }
    serde_json::from_value(value).unwrap()
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
            Wait::Stage(_) => "stage",
            Wait::Card(_) => "card",
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
