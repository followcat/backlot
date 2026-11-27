//! Focus-dock key & mouse interaction tests: screen-order `Tab`/`Shift+Tab`
//! skipping calm `WAITING`, digits `1`–`9`/`0`, uneven-width chip strip reveal,
//! `STUDIOS N` header focus, pinned `[+ new]`, and outside-click boundary
//! immediately below `▀▀`.

use super::*;
use crate::hollywood::Target;

fn strip_app_six_studios() -> App {
    let mut app = app_with_hollywood();
    let mut doc = summary_json();
    doc["studios"] = serde_json::json!([
        { "id": "acme-shop", "title": "Acme", "state": "live", "requirement_version": 2, "mode": "standard", "open_reviews": 0, "waiting": 0, "ready_to_finish": true, "tasks_done": 8, "tasks_total": 8, "roles": [] },
        { "id": "beta-api-longname", "title": "Beta", "state": "live", "requirement_version": 1, "mode": "standard", "open_reviews": 0, "waiting": 0, "tasks_done": 3, "tasks_total": 7, "roles": [] },
        { "id": "gamma-web-service", "title": "Gamma", "state": "live", "requirement_version": 1, "mode": "standard", "open_reviews": 0, "waiting": 0, "tasks_done": 10, "tasks_total": 12, "roles": [] },
        { "id": "delta-ops-workers", "title": "Delta", "state": "live", "requirement_version": 1, "mode": "standard", "open_reviews": 0, "waiting": 0, "tasks_done": 6, "tasks_total": 8, "roles": [] },
        { "id": "eps-ui-components", "title": "Eps", "state": "live", "requirement_version": 1, "mode": "standard", "open_reviews": 0, "waiting": 0, "tasks_done": 1, "tasks_total": 5, "roles": [] },
        { "id": "zeta-background", "title": "Zeta", "state": "live", "requirement_version": 1, "mode": "standard", "open_reviews": 0, "waiting": 0, "tasks_done": 2, "tasks_total": 4, "roles": [] },
    ]);
    // 5 waits keep `cap=12` spare at 1 row so STUDIOS stays in 1-row strip mode even when focused.
    doc["hollywood"]["cards"] = serde_json::json!([
        { "id": "c1", "studio": "acme-shop", "action": "pause a", "why": "w1" },
        { "id": "c2", "studio": "beta-api-longname", "action": "pause b", "why": "w2" },
        { "id": "c3", "studio": "gamma-web-service", "action": "pause c", "why": "w3" },
        { "id": "c4", "studio": "delta-ops-workers", "action": "pause d", "why": "w4" },
    ]);
    doc["hollywood"]["exchange"] = serde_json::json!([
        { "seq": 1, "who": "hollywood", "text": "all 6 studios healthy" },
    ]);
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: doc,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    app.area = (90, 24);
    app
}

#[tokio::test]
async fn digits_and_arrows_keep_uneven_strip_chip_visible_and_new_pinned() {
    let mut app = strip_app_six_studios();
    alt_y(&mut app);
    // `Tab` from Compose jumps to STUDIOS; digits 1..9 select studios when not editing.
    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Studio("acme-shop".into()))
    );
    draw_to(&mut app, 90, 24);
    assert!(app.hollywood_hits.strip, "cap=12 draws 1-row strip");

    // Digit `6` selects 6th studio (`zeta-background`) and scrolls uneven strip cleanly.
    key(&mut app, hc_vt::Key::Char('6'));
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Studio("zeta-background".into()))
    );
    assert!(app.hollywood.frame_scroll > 0);
    let buf = draw_to(&mut app, 90, 24);
    let strip_y = app.hollywood_hits.body.y;
    let row: String = (app.hollywood_hits.area.x..app.hollywood_hits.area.right())
        .map(|x| buf.buffer()[(x, strip_y)].symbol())
        .collect();
    assert!(
        row.contains("zeta-background"),
        "6th chip scrolled into view:\n{row}"
    );
    assert!(
        row.contains("[+ new]"),
        "[+ new] stays pinned at right:\n{row}"
    );

    // `←` back to `1` scrolls back to 0 cleanly; `0` targets NewStudio while pinned.
    for _ in 0..5 {
        key(&mut app, hc_vt::Key::ArrowLeft);
    }
    assert_eq!(
        app.hollywood.selected,
        Some(Target::Studio("acme-shop".into()))
    );
    assert_eq!(app.hollywood.frame_scroll, 0);
    key(&mut app, hc_vt::Key::Char('0'));
    assert_eq!(app.hollywood.selected, Some(Target::NewStudio));
}

fn calm_app_with_exchange() -> App {
    let mut app = app_with_hollywood();
    let mut doc = summary_json();
    doc["hollywood"]["cards"] = serde_json::json!([]);
    doc["hollywood"]["exchange"] = serde_json::json!([
        { "seq": 10, "who": "user", "text": "check status" },
        { "seq": 11, "who": "hollywood", "text": "all 2 studios clear" },
    ]);
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: doc,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    app
}

#[tokio::test]
async fn calm_tab_skips_waiting_and_mouse_covers_talk_chips_and_strip_clicks() {
    let mut app = calm_app_with_exchange();
    app.area = (120, 24);
    alt_y(&mut app);
    // Empty WAITING is skipped cleanly by `Tab` / `Shift+Tab`.
    key(&mut app, hc_vt::Key::Tab);
    assert!(matches!(app.hollywood.selected, Some(Target::Studio(_))));
    key(&mut app, hc_vt::Key::Tab);
    assert!(app.hollywood.editing);
    key_mods(&mut app, hc_vt::Key::BackTab, hc_vt::Mods::SHIFT);
    assert!(matches!(app.hollywood.selected, Some(Target::Studio(_))));

    // Clicking a talk line selects & focuses HOLLYWOOD (does not open Reader until `↵ read` click).
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    draw_to(&mut app, 120, 24);
    let (talk_rect, _) = app
        .hollywood_hits
        .rows
        .iter()
        .find(|(_, t)| *t == Target::Talk(11))
        .expect("talk row hit")
        .clone();
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        talk_rect.x + 4,
        talk_rect.y,
    ));
    assert_eq!(app.hollywood.selected, Some(Target::Talk(11)));
    assert!(app.hollywood.view.is_none());
    assert!(recorded.lock().unwrap().is_empty());

    // Clicking a studio chip on the 1-row strip focuses that studio and collapses.
    app.hollywood_select(Target::Input);
    app.hollywood.editing = false;
    draw_to(&mut app, 120, 24);
    assert!(app.hollywood_hits.strip);
    let (chip_rect, _) = app
        .hollywood_hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Studio(k) if k.id == "beta-tools"))
        .expect("strip chip hit")
        .clone();
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        chip_rect.x + 2,
        chip_rect.y,
    ));
    assert_eq!(app.studio.current, Some("beta-tools".into()));
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
}

#[tokio::test]
async fn focus_dock_mouse_clicks_labels_and_bottom_edge_boundary() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    alt_y(&mut app);
    // Rest on `Target::Input`.
    key(&mut app, hc_vt::Key::Escape);
    draw_to(&mut app, 120, 24);

    // Clicking `STUDIOS N` header label focuses STUDIOS without opening/collapsing.
    let lbl = app.hollywood_hits.studios_label;
    assert!(!lbl.is_empty(), "strip label hit recorded");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        lbl.x + 2,
        lbl.y,
    ));
    assert!(matches!(app.hollywood.selected, Some(Target::Studio(_))));
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);

    // Clicking `[+ new]` opens wizard.
    draw_to(&mut app, 120, 24);
    let new_rect = app
        .hollywood_hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::NewStudio))
        .expect("pinned [+ new] hit")
        .0;
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        new_rect.x + 1,
        new_rect.y,
    ));
    assert!(app.new_studio.is_some());
    app.new_studio = None;

    // Click right below the bottom `▀▀` edge (`area.bottom()`) dismisses Call Sheet.
    draw_to(&mut app, 120, 24);
    let below_y = app.hollywood_hits.area.bottom();
    app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 60, below_y));
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
}
