//! S3 §1 menu tests — each target kind's item list and order, the
//! dimmed-or-hidden rule and the offline dim.

use super::{MenuBacklot, MenuKind, StudioAct, StudioMenu};
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
            "Edit cast…",
            "Finish studio…",
            "Actor records",
            "Delete studio…"
        ]
    );
    // A down stage adds `Start stage` — the row that unblocks this
    // studio. Stopping the stage is machine-wide and lives on the
    // machine's own menu.
    let down = StudioMenu::studio(&key(None), &studio(), true, false);
    assert_eq!(labels(&down)[5], "Start stage");
    assert!(
        !labels(&menu).iter().any(|l| l.contains("Stop stage")),
        "a live stage offers no per-studio stop"
    );
    let mut ready = studio();
    ready.ready_to_finish = true;
    let menu = StudioMenu::studio(&key(None), &ready, false, false);
    assert!(menu.items()[5].teal, "finish draws teal when ready");
    assert_eq!(menu.title(), "Studio · acme-shop");
    let role_menu = StudioMenu::role(&key(None), &role("live"), false);
    assert_eq!(role_menu.title(), "Role · alice (acme-shop)");
    let task_menu = StudioMenu::task(&key(None), &task(), false);
    assert_eq!(task_menu.title(), "Task · t1 (acme-shop)");
    assert_eq!(StudioMenu::stage(false).title(), "Stage");
    assert_eq!(StudioMenu::lot().title(), "Lot");
    assert_eq!(StudioMenu::hollywood(None).title(), "Hollywood");
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
            "Restart alice",
            "Recast…",
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
            "Restart alice",
            "Recast…",
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
            "Restart alice",
            "Recast…",
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

fn machine_row(backlot: MenuBacklot, stage_down: bool) -> StudioMenu {
    StudioMenu::machine_row(Some("host-b".into()), backlot, stage_down, false, None)
}

#[test]
fn a_running_stage_machine_menu_restarts_then_stops_the_stage() {
    let menu = machine_row(MenuBacklot::Installed("0.3.0".into()), false);
    let items = menu.items();
    assert_eq!(
        labels(&menu),
        [
            "Restart the stage",
            "Stop the stage…",
            "Stage log",
            "Agent skill…",
            "New studio here",
            "Connect machine",
            "Rename machine",
            "Remove machine"
        ]
    );
    assert_eq!(items[0].act, StudioAct::StageRestart);
    assert_eq!(items[1].act, StudioAct::StageStop);
    assert!(items.iter().all(|it| it.dim.is_none()), "{items:?}");
}

#[test]
fn a_down_stage_machine_menu_only_starts_the_stage() {
    let menu = machine_row(MenuBacklot::Installed("0.3.0".into()), true);
    let labels = labels(&menu);
    assert_eq!(labels[0], "Start the stage");
    assert!(
        !labels
            .iter()
            .any(|l| l.contains("Stop") || l.contains("Restart")),
        "a down stage offers only its start: {labels:?}"
    );
}

#[test]
fn a_machine_without_backlot_offers_no_stage_controls() {
    for backlot in [MenuBacklot::Missing, MenuBacklot::Unknown] {
        let labels = labels(&machine_row(backlot, false));
        assert!(
            !labels.iter().any(|l| l.contains("stage")),
            "no stage controls without an install: {labels:?}"
        );
    }
}

#[test]
fn the_machine_menu_key_hints_are_unique() {
    for menu in [
        machine_row(MenuBacklot::Installed("0.3.0".into()), false),
        machine_row(MenuBacklot::Installed("0.3.0".into()), true),
        machine_row(MenuBacklot::Missing, false),
    ] {
        let mut seen = std::collections::BTreeSet::new();
        for item in menu.items() {
            if let Some(k) = item.key {
                assert!(seen.insert(k), "duplicate hint {k} on {}", item.label);
            }
        }
    }
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
    let cfg = backlot_stage::config::HollywoodCfg {
        runner: "fake".into(),
        model: "m1".into(),
        effort: Some("high".into()),
        key: None,
        default_cast: None,
    };
    let unconfigured = labels(&StudioMenu::hollywood_title(None));
    assert_eq!(unconfigured, vec!["Switch agent…"]);
    let configured = labels(&StudioMenu::hollywood_title(Some(&cfg)));
    assert_eq!(
        configured,
        vec![
            "Switch agent…",
            "Focus Hollywood terminal",
            "Restart Hollywood",
        ]
    );
    for menu in [
        StudioMenu::studio(&key(None), &studio(), false, false),
        StudioMenu::role(&key(None), &role("live"), false),
        StudioMenu::card("c3"),
        StudioMenu::task(&key(None), &task(), false),
        StudioMenu::lot(),
        StudioMenu::hollywood_title(None),
        StudioMenu::hollywood_title(Some(&cfg)),
    ] {
        let mut seen = std::collections::BTreeSet::new();
        for item in menu.items() {
            if let Some(k) = item.key {
                assert!(seen.insert(k), "duplicate hint {k} on {}", item.label);
            }
        }
    }
}
