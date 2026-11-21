//! State-aware studio menus: a finished studio offers the producer
//! reopen path instead of a dead Finish, and keeps the ledger-reading
//! rows. A live or ready studio keeps the set it had.

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

fn item(menu: &StudioMenu, prefix: &str) -> Option<super::StudioItem> {
    menu.items()
        .into_iter()
        .find(|i| i.label.starts_with(prefix))
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

/// The producer row a live studio carries.
fn producer_live() -> RoleSummary {
    serde_json::from_value(serde_json::json!({
        "role": "producer",
        "name": "producer",
        "terminal": "t1",
        "state": "live"
    }))
    .unwrap()
}

/// The producer row a finished studio carries — released, its
/// session kept for a reopen.
fn producer_closed() -> RoleSummary {
    serde_json::from_value(serde_json::json!({
        "role": "producer",
        "name": "producer",
        "state": "closed",
        "resumable": true
    }))
    .unwrap()
}

fn studio() -> StudioSummary {
    serde_json::from_value(serde_json::json!({
        "id": "acme-shop",
        "title": "acme-shop",
        "state": "live",
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

/// A live studio with a live producer.
fn live_studio() -> StudioSummary {
    let mut s = studio();
    s.roles = vec![producer_live()];
    s
}

/// A studio that is done but not yet finished — every task terminal,
/// `studio.finished` never written. It reads `state: "finished"` too,
/// which is why `ready_to_finish` is the discriminator.
fn ready() -> StudioSummary {
    let mut s = live_studio();
    s.state = "finished".into();
    s.ready_to_finish = true;
    s
}

/// A studio whose `studio.finished` landed: every role released.
fn finished() -> StudioSummary {
    let mut s = studio();
    s.state = "finished".into();
    s.roles = vec![producer_closed()];
    s
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

/// A finished studio offers `Reopen producer`, never a dead
/// `Finish studio…`, and never a `Focus producer` on a closed seat.
#[test]
fn a_finished_studio_offers_reopen_and_no_finish() {
    let menu = StudioMenu::studio(&key(None), &finished(), false, false);
    let labels = labels(&menu);
    assert!(
        labels.iter().any(|l| l.starts_with("Reopen producer")),
        "a finished studio offers the reopen path: {labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l.starts_with("Finish studio")),
        "Finish is removed, not dimmed — it looks pressable: {labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l.starts_with("Focus producer")),
        "a closed producer has no terminal to focus: {labels:?}"
    );
}

/// The rows a finished studio keeps: the sheet, the records and the
/// requirement read the ledger, and the stage and `tell` still work.
#[test]
fn a_finished_studio_keeps_its_live_rows() {
    let menu = StudioMenu::studio(&key(None), &finished(), false, false);
    let labels = labels(&menu);
    for want in [
        "Inspect studio sheet (v)",
        "Revise requirement…",
        "Tell producer…",
        "Stop stage…",
        "Actor records",
    ] {
        assert!(labels.iter().any(|l| l == want), "{want} stays: {labels:?}");
    }
    // Nothing in the finished set is dimmed — the review keeps the
    // rows pressable, and only Finish goes away entirely.
    for it in menu.items() {
        assert!(it.dim.is_none(), "{} must not dim", it.label);
    }
}

/// A ready-but-unfinished studio keeps `Finish studio…` and has no
/// reopen — the press is the whole point of `ready_to_finish`, even
/// though `state` already reads `finished`.
#[test]
fn a_ready_studio_still_offers_finish() {
    let menu = StudioMenu::studio(&key(None), &ready(), false, false);
    let labels = labels(&menu);
    assert!(
        labels.iter().any(|l| l.starts_with("Finish studio")),
        "{labels:?}"
    );
    assert!(
        !labels.iter().any(|l| l.starts_with("Reopen")),
        "nothing to reopen before the finish: {labels:?}"
    );
    assert!(item(&menu, "Finish studio").unwrap().teal, "teal stays");
}

/// A live studio keeps the set it had: Finish present, entering the
/// confirm dialog — never pre-dimmed because tasks are still open —
/// and no reopen.
#[test]
fn a_live_studio_keeps_its_set() {
    let menu = StudioMenu::studio(&key(None), &live_studio(), false, false);
    let labels = labels(&menu);
    assert!(labels.iter().any(|l| l == "Focus producer"), "{labels:?}");
    let finish = item(&menu, "Finish studio").expect("Finish stays live");
    assert!(finish.dim.is_none(), "Finish is never pre-dimmed");
    assert!(!finish.teal, "an unfinished studio's finish is plain");
    assert!(
        !labels.iter().any(|l| l.starts_with("Reopen")),
        "{labels:?}"
    );
}

/// The reopen row runs the producer's own reopen — the existing
/// people-reopen path — with the `o` hint, and is not dimmed: a fixed
/// seat reopens even with no saved session.
#[test]
fn the_reopen_item_uses_the_people_reopen_path() {
    let menu = StudioMenu::studio(&key(None), &finished(), false, false);
    let reopen = item(&menu, "Reopen").expect("a reopen item");
    assert_eq!(reopen.act, StudioAct::Reopen);
    assert_eq!(reopen.key, Some('o'));
    assert!(reopen.dim.is_none(), "a fixed seat always reopens");
    // The act reads the producer role the menu carries — the same
    // field a closed-person row uses.
    assert_eq!(
        menu.role.as_ref().map(|r| r.role.as_str()),
        Some("producer"),
        "the reopen targets the producer"
    );
}

/// A finished studio whose ledger lists no producer row still gets a
/// reopen — the fixed seat is derived from the studio, not the roles.
#[test]
fn a_finished_studio_with_no_producer_row_still_reopens() {
    let mut s = finished();
    s.roles = Vec::new();
    let menu = StudioMenu::studio(&key(None), &s, false, false);
    let reopen = item(&menu, "Reopen").expect("the producer is derived");
    assert_eq!(reopen.act, StudioAct::Reopen);
    assert_eq!(
        menu.role.as_ref().map(|r| r.role.as_str()),
        Some("producer")
    );
}

/// An unreachable machine still dims the reopen row like every other
/// command row — the offline rule is not bypassed.
#[test]
fn an_offline_finished_studios_reopen_dims() {
    let menu = StudioMenu::studio(&key(Some("host-b")), &finished(), false, true);
    let reopen = item(&menu, "Reopen").expect("a reopen item");
    assert_eq!(reopen.dim.as_deref(), Some("unreachable"));
}

/// Reopens of individual people stay on their own closed rows — the
/// studio menu names only the producer, never the actors.
#[test]
fn the_studio_menu_never_piles_in_role_reopens() {
    let mut s = finished();
    let mut alice = producer_closed();
    alice.role = "actor-alice".into();
    alice.name = "alice".into();
    s.roles.push(alice);
    let menu = StudioMenu::studio(&key(None), &s, false, false);
    let reopens: Vec<String> = labels(&menu)
        .into_iter()
        .filter(|l| l.starts_with("Reopen"))
        .collect();
    assert_eq!(reopens.len(), 1, "one reopen, the producer's: {reopens:?}");
    assert!(reopens[0].starts_with("Reopen producer"));
}

#[test]
fn a_task_rows_items_and_the_hidden_rule() {
    let mut task = task();
    let menu = StudioMenu::task(&key(None), &task, false);
    let item_labels: Vec<_> = menu.items().iter().map(|i| i.label.clone()).collect();
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

/// Every hint stays unique per menu, the reopen row included, and no
/// menu carries a `Forget` row anywhere.
#[test]
fn every_items_key_hint_is_unique_within_its_menu() {
    for menu in [
        StudioMenu::studio(&key(None), &live_studio(), false, false),
        StudioMenu::studio(&key(None), &finished(), false, false),
        StudioMenu::studio(&key(None), &ready(), false, false),
        StudioMenu::role(&key(None), &role("live"), false),
        StudioMenu::card("c3"),
        StudioMenu::task(&key(None), &task(), false),
    ] {
        let mut seen = std::collections::BTreeSet::new();
        for it in menu.items() {
            assert!(
                !it.label.contains("Forget"),
                "no Forget row anywhere: {}",
                it.label
            );
            if let Some(k) = it.key {
                assert!(seen.insert(k), "duplicate hint {k} on {}", it.label);
            }
        }
    }
}

/// The finished menu is still a `Studio` menu — the sheet and the
/// picker key off the kind, not its rows.
#[test]
fn a_finished_studio_is_still_a_studio_menu() {
    let menu = StudioMenu::studio(&key(None), &finished(), false, false);
    assert_eq!(menu.kind, MenuKind::Studio);
}
