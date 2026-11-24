//! Shared fixtures for W4b's lifecycle tests: a three-studio document
//! (a live `acme-shop`, an empty `fresh-one`, a finished `old-shop`),
//! an app whose dialog runner answers the lifecycle reads, and the
//! small menu/label helpers the per-part test files share. Neutral
//! data only (`acme-shop`, `alice`, `/home/user`).

use std::sync::{Arc, Mutex};

use super::*;
use hc_proto::watch::{DocKey, Document};
use hc_vt::{Action, Key, Mods};
use serde_json::json;

pub(super) const RUNNERS_JSON: &str = r#"[
  {"runner":"claude","status":"found","version":"1.0","models":["m1","m2"],
   "efforts":["low","medium","high","xhigh","max"]},
  {"runner":"cursor","status":"found","version":"1.0","models":["auto"],
   "efforts":[]},
  {"runner":"devin","status":"found","version":"1.0","models":["swe-2-max"],
   "efforts":[]}
]"#;

pub(super) const PRUNE_PLAN: &str = "backlot/acme-shop/integration: removed\nwork/t1: removed";

pub(super) const ARCHIVED_JSON: &str = r#"{
  "pinned": true,
  "archived": [
    {"name": "old-shop-20260101T120000Z", "stamp": "20260101T120000Z",
     "studio": "old-shop", "repo": "/home/user/acme-shop",
     "requirement": "the shop"}
  ]
}"#;

pub(super) fn summary() -> serde_json::Value {
    json!({
        "schema": "backlot/1",
        "updated_ms": 1,
        "studios": [
            {
                "id": "acme-shop", "title": "Acme shop", "state": "live",
                "requirement_version": 1, "mode": "standard",
                "open_reviews": 0, "waiting": 0, "tasks_total": 1,
                "roles": [
                    { "role": "producer", "name": "producer", "terminal": "t1",
                      "state": "live", "runner": "claude", "model": "m1", "effort": "high" },
                    { "role": "director", "name": "director", "terminal": "t2",
                      "state": "live", "runner": "claude", "model": "m1", "effort": "high" },
                    { "role": "writer", "name": "writer", "terminal": "t3",
                      "state": "live", "runner": "claude", "model": "m2", "effort": "medium" },
                    { "role": "actor-alice", "name": "alice", "terminal": "t4",
                      "state": "live", "runner": "claude", "model": "m1", "task": "t1" }
                ]
            },
            {
                "id": "fresh-one", "title": "Fresh", "state": "live",
                "requirement_version": 0, "mode": "standard",
                "open_reviews": 0, "waiting": 0, "tasks_total": 0,
                "roles": []
            },
            {
                "id": "old-shop", "title": "Old shop", "state": "finished",
                "requirement_version": 1, "mode": "standard",
                "open_reviews": 0, "waiting": 0, "tasks_total": 1,
                "roles": []
            }
        ]
    })
}

pub(super) fn lifecycle_app() -> App {
    let mut app = app_with(&["t1", "t2", "t3", "t4"]);
    app.set_plain(false);
    let mut stage = crate::sidebar::fixture::term("t9");
    stage.name = Some("backlot-stage".into());
    stage.state = "live".into();
    let terminals: Vec<_> = app
        .view
        .inventory
        .terminals()
        .values()
        .cloned()
        .chain(std::iter::once(stage))
        .collect();
    app.view.inventory_mut().replace(2, terminals);
    app.view.documents_mut().insert(
        DocKey {
            machine: None,
            name: "backlot".into(),
        },
        Document {
            value: summary(),
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    app.area = (120, 40);
    app.hollywood.mode = crate::hollywood::LayerMode::Hidden;
    app
}

/// A runner that records every argv and answers the lifecycle reads:
/// `runners --json` with the fixture rows, a `studio … --dry-run` with
/// the prune plan, everything else with empty output.
pub(super) fn scripted(app: &mut App) -> Arc<Mutex<Vec<Vec<String>>>> {
    let recorded = Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    app.set_ns_run(Arc::new(move |_machine, argv, _stdin| {
        sink.lock().unwrap().push(argv.clone());
        let out = if argv.first().map(String::as_str) == Some("runners") {
            Ok(RUNNERS_JSON.to_owned())
        } else if argv.iter().any(|a| a == "--dry-run") {
            Ok(PRUNE_PLAN.to_owned())
        } else {
            Ok(String::new())
        };
        Box::pin(async move { out })
    }));
    recorded
}

pub(super) fn key(app: &mut App, k: Key) {
    app.on_input(
        UiEvent::Key(term_key(k, Mods::NONE, Action::Press)),
        app.area.0,
    );
}

pub(super) fn role_menu(app: &App, role: &str) -> crate::studio_menu::StudioMenu {
    let machines = app.panel_summaries();
    let studio =
        crate::panel::data::studio(&machines, &crate::studio::StudioKey::local("acme-shop"))
            .expect("acme-shop");
    let row = studio
        .studio
        .roles
        .iter()
        .find(|r| r.role == role)
        .unwrap_or_else(|| panic!("{role} in the cast"));
    crate::studio_menu::StudioMenu::role(&crate::studio::StudioKey::local("acme-shop"), row, false)
}

pub(super) fn studio_menu(app: &App, id: &str) -> crate::studio_menu::StudioMenu {
    let machines = app.panel_summaries();
    let studio = crate::panel::data::studio(&machines, &crate::studio::StudioKey::local(id))
        .unwrap_or_else(|| panic!("{id}"));
    crate::studio_menu::StudioMenu::studio(
        &crate::studio::StudioKey::local(id),
        studio.studio,
        false,
        false,
    )
}

pub(super) fn labels(menu: &crate::studio_menu::StudioMenu) -> Vec<String> {
    menu.items().into_iter().map(|i| i.label).collect()
}

pub(super) fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_owned()).collect()
}
