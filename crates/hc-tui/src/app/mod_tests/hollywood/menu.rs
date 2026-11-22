//! `m` on a selected band target — the row's context menu must open
//! AND paint (the sim's invisible-menu bug): the frame's menu anchors
//! at its rect, a band row's at its row, and a target the last frame
//! never drew falls back to the band's corner — never nowhere.
use super::*;
use crate::chrome::menu::ContextMenuTarget;

/// Select the finished `beta-tools` frame, press `m`: the studio's
/// menu opens with its items, and the next draw paints the box —
/// `m` must never leave the keys in a menu-less dead end.
#[tokio::test]
async fn m_on_a_studio_frame_opens_a_visible_menu() {
    let mut app = app_with_hollywood();
    app.area = (120, 30);
    alt_y(&mut app);
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    draw_to(&mut app, 120, 30);
    app.hollywood.selected = Some(Target::Studio(crate::studio::StudioKey {
        machine: None,
        id: "beta-tools".into(),
    }));
    key(&mut app, hc_vt::Key::Char('m'));
    let Some(menu) = app.menu.as_ref() else {
        panic!("m on a studio frame opens its menu");
    };
    assert!(
        matches!(menu.target, ContextMenuTarget::Studio(_)),
        "a frame's menu is the studio's"
    );
    let text = screen_lines(&draw_to(&mut app, 120, 30)).join("\n");
    assert!(
        text.contains("Inspect studio sheet"),
        "the menu paints: {text}"
    );
}

/// A prompt row's menu. `actor-alice` gets `attention_user` so the
/// row exists in `hollywood_targets` — a target that is not in the
/// list silently selects index 0 (the title), which opens nothing.
#[tokio::test]
async fn m_on_a_waiting_row_opens_its_menu() {
    let mut app = app_with_hollywood();
    let mut doc = summary_json();
    doc["studios"][0]["roles"][1]["attention_user"] = serde_json::json!(true);
    doc["studios"][0]["roles"][1]["attention_seq"] = serde_json::json!(7);
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: doc,
            ..Document::default()
        }
        .into(),
    );
    app.area = (120, 30);
    alt_y(&mut app);
    draw_to(&mut app, 120, 30);
    let target = app
        .hollywood_targets()
        .into_iter()
        .find(|t| matches!(t, Target::Prompt(_, role) if role == "actor-alice"))
        .expect("the prompt row is a target");
    app.hollywood.selected = Some(target);
    key(&mut app, hc_vt::Key::Char('m'));
    let Some(menu) = app.menu.as_ref() else {
        panic!("a prompt row's menu opens");
    };
    assert!(
        matches!(menu.target, ContextMenuTarget::Studio(_)),
        "the menu is the role's"
    );
    let text = screen_lines(&draw_to(&mut app, 120, 30)).join("\n");
    assert!(text.contains("Allow prompt"), "the menu paints: {text}");
}

/// `m` on a target with no menu (the title, headline, calm or input
/// rows — all real band targets) opens nothing — and the keys stay
/// with the band, not a phantom layer. Esc still collapses
/// afterwards. The title is the always-present menu-less target.
#[tokio::test]
async fn m_on_a_menu_less_target_opens_nothing_and_esc_still_collapses() {
    let mut app = app_with_hollywood();
    app.area = (120, 30);
    alt_y(&mut app);
    draw_to(&mut app, 120, 30);
    app.hollywood.selected = Some(Target::Title);
    key(&mut app, hc_vt::Key::Char('m'));
    assert!(app.menu.is_none(), "the title row has no menu");
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
}

/// The selection can outlive its row — a publish drops the studio the
/// cursor sat on. `m` must not silently target the title (index 0 has
/// no menu, so the press looked dead in the sim): the fallback is the
/// first frame, and its menu opens.
#[tokio::test]
async fn a_vanished_selection_falls_back_to_the_first_frame_for_m() {
    let mut app = app_with_hollywood();
    app.area = (120, 30);
    alt_y(&mut app);
    draw_to(&mut app, 120, 30);
    // A studio that is no longer in the document — as if a publish
    // dropped it between the draw and the keypress.
    app.hollywood.selected = Some(Target::Studio(crate::studio::StudioKey {
        machine: None,
        id: "gone-shop".into(),
    }));
    key(&mut app, hc_vt::Key::Char('m'));
    let Some(menu) = app.menu.as_ref() else {
        panic!("the fallback frame's menu opens, not silence");
    };
    let ContextMenuTarget::Studio(menu) = &menu.target else {
        panic!("a frame's menu is the studio's");
    };
    assert_eq!(
        menu.studio.as_ref().map(|key| key.id.as_str()),
        Some("acme-shop"),
        "the first frame, not the title"
    );
}

#[tokio::test]
async fn m_on_a_talk_or_headline_row_offers_raw_hollywood_terminal_focus() {
    let mut app = app_with_hollywood();
    let mut doc = summary_json();
    doc["hollywood"]["headline_seq"] = serde_json::json!(9);
    doc["hollywood"]["exchange"] = serde_json::json!([
        { "seq": 8, "who": "user", "text": "status?" },
        { "seq": 9, "who": "hollywood", "text": "acme-shop needs a look" }
    ]);
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: doc,
            ..Document::default()
        }
        .into(),
    );
    app.area = (120, 30);
    alt_y(&mut app);
    draw_to(&mut app, 120, 30);
    app.hollywood.selected = Some(Target::Talk(9));
    key(&mut app, hc_vt::Key::Char('m'));
    assert!(app.menu.is_some(), "`m` on Talk opens the entry menu");
    let text = screen_lines(&draw_to(&mut app, 120, 30)).join("\n");
    assert!(text.contains("Read entry #9"), "{text}");
    assert!(text.contains("Focus Hollywood terminal"), "{text}");

    key(&mut app, hc_vt::Key::Char('t'));
    assert_eq!(app.focused.as_deref(), Some("t10"));
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);
}
