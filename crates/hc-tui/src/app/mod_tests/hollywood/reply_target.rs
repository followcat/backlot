//! Compose reply-target lifetime tests: `r` answers one ask only —
//! leaving Compose, closing the sheet, answering elsewhere, or empty
//! Backspace returns the line to Hollywood (`acme-shop`, `actor-alice`).
use super::*;
use crate::studio::StudioKey;

fn type_str(app: &mut App, s: &str) {
    for ch in s.chars() {
        key(app, hc_vt::Key::Char(ch));
    }
}

fn asking_app() -> App {
    let mut value = summary_json();
    value["studios"][0]["roles"][1]["work"] = serde_json::json!("waiting");
    value["studios"][0]["roles"][1]["asks"] = serde_json::json!("which shop?");
    value["studios"][0]["exchange"] = serde_json::json!([
        {
            "seq": 11,
            "who": "actor-alice",
            "to": "user",
            "kind": "tell",
            "ask": true,
            "text": "which shop?"
        }
    ]);
    let mut app = app_with_doc();
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    app.hollywood.mode = LayerMode::Collapsed;
    app.set_hollywood(Ok(Some(cfg(None))), std::path::Path::new("config.toml"));
    app
}

fn arm_reply(app: &mut App) {
    app.area = (120, 24);
    alt_y(app);
    app.hollywood_select(Target::Role(
        StudioKey::local("acme-shop"),
        "actor-alice".into(),
    ));
    key(app, hc_vt::Key::Char('r'));
    assert_eq!(
        app.hollywood.reply_to,
        Some((StudioKey::local("acme-shop"), "actor-alice".into()))
    );
    assert!(app.hollywood.editing);
}

fn send_line(app: &mut App, line: &str) {
    type_str(app, line);
    key(app, hc_vt::Key::Enter);
}

fn hollywood_tell(line: &str) -> Vec<String> {
    ["tell", "hollywood", line, "--as", "user"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

#[tokio::test]
async fn tabbing_out_of_compose_drops_the_reply_target() {
    let mut app = asking_app();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    arm_reply(&mut app);

    key(&mut app, hc_vt::Key::Tab);
    assert!(app.hollywood.reply_to.is_none());
    for _ in 0..3 {
        if !app.hollywood.editing {
            key(&mut app, hc_vt::Key::Tab);
        }
    }
    assert!(app.hollywood.editing);
    send_line(&mut app, "status please");
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[hollywood_tell("status please")]
    );
}

#[tokio::test]
async fn alt_y_close_and_reopen_drops_the_reply_target() {
    let mut app = asking_app();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    arm_reply(&mut app);

    alt_y(&mut app);
    assert!(app.hollywood.reply_to.is_none());
    alt_y(&mut app);
    assert!(app.hollywood.editing);
    send_line(&mut app, "status please");
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[hollywood_tell("status please")]
    );
}

#[tokio::test]
async fn a_click_outside_compose_drops_the_reply_target() {
    let mut app = asking_app();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    arm_reply(&mut app);
    draw_to(&mut app, 120, 24);
    let outside = app.hollywood_hits.area.bottom() + 1;

    app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 60, outside));
    assert!(app.hollywood.reply_to.is_none());
    alt_y(&mut app);
    send_line(&mut app, "status please");
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[hollywood_tell("status please")]
    );
}

#[tokio::test]
async fn a_click_elsewhere_in_the_sheet_drops_the_reply_target() {
    let mut app = asking_app();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    arm_reply(&mut app);
    draw_to(&mut app, 120, 24);
    let hints_y = app.hollywood_hits.area.bottom().saturating_sub(2);

    app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 60, hints_y));
    assert!(app.hollywood.reply_to.is_none());
    send_line(&mut app, "status please");
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[hollywood_tell("status please")]
    );
}

#[tokio::test]
async fn an_ask_answered_elsewhere_drops_the_reply_target() {
    let mut app = asking_app();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    arm_reply(&mut app);

    let mut value = summary_json();
    value["studios"][0]["roles"][1]["work"] = serde_json::json!("working");
    value["studios"][0]["exchange"] = serde_json::json!([
        {
            "seq": 11,
            "who": "actor-alice",
            "to": "user",
            "kind": "tell",
            "ask": true,
            "text": "which shop?"
        },
        {
            "seq": 12,
            "who": "user",
            "to": "actor-alice",
            "kind": "tell",
            "text": "answered in the dock"
        }
    ]);
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    assert!(app.hollywood.reply_to.is_none());
    send_line(&mut app, "status please");
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[hollywood_tell("status please")]
    );
}

#[tokio::test]
async fn backspace_on_an_empty_reply_drops_the_reply_target() {
    let mut app = asking_app();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    arm_reply(&mut app);

    key(&mut app, hc_vt::Key::Backspace);
    assert!(app.hollywood.reply_to.is_none());
    send_line(&mut app, "status please");
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[hollywood_tell("status please")]
    );
}

#[tokio::test]
async fn a_sent_reply_gets_a_call_sheet_history_row() {
    let mut app = asking_app();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    arm_reply(&mut app);
    send_line(&mut app, "yes, use acme-shop");
    assert!(recorded.lock().unwrap()[0].starts_with(&["tell".into(), "actor-alice".into()]));

    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(
        text.contains("you")
            && text.contains("→ acme-shop · actor-alice")
            && text.contains("yes, use acme-shop"),
        "reply history row: {text}"
    );
}
