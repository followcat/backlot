//! Slice 3 tests: Call Sheet & Lot Pill thinking / queued spinners
//! (`has_animation()`) and Lot Pill unread reply count initialization,
//! user-tell exclusion, collapsed incrementing, and expand clearing.

use super::*;

fn publish_doc(app: &mut App, doc: serde_json::Value) {
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: doc,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
}

#[tokio::test]
async fn call_sheet_and_collapsed_lot_pill_animate_when_hollywood_is_queued_or_working() {
    let mut app = app_with_hollywood();
    app.panel.mode = crate::panel::PanelMode::Closed;
    app.area = (120, 24);

    // Idle Hollywood + closed dock: no animation armed.
    let mut doc = summary_json();
    for s in doc["studios"].as_array_mut().unwrap() {
        for r in s["roles"].as_array_mut().unwrap() {
            r["work"] = serde_json::Value::Null;
        }
    }
    publish_doc(&mut app, doc.clone());
    draw_to(&mut app, 120, 24);
    assert!(!app.has_animation(), "idle pill arms no animation");

    // 1. Queued behind send gate (`hollywood.pending = true`):
    //    Expanded Call Sheet renders `queued…` and arms `has_animation()`.
    doc["hollywood"]["pending"] = serde_json::json!(true);
    publish_doc(&mut app, doc.clone());
    app.hollywood_open();
    let expanded_queued = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(expanded_queued.contains("queued…"), "{expanded_queued}");
    assert!(app.has_animation(), "queued Call Sheet arms spinner");

    // 2. Collapsing the Call Sheet keeps `has_animation()` active via the Lot Pill spinner!
    app.hollywood_toggle();
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
    let top_spin0 = screen_lines(&draw_to(&mut app, 120, 24))[0].clone();
    assert!(
        app.has_animation(),
        "collapsed Lot Pill keeps spinner armed"
    );
    assert!(
        !top_spin0.contains('✦'),
        "spinner glyph replaced static spark: {top_spin0}"
    );
    let due = app.panel_anim_deadline.expect("deadline armed");
    app.tick_panel(due);
    let top_spin1 = screen_lines(&draw_to(&mut app, 120, 24))[0].clone();
    assert_ne!(top_spin0, top_spin1, "Lot Pill spinner advances on tick");

    // 3. `hollywood.work = "working"` renders `thinking…` in Call Sheet and animates pill.
    doc["hollywood"]["pending"] = serde_json::json!(false);
    doc["hollywood"]["work"] = serde_json::json!("working");
    publish_doc(&mut app, doc.clone());
    app.hollywood_open();
    let expanded_working = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(expanded_working.contains("thinking…"), "{expanded_working}");
    assert!(app.has_animation());

    // 4. Settled (`work = "waiting"`, `pending = false`) stops animation and restores `✦`.
    app.hollywood_toggle();
    doc["hollywood"]["work"] = serde_json::json!("waiting");
    publish_doc(&mut app, doc.clone());
    let top_calm = screen_lines(&draw_to(&mut app, 120, 24))[0].clone();
    assert!(top_calm.contains('✦'), "{top_calm}");
    assert!(!app.has_animation(), "settled pill disarms animation");

    // 5. Stopped Hollywood (`state = "down"`, e.g. `cannot resume`) with undelivered
    //    inbox mail stays `down` instead of spinning `queued…` forever.
    doc["hollywood"]["state"] = serde_json::json!("down");
    doc["hollywood"]["pending"] = serde_json::json!(true);
    publish_doc(&mut app, doc);
    let top_down = screen_lines(&draw_to(&mut app, 120, 24))[0].clone();
    assert!(!app.has_animation(), "down hollywood never spins queued");
    assert!(top_down.contains('✦'), "{top_down}");
    app.hollywood_open();
    let expanded_down = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(expanded_down.contains("down"), "{expanded_down}");
    assert!(!expanded_down.contains("queued…"), "{expanded_down}");
}

#[tokio::test]
async fn lot_pill_unread_seeds_zero_on_boot_ignores_user_tells_and_clears_on_expand() {
    // Cold boot with historical Hollywood reports (#2 and #5) already in the first summary.
    let mut app = app_with(&["t1", "t2", "t4"]);
    app.set_plain(false);
    let mut stage = crate::sidebar::fixture::term("t9");
    stage.name = Some("backlot-stage".into());
    stage.state = "live".into();
    let mut hwood = crate::sidebar::fixture::term("t10");
    hwood.name = Some("bl-hollywood".into());
    hwood.state = "live".into();
    let terms: Vec<_> = app
        .view
        .inventory
        .terminals()
        .values()
        .cloned()
        .chain([stage, hwood])
        .collect();
    app.view.inventory_mut().replace(2, terms);
    app.set_hollywood(Ok(Some(cfg(None))), std::path::Path::new("config.toml"));
    app.hollywood.mode = LayerMode::Collapsed;
    // Prevent `when_waiting` from auto-expanding so we test cold-boot collapsed seeding.
    app.panel_config.call_sheet_start = crate::panel::CallSheetStart::Collapsed;

    let mut doc = summary_json();
    doc["hollywood"]["headline"] = serde_json::json!("historical headline");
    doc["hollywood"]["headline_seq"] = serde_json::json!(5);
    doc["hollywood"]["exchange"] = serde_json::json!([
        { "seq": 2, "who": "hollywood", "text": "older report" },
        { "seq": 5, "who": "hollywood", "text": "historical headline" }
    ]);
    publish_doc(&mut app, doc.clone());
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
    assert_eq!(app.hollywood.seen_hollywood_seq, Some(5));
    assert_eq!(
        app.studio.lot.counts.unread, 0,
        "historical reports on boot never badge as unread"
    );

    // User sends `tell hollywood` (`seq = 6, who = "user"`) while collapsed -> still 0 unread!
    doc["hollywood"]["exchange"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({ "seq": 6, "who": "user", "text": "any updates?" }));
    publish_doc(&mut app, doc.clone());
    assert_eq!(
        app.studio.lot.counts.unread, 0,
        "user's own tell never counts as unread"
    );

    // Hollywood posts two replies (`seq = 7` and `seq = 8`) while collapsed -> unread == 2!
    doc["hollywood"]["headline"] = serde_json::json!("beta-tools ready");
    doc["hollywood"]["headline_seq"] = serde_json::json!(8);
    let ex = doc["hollywood"]["exchange"].as_array_mut().unwrap();
    ex.push(serde_json::json!({ "seq": 7, "who": "hollywood", "text": "checked acme-shop" }));
    ex.push(serde_json::json!({ "seq": 8, "who": "hollywood", "text": "beta-tools ready" }));
    publish_doc(&mut app, doc.clone());
    assert_eq!(app.studio.lot.counts.unread, 2);

    // Lot Pill renders `+2 new` at full width and `+2` at compact width.
    app.area = (150, 24);
    let top_full = screen_lines(&draw_to(&mut app, 150, 24))[0].clone();
    assert!(top_full.contains("+2 new"), "{top_full}");
    app.area = (80, 24);
    let top_compact = screen_lines(&draw_to(&mut app, 80, 24))[0].clone();
    assert!(top_compact.contains("+2"), "{top_compact}");

    // Expanding the Call Sheet immediately advances `seen_hollywood_seq` to 8 and clears unread!
    app.hollywood_toggle();
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    assert_eq!(app.hollywood.seen_hollywood_seq, Some(8));
    assert_eq!(app.studio.lot.counts.unread, 0);

    // Collapsing again keeps unread == 0 until a newer Hollywood reply arrives.
    app.hollywood_toggle();
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
    assert_eq!(app.studio.lot.counts.unread, 0);
    app.area = (150, 24);
    let top_cleared = screen_lines(&draw_to(&mut app, 150, 24))[0].clone();
    assert!(!top_cleared.contains("+2"), "{top_cleared}");
}
