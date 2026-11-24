//! App-level tests for standby fixed roles: peek and terminal focus
//! unavailable notices, `m` menu offers Recast without Restart/Close,
//! launcher offers Recast, and lot / waiting counts ignore standby.

use super::lifecycle_fixtures::*;
use super::*;
use crate::panel::Target;
use crate::studio::StudioKey;
use hc_proto::watch::{DocKey, Document};
use hc_vt::Key;

fn standby_app() -> App {
    let mut doc = summary();
    doc["studios"][0]["roles"][2] = serde_json::json!({
        "role": "writer",
        "name": "writer",
        "state": "standby",
        "runner": "claude",
        "model": "m2",
        "effort": "medium"
    });
    let mut app = app_with(&["t1", "t2", "t4"]);
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
            value: doc,
            ..Document::default()
        }
        .into(),
    );
    app.focused = Some("t1".into());
    app.sync_target();
    app.reconcile_published_view();
    app.area = (120, 40);
    app.panel.mode = crate::panel::PanelMode::Expanded;
    app.panel.focused = true;
    app.hollywood.mode = crate::hollywood::LayerMode::Hidden;
    draw_to(&mut app, 120, 40);
    app
}

#[tokio::test]
async fn standby_writer_disallows_peek_and_focus_offers_recast_and_ignores_counts() {
    let mut app = standby_app();
    let key_acme = StudioKey::local("acme-shop");

    // Counts ignore standby writer.
    {
        let machines = app.panel_summaries();
        let status = crate::hollywood::status_of(&app.view);
        let waits = crate::hollywood::waiting_items(&machines, &status);
        assert!(
            !waits.iter().any(|w| match w {
                crate::hollywood::Wait::Prompt(p) => p.role.role == "writer",
                crate::hollywood::Wait::Role(r) => r.role.role == "writer",
                _ => false,
            }),
            "waiting_items must ignore standby writer"
        );
    }
    // Single-studio check without empty `fresh-one`: urgent == 0 despite standby writer.
    let mut one_studio = summary();
    one_studio["studios"].as_array_mut().unwrap().truncate(1);
    one_studio["studios"][0]["roles"][2] = serde_json::json!({
        "role": "writer",
        "name": "writer",
        "state": "standby",
        "runner": "claude",
        "model": "m2",
        "effort": "medium"
    });
    let one_doc: backlot_model::summary::Summary = serde_json::from_value(one_studio).unwrap();
    let one_machines = vec![crate::panel::data::MachineSummary {
        machine: None,
        summary: one_doc,
    }];
    {
        let one_status = crate::hollywood::status_of(&app.view);
        assert!(
            crate::hollywood::waiting_items(&one_machines, &one_status).is_empty(),
            "no waiting items when only writer is in standby"
        );
    }

    // Selecting standby writer in dock: `p` and `Enter` show short unavailable reasons.
    app.panel.focused = true;
    app.panel.selected = Some(Target::Person(key_acme.clone(), "writer".into()));
    app.notice = None;
    key(&mut app, Key::Char('p'));
    assert!(app.peek.is_none(), "standby role cannot peek");
    let peek_notice = app.notice.take().expect("peek unavailable notice");
    assert!(!peek_notice.body.is_empty());

    key(&mut app, Key::Enter);
    assert_ne!(app.focused.as_deref(), Some("t3"));
    let focus_notice = app.notice.take().expect("terminal unavailable notice");
    assert!(
        focus_notice.body.contains("started") || focus_notice.body.contains("plan"),
        "expected short standby reason, got: {}",
        focus_notice.body
    );

    // `m` menu offers `Recast…` (enabled), dims Focus/Peek with a reason, omits Restart/Close/Reopen.
    let menu = role_menu(&app, "writer");
    let items = menu.items();
    let recast = items
        .iter()
        .find(|i| i.label.starts_with("Recast"))
        .expect("Recast item on standby writer");
    assert_eq!(recast.dim, None, "Recast must be enabled");
    assert!(
        !items.iter().any(|i| i.label.starts_with("Restart")
            || i.label.starts_with("Close")
            || i.label.starts_with("Reopen")),
        "unexpected lifecycle actions on standby: {:?}",
        labels(&menu)
    );
    for unavailable in items
        .iter()
        .filter(|i| i.label.starts_with("Focus") || i.label.starts_with("Peek"))
    {
        assert!(
            unavailable.dim.is_some(),
            "Focus/Peek must carry dim reason on standby: {unavailable:?}"
        );
    }

    // `Ctrl+B ?` launcher includes Recast writer, not Restart or Reopen.
    let help_labels: Vec<String> = app
        .help_groups()
        .into_iter()
        .flat_map(|g| g.rows)
        .map(|r| r.label)
        .collect();
    assert!(
        help_labels.iter().any(|l| l == "Recast writer · acme-shop"),
        "missing Recast writer in launcher: {help_labels:?}"
    );
    assert!(
        !help_labels
            .iter()
            .any(|l| l == "Restart writer · acme-shop" || l.starts_with("Reopen writer")),
        "unexpected Restart/Reopen in launcher: {help_labels:?}"
    );
}
