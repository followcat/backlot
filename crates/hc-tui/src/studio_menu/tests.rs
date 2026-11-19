//! S3 §1 menu tests — each target kind's item list and order, the
//! dimmed-or-hidden rule and the offline dim.

use super::{MenuKind, StudioAct, StudioMenu};
use crate::studio::StudioKey;
use backlot_model::summary::{RoleSummary, StudioSummary, TaskSummary};

fn key(machine: Option<&str>) -> StudioKey {
    StudioKey {
        machine: machine.map(str::to_owned),
        id: "acme-shop".into(),
    }
}

fn labels(menu: &StudioMenu) -> Vec<String> {
    menu.items().iter().map(|i| i.label.clone()).collect()
}

fn role(state: &str) -> RoleSummary {
    serde_json::from_value(serde_json::json!({
        "role": "actor-alice",
        "name": "alice",
        "terminal": "host-b:term-1",
        "state": state,
        "task": "t1",
        "resumable": true
    }))
    .unwrap()
}

fn studio() -> StudioSummary {
    serde_json::from_value(serde_json::json!({
        "id": "acme-shop",
        "title": "acme-shop",
        "state": "working",
        "requirement_version": 3,
        "mode": "standard",
        "open_reviews": 0,
        "waiting": 0,
        "ready_to_finish": false,
        "tasks_done": 0,
        "tasks_total": 1,
        "tasks": [],
        "roles": []
    }))
    .unwrap()
}

fn task() -> TaskSummary {
    serde_json::from_value(serde_json::json!({
        "id": "t1",
        "title": "build the shop",
        "status": "running",
        "priority": "normal",
        "requirement_version": 3,
        "rework_rounds": 0,
        "rework_cap": 3,
        "waits_for": []
    }))
    .unwrap()
}

#[test]
fn a_studio_rows_items_follow_the_spec_order() {
    let menu = StudioMenu::studio(&key(Some("host-b")), &studio(), false, false);
    assert_eq!(
        labels(&menu),
        [
            "Focus producer",
            "Inspect studio sheet (v)",
            "Revise requirement…",
            "Tell producer…",
            "Stop stage…",
            "Finish studio…",
            "Actor records"
        ]
    );
    // A down stage swaps stop for start; a ready studio's finish is
    // teal.
    let down = StudioMenu::studio(&key(None), &studio(), true, false);
    assert_eq!(labels(&down)[4], "Start stage");
    let mut ready = studio();
    ready.ready_to_finish = true;
    let menu = StudioMenu::studio(&key(None), &ready, false, false);
    assert!(menu.items()[5].teal, "finish draws teal when ready");
}

#[test]
fn a_live_role_rows_items() {
    let menu = StudioMenu::role(&key(None), &role("live"), false);
    assert_eq!(menu.kind, MenuKind::Role);
    assert_eq!(
        labels(&menu),
        [
            "Focus terminal",
            "Peek screen (p)",
            "Tell alice…",
            "Inspect task t1 (v)",
            "Close alice…"
        ]
    );
}

#[test]
fn a_user_prompt_rows_items() {
    let mut r = role("live");
    r.attention_seq = Some(7);
    r.attention_user = true;
    let menu = StudioMenu::role(&key(None), &r, false);
    assert_eq!(menu.kind, MenuKind::Prompt);
    assert_eq!(
        labels(&menu),
        [
            "Allow prompt (y)",
            "Deny prompt (d)",
            "Peek screen (p)",
            "Focus terminal",
            "Clear stale attention #7…"
        ]
    );
}

#[test]
fn a_director_prompt_rows_items() {
    // D9: the user cannot answer directly — the override items name
    // the confirm the press opens.
    let mut r = role("live");
    r.attention_seq = Some(7);
    r.attention_user = false;
    let menu = StudioMenu::role(&key(None), &r, false);
    assert_eq!(menu.kind, MenuKind::DirPrompt);
    assert_eq!(
        labels(&menu),
        [
            "Peek screen (p)",
            "Focus terminal",
            "Inspect task t1 (v)",
            "Answer prompt myself: Allow…",
            "Answer prompt myself: Deny…",
            "Clear stale attention #7…"
        ]
    );
}

#[test]
fn a_task_rows_items_and_the_hidden_rule() {
    let mut task = task();
    let menu = StudioMenu::task(&key(None), &task, false);
    let items = menu.items();
    let item_labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(
        item_labels,
        [
            "Inspect deliveries & reviews (v)",
            "Priority: High (+)",
            "Priority: Normal (n)",
            "Priority: Low (-)",
            "Pause task…",
            "Take over task…",
            "Abandon task…"
        ]
    );
    // The current state is hidden-or-dimmed, never a no-op press:
    // `Priority: Normal` dims as `current`, and a paused task offers
    // Resume where a running one offers Pause.
    assert_eq!(items[2].dim.as_deref(), Some("current"));
    task.status = backlot_model::state::TaskStatus::Paused;
    assert!(
        labels(&StudioMenu::task(&key(None), &task, false))
            .iter()
            .any(|l| l == "Resume task…")
    );
    task.status = backlot_model::state::TaskStatus::Abandoned;
    let gone = labels(&StudioMenu::task(&key(None), &task, false));
    assert!(!gone.iter().any(|l| l.contains("Abandon")));
    assert!(!gone.iter().any(|l| l.contains("Pause")));
}

#[test]
fn a_card_rows_items() {
    let menu = StudioMenu::card("c3");
    assert_eq!(
        labels(&menu),
        [
            "Apply card (a)",
            "Show card details (s)",
            "Cancel card… (x)"
        ]
    );
}

#[test]
fn a_stage_rows_items() {
    let menu = StudioMenu::stage(false);
    assert_eq!(labels(&menu), ["Start stage"]);
    // An unreachable machine's stage row shows no items at all.
    assert!(StudioMenu::stage(true).items().is_empty());
}

#[test]
fn an_offline_menu_dims_commands_but_keeps_reads() {
    let menu = StudioMenu::studio(&key(Some("host-b")), &studio(), false, true);
    for item in menu.items() {
        match item.act {
            StudioAct::FocusTerminal | StudioAct::Peek | StudioAct::Sheet => {
                assert!(item.dim.is_none(), "{} stays live", item.label);
            }
            _ => assert_eq!(
                item.dim.as_deref(),
                Some("unreachable"),
                "{} dims",
                item.label
            ),
        }
    }
}

#[test]
fn every_items_key_hint_is_unique_within_its_menu() {
    for menu in [
        StudioMenu::studio(&key(None), &studio(), false, false),
        StudioMenu::role(&key(None), &role("live"), false),
        StudioMenu::card("c3"),
        StudioMenu::task(&key(None), &task(), false),
    ] {
        let mut seen = std::collections::BTreeSet::new();
        for item in menu.items() {
            if let Some(k) = item.key {
                assert!(seen.insert(k), "duplicate hint {k} on {}", item.label);
            }
        }
    }
}
