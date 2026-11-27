//! Tests for Lot Dock `Tab` / `Shift+Tab` section cycling, wrapping,
//! empty-section skipping, compose-mode `Tab` completion preservation, and
//! discoverability in hints and `?` help.

use super::*;
use crate::app::input::UiEvent;
use crate::panel::Target;
use crate::studio::StudioKey;

fn send_key(app: &mut App, k: hc_vt::Key, mods: hc_vt::Mods, action: hc_vt::Action) {
    app.on_input(UiEvent::Key(term_key(k, mods, action)), app.area.0);
}

fn tab(app: &mut App) {
    send_key(
        app,
        hc_vt::Key::Tab,
        hc_vt::Mods::NONE,
        hc_vt::Action::Press,
    );
}

fn shift_tab(app: &mut App) {
    send_key(
        app,
        hc_vt::Key::BackTab,
        hc_vt::Mods::SHIFT,
        hc_vt::Action::Press,
    );
}

fn multi_section_app() -> App {
    let mut app = app_with_panel_doc();
    let mut doc = summary_json();
    doc["studios"][0]["tasks"] = serde_json::json!([
        { "id": "task-4", "title": "Ship cart", "status": "created", "requirement_version": 1, "priority": "normal", "rework_rounds": 0, "rework_cap": 0 }
    ]);
    doc["studios"][0]["exchange"] = serde_json::json!([
        { "seq": 1, "who": "user", "text": "ship checkout" },
        { "seq": 2, "who": "producer", "text": "casting tasks" }
    ]);
    // Second active studio (`blog`), folds when unselected, has working actor + tasks + exchange.
    let blog = serde_json::json!({
        "id": "blog",
        "title": "Blog",
        "state": "live",
        "requirement_version": 1,
        "mode": "standard",
        "open_reviews": 0,
        "waiting": 0,
        "roles": [
            { "role": "producer", "name": "producer", "terminal": "t8", "state": "live", "work": "waiting" },
            { "role": "actor-b", "name": "bob", "terminal": "t9", "state": "live", "work": "working" }
        ],
        "tasks": [
            { "id": "t1", "title": "Write", "status": "created", "requirement_version": 1, "priority": "normal", "rework_rounds": 0, "rework_cap": 0 }
        ],
        "exchange": [
            { "seq": 1, "who": "user", "text": "start blog" }
        ]
    });
    // Third studio (`gamma-web`), calm/ready -> renders as 1-line CompactStudio (header-only block).
    let gamma = serde_json::json!({
        "id": "gamma-web",
        "title": "Gamma",
        "state": "live",
        "requirement_version": 1,
        "mode": "standard",
        "open_reviews": 0,
        "waiting": 0,
        "tasks_done": 1,
        "tasks_total": 1,
        "ready_to_finish": true,
        "roles": [
            { "role": "producer", "name": "producer", "terminal": "t10", "state": "live", "work": "waiting" }
        ],
        "tasks": [
            { "id": "t1", "title": "Done", "status": "accepted", "requirement_version": 1, "priority": "normal", "rework_rounds": 0, "rework_cap": 0 }
        ]
    });
    doc["studios"] = serde_json::json!([doc["studios"][0], blog, gamma]);
    let _: backlot_model::summary::Summary =
        serde_json::from_value(doc.clone()).expect("valid fixture");
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: doc,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    app.area = (160, 44);
    open_panel(&mut app);
    draw_to(&mut app, 160, 44);
    app
}

#[tokio::test]
async fn tab_and_shift_tab_cycle_five_sections_cross_studios_and_wrap() {
    let mut app = multi_section_app();

    // Starts on `acme-shop` Header (`BlockTop`).
    assert_eq!(app.panel.selected, Some(Target::Studio("acme-shop".into())));

    // 1 -> 2: Header -> People (first person = producer).
    tab(&mut app);
    assert_eq!(
        app.panel.selected,
        Some(Target::Person("acme-shop".into(), "producer".into()))
    );

    // Move deeper into People (`director`) so we test jumping out of mid-section.
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(
        app.panel.selected,
        Some(Target::Person("acme-shop".into(), "director".into()))
    );

    // 2 -> 3: People -> Tasks (first task/fold row in summary_json is task-4).
    tab(&mut app);
    assert_eq!(
        app.panel.selected,
        Some(Target::Task("acme-shop".into(), "task-4".into()))
    );

    // 3 -> 4: Tasks -> Exchange (first exchange line = seq 1).
    tab(&mut app);
    assert_eq!(
        app.panel.selected,
        Some(Target::Talk("acme-shop".into(), 1))
    );

    // 4 -> 5: Exchange -> Input (resting on `›`, never composing!).
    tab(&mut app);
    assert_eq!(app.panel.selected, Some(Target::Input("acme-shop".into())));
    assert!(!app.panel_editing());

    // 5 -> Next studio (`blog`): crosses to `blog`'s Header and unfolds `blog`.
    tab(&mut app);
    assert_eq!(app.panel.selected, Some(Target::Studio("blog".into())));
    let targets = app.panel_targets();
    assert!(targets.contains(&Target::Input("blog".into())));

    // Jump through `blog` (`Header -> People -> Tasks -> Exchange -> Input`), then cross to compact `gamma-web`.
    tab(&mut app); // People
    assert_eq!(
        app.panel.selected,
        Some(Target::Person("blog".into(), "producer".into()))
    );
    tab(&mut app); // Tasks (t1)
    assert_eq!(
        app.panel.selected,
        Some(Target::Task("blog".into(), "t1".into()))
    );
    tab(&mut app); // Exchange
    assert_eq!(app.panel.selected, Some(Target::Talk("blog".into(), 1)));
    tab(&mut app); // Input
    assert_eq!(app.panel.selected, Some(Target::Input("blog".into())));

    // Next `Tab` crosses to compact `gamma-web` (counts as header-only `Target::Block`).
    tab(&mut app);
    assert_eq!(app.panel.selected, Some(Target::Block("gamma-web".into())));

    // Next `Tab` wraps from last studio (`gamma-web`) to first (`acme-shop` Header).
    tab(&mut app);
    assert_eq!(app.panel.selected, Some(Target::Studio("acme-shop".into())));

    // Now test `Shift+Tab` reverse: first studio Header wraps to last studio (`gamma-web` header-only).
    shift_tab(&mut app);
    assert_eq!(app.panel.selected, Some(Target::Block("gamma-web".into())));

    // `Shift+Tab` from compact `gamma-web` crosses to `blog`'s last section (`Input`, unfolding `blog`!).
    shift_tab(&mut app);
    assert_eq!(app.panel.selected, Some(Target::Input("blog".into())));
    assert!(!app.panel_editing());

    // Also test `Tab + SHIFT` (non-BackTab encoding) and `Action::Repeat` both step backward.
    send_key(
        &mut app,
        hc_vt::Key::Tab,
        hc_vt::Mods::SHIFT,
        hc_vt::Action::Repeat,
    );
    assert_eq!(app.panel.selected, Some(Target::Talk("blog".into(), 1)));

    // Repeat `Tab` forward steps cleanly back to Input.
    send_key(
        &mut app,
        hc_vt::Key::Tab,
        hc_vt::Mods::NONE,
        hc_vt::Action::Repeat,
    );
    assert_eq!(app.panel.selected, Some(Target::Input("blog".into())));
}

#[tokio::test]
async fn tab_skips_empty_sections_and_groups_stage_down_with_header() {
    let mut app = app_with_panel_doc();
    let mut doc = summary_json();
    // Keep only producer/director (People), strip tasks/folds and exchange -> Header, People, Input.
    doc["studios"][0]["roles"] = serde_json::json!([
        { "role": "producer", "name": "producer", "terminal": "t1", "state": "live", "work": "waiting" },
        { "role": "director", "name": "director", "terminal": "t2", "state": "live", "work": "waiting" }
    ]);
    doc["studios"][0]["tasks"] = serde_json::json!([]);
    doc["studios"][0]["exchange"] = serde_json::json!([]);
    // Drop stage terminal so `Owed::StageDown` renders directly below BlockTop (`Target::Stage`).
    let remaining: Vec<_> = app
        .view
        .inventory
        .terminals()
        .values()
        .filter(|t| t.name.as_deref() != Some("backlot-stage"))
        .cloned()
        .collect();
    app.view.inventory_mut().replace(3, remaining);
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: doc,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    app.area = (120, 30);
    open_panel(&mut app);
    draw_to(&mut app, 120, 30);

    // Move onto `Owed::StageDown` inside Header section.
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.panel.selected, Some(Target::Stage(None)));

    // `Tab` treats StageDown as part of Header -> jumps straight to `producer` (People).
    tab(&mut app);
    assert_eq!(
        app.panel.selected,
        Some(Target::Person("acme-shop".into(), "producer".into()))
    );

    // `Tab` skips empty Tasks & Exchange -> lands on Input.
    tab(&mut app);
    assert_eq!(app.panel.selected, Some(Target::Input("acme-shop".into())));

    // `Shift+Tab` skips empty Exchange & Tasks -> People (`producer`), then Header (`BlockTop`).
    shift_tab(&mut app);
    assert_eq!(
        app.panel.selected,
        Some(Target::Person("acme-shop".into(), "producer".into()))
    );
    shift_tab(&mut app);
    assert_eq!(app.panel.selected, Some(Target::Studio("acme-shop".into())));
}

#[tokio::test]
async fn compose_keeps_tab_completion_ignores_shift_tab_and_help_advertises_sections() {
    let mut app = multi_section_app();
    app.panel.selected = Some(Target::Input("acme-shop".into()));
    key(&mut app, hc_vt::Key::Char('/'));
    assert!(app.panel_editing());
    assert_eq!(
        app.panel
            .draft(&StudioKey::local("acme-shop"))
            .unwrap()
            .as_str(),
        "/"
    );

    // While composing `/p`, `Shift+Tab` does nothing new (stays editing on `/p`).
    for ch in "pa".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    shift_tab(&mut app);
    assert!(app.panel_editing());
    assert_eq!(app.panel.selected, Some(Target::Input("acme-shop".into())));
    assert_eq!(
        app.panel
            .draft(&StudioKey::local("acme-shop"))
            .unwrap()
            .as_str(),
        "/pa"
    );

    // Plain `Tab` completes `/pause ` instead of jumping sections.
    tab(&mut app);
    assert!(app.panel_editing());
    assert_eq!(app.panel.selected, Some(Target::Input("acme-shop".into())));
    assert!(
        app.panel
            .draft(&StudioKey::local("acme-shop"))
            .unwrap()
            .as_str()
            .starts_with("/pause"),
        "{:?}",
        app.panel
            .draft(&StudioKey::local("acme-shop"))
            .unwrap()
            .as_str()
    );

    // `?` / keybind_help advertises `tab / ⇧tab`.
    let classic = crate::keybind_help::backlot_help_groups(
        &app.live.keybinds,
        &app.live.prefix,
        &app.panel_config,
        app.hollywood_key.unwrap_or_default(),
        app.panel_key_combo,
    );
    let dock_group = classic
        .iter()
        .find(|(title, _)| *title == "studio dock (open)")
        .expect("studio dock help group");
    assert!(
        dock_group.1.iter().any(|(k, _)| k.contains("tab")),
        "help missing tab section hint: {:?}",
        dock_group.1
    );

    // Focused footer at 160 cols displays `tab/⇧tab`.
    key(&mut app, hc_vt::Key::Escape);
    let lot_text = screen_lines(&draw_to(&mut app, 160, 44)).join("\n");
    assert!(lot_text.contains("tab/⇧tab"), "{lot_text}");
}
