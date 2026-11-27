//! Group 3 fail-first tests (§1.3):
//! - Lot inline compose target prefill (`t`/`r`/`i`) without `/tell <role> ` clutter
//! - Empty compose placeholder (`› tell <target>…`) and `Tab` cycling
//!   `producer` -> cursor role -> `hollywood` (vs `/verb` completion when `/…`)
//! - Sending to a studio role or to `hollywood` from Lot & Cast
//! - `TALK` visible delivery lifecycle: `… sending…`, `… queued · <reason>`
//!   (`Busy`, `Prompt`, `RateLimited`, `OutOfQuota`, `Handoff`, `Down`,
//!   `Starting`, `WaitingQuiet`), `✓ delivered · <age>`, `⠶ thinking…`, and
//!   clearing on reply.

use super::*;
use crate::action::mark;
use crate::app::input::UiEvent;
use crate::focus::{LotLevel, TellTarget, WorkspaceRegion};
use crate::panel::Target;
use crate::studio::StudioKey;
use hc_proto::watch::{DocKey, Document};

fn key(app: &mut App, k: hc_vt::Key) {
    key_mods(app, k, hc_vt::Mods::NONE);
}

fn key_mods(app: &mut App, k: hc_vt::Key, mods: hc_vt::Mods) {
    app.on_input(
        UiEvent::Key(term_key(k, mods, hc_vt::Action::Press)),
        app.area.0,
    );
}

fn prefix_key(app: &mut App, ch: char) {
    key_mods(app, hc_vt::Key::Char('b'), hc_vt::Mods::CTRL);
    key(app, hc_vt::Key::Char(ch));
}

fn alt_l(app: &mut App) {
    key_mods(app, hc_vt::Key::Char('l'), hc_vt::Mods::ALT);
}

fn draft(app: &App, studio: &str) -> String {
    app.panel
        .draft(&StudioKey::local(studio))
        .map(|d| d.as_str().to_owned())
        .unwrap_or_default()
}

fn runner() -> (
    std::sync::Arc<std::sync::Mutex<Vec<Vec<String>>>>,
    crate::app::action_tracker::TestPanelRunner,
) {
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    (
        recorded,
        std::sync::Arc::new(move |argv: Vec<String>| {
            sink.lock().unwrap().push(argv);
            Box::pin(async { Ok(()) })
        }),
    )
}

fn base_tell_app(stage_live: bool, studio_doc: serde_json::Value) -> App {
    let mut app = app_with(&["t1", "t2", "t3"]);
    app.set_plain(false);
    let mut terms: Vec<_> = app.view.inventory.terminals().values().cloned().collect();
    if stage_live {
        let mut stage = crate::sidebar::fixture::term("t9");
        stage.name = Some("backlot-stage".into());
        stage.state = "live".into();
        terms.push(stage);
    }
    app.view.inventory_mut().replace(2, terms);
    app.view.documents_mut().insert(
        DocKey {
            machine: None,
            name: "backlot".into(),
        },
        Document {
            value: serde_json::json!({
                "schema": "backlot/1",
                "updated_ms": 250_000,
                "hollywood": { "state": "live", "runner": "fake", "model": "m", "effort": "high" },
                "studios": [studio_doc],
            }),
            ..Document::default()
        }
        .into(),
    );
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    app.hollywood.mode = crate::hollywood::LayerMode::Collapsed;
    app.area = (140, 38);
    app
}

fn sample_studio_doc() -> serde_json::Value {
    serde_json::json!({
        "id": "acme-shop",
        "title": "Acme shop",
        "state": "live",
        "requirement_version": 2,
        "mode": "standard",
        "open_reviews": 0,
        "waiting": 1,
        "roles": [
            {
                "role": "producer", "name": "producer", "terminal": "t1",
                "state": "live", "work": "waiting",
                "asks": "Ship OAuth v2 or keep v1?", "asks_seq": 48, "since_ms": 1_000
            },
            {
                "role": "director", "name": "director", "terminal": "t2",
                "state": "live", "work": "waiting"
            },
            {
                "role": "actor-t1", "name": "actor-t1", "terminal": "t3",
                "state": "live", "work": "working", "task": "t1"
            }
        ],
        "exchange": [
            { "seq": 10, "who": "user", "to": "producer", "ts_ms": 10_000, "text": "Prioritize v2 schema." }
        ]
    })
}

#[tokio::test]
async fn t_and_r_on_role_or_ask_prefill_target_without_slash_text() {
    let mut app = base_tell_app(true, sample_studio_doc());
    app.shell_state.sidebar_collapsed = true;
    let (recorded, run) = runner();
    app.set_panel_runner(run);

    // Enter Lot inside acme-shop, move down from producer ask to director, press `t`.
    alt_l(&mut app);
    key(&mut app, hc_vt::Key::Char('j'));
    key(&mut app, hc_vt::Key::Char('t'));

    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::Compose {
            studio: "acme-shop".into(),
            target: TellTarget::Role("director".into()),
        })
    );
    assert_eq!(
        draft(&app, "acme-shop"),
        "",
        "`t` on a person pre-targets without injecting `/tell <role> `"
    );
    let rendered = screen_lines(&draw_to(&mut app, 140, 38)).join("\n");
    assert!(
        rendered.contains("tell director"),
        "placeholder names selected role:\n{rendered}"
    );

    for ch in "hold t2".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec![
            "tell".to_string(),
            "director".into(),
            "--text".into(),
            "hold t2".into(),
            "--studio".into(),
            "acme-shop".into(),
            "--as".into(),
            "user".into(),
        ]]
    );

    // Back in InsideStudio: select producer (open ask), press `r` to reply.
    recorded.lock().unwrap().clear();
    app.panel.selected = Some(Target::Person("acme-shop".into(), "producer".into()));
    app.sync_focus();
    key(&mut app, hc_vt::Key::Char('r'));
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::Compose {
            studio: "acme-shop".into(),
            target: TellTarget::Role("producer".into()),
        })
    );
    assert_eq!(draft(&app, "acme-shop"), "");
    for ch in "use v2".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec![
            "tell".to_string(),
            "producer".into(),
            "--text".into(),
            "use v2".into(),
            "--studio".into(),
            "acme-shop".into(),
            "--as".into(),
            "user".into(),
        ]]
    );
}

#[tokio::test]
async fn empty_compose_tab_cycles_producer_cursor_role_hollywood_and_keeps_slash_complete() {
    let mut app = base_tell_app(true, sample_studio_doc());
    let (recorded, run) = runner();
    app.set_panel_runner(run);

    // Select director, press `i` on director (or press `Tab` from producer while director was remembered).
    alt_l(&mut app);
    app.panel.selected = Some(Target::Person("acme-shop".into(), "director".into()));
    app.sync_focus();
    // From Compose targeting `producer` with remembered cursor role `director`:
    app.enter_lot_compose(&StudioKey::local("acme-shop"), TellTarget::default());
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::Compose {
            studio: "acme-shop".into(),
            target: TellTarget::Role("producer".into()),
        })
    );

    // 1st Tab on empty compose jumps from producer -> cursor role (`director`).
    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::Compose {
            studio: "acme-shop".into(),
            target: TellTarget::Role("director".into()),
        })
    );
    let dir_screen = screen_lines(&draw_to(&mut app, 140, 38)).join("\n");
    assert!(dir_screen.contains("tell director"), "{dir_screen}");

    // 2nd Tab -> `hollywood`.
    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::Compose {
            studio: "acme-shop".into(),
            target: TellTarget::Hollywood,
        })
    );
    let hw_screen = screen_lines(&draw_to(&mut app, 140, 38)).join("\n");
    assert!(hw_screen.contains("tell hollywood"), "{hw_screen}");

    for ch in "adjust scope".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec![
            "tell".to_string(),
            "hollywood".into(),
            "adjust scope".into(),
            "--as".into(),
            "user".into(),
        ]]
    );

    // `/verb` completion still runs when draft starts with `/`.
    key(&mut app, hc_vt::Key::Char('/'));
    for ch in "sto".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(draft(&app, "acme-shop"), "/stop ");
}

#[tokio::test]
async fn t_in_cast_opens_lot_compose_for_selected_role() {
    let mut app = base_tell_app(true, sample_studio_doc());
    app.shell_state.sidebar_collapsed = false;

    prefix_key(&mut app, 'a');
    assert_eq!(app.focus.workspace, WorkspaceRegion::Cast);
    key(&mut app, hc_vt::Key::Char('j')); // move from producer to director
    key(&mut app, hc_vt::Key::Char('t'));
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::Compose {
            studio: "acme-shop".into(),
            target: TellTarget::Role("director".into()),
        })
    );
    assert_eq!(draft(&app, "acme-shop"), "");
    let screen = screen_lines(&draw_to(&mut app, 140, 38)).join("\n");
    assert!(screen.contains("tell director"), "{screen}");
    let (recorded, run) = runner();
    app.set_panel_runner(run);
    for ch in "focus api".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec![
            "tell".to_string(),
            "director".into(),
            "--text".into(),
            "focus api".into(),
            "--studio".into(),
            "acme-shop".into(),
            "--as".into(),
            "user".into(),
        ]]
    );
}

#[tokio::test]
async fn talk_row_renders_sending_queued_reasons_delivered_age_and_thinking() {
    let render_with = |stage_live: bool, doc: serde_json::Value, start_in_flight: bool| {
        let mut app = base_tell_app(stage_live, doc);
        app.shell_state.sidebar_collapsed = true;
        app.panel.drag_width = Some(48);
        alt_l(&mut app);
        if start_in_flight {
            let _ = app.actions.start(crate::studio_cmds::tell(
                &StudioKey::local("acme-shop"),
                "producer",
                "hi",
            ));
        }
        let buf = draw_to(&mut app, 180, 38);
        let _ = mark::role(&StudioKey::local("acme-shop"), "producer");
        screen_lines(&buf).join("\n")
    };

    // 1. In-flight client action -> `… sending…`
    let sending = render_with(true, sample_studio_doc(), true);
    assert!(sending.contains("sending"), "in-flight cue:\n{sending}");

    // 2. Queued with Busy reason
    let mut busy_doc = sample_studio_doc();
    busy_doc["pending_tell"] = serde_json::json!(true);
    busy_doc["roles"][0]["queue"] = serde_json::json!({
        "count": 1,
        "reason": "busy",
        "detail": "working on t1 review"
    });
    let busy = render_with(true, busy_doc, false);
    assert!(
        busy.contains("queued") && busy.contains("busy") && busy.contains("working on t1 review"),
        "busy queued reason:\n{busy}"
    );

    // 3. Queued with Prompt reason
    let mut prompt_doc = sample_studio_doc();
    prompt_doc["pending_tell"] = serde_json::json!(true);
    prompt_doc["roles"][0]["attention_user"] = serde_json::json!(true);
    prompt_doc["roles"][0]["queue"] = serde_json::json!({
        "count": 1,
        "reason": "prompt",
        "detail": "Run git status?"
    });
    let prompt = render_with(true, prompt_doc, false);
    assert!(
        prompt.contains("queued")
            && prompt.contains("prompt")
            && prompt.contains("[y]")
            && prompt.contains("[p]"),
        "prompt queued reason:\n{prompt}"
    );

    // 4. Queued with RateLimited reason
    let mut rl_doc = sample_studio_doc();
    rl_doc["pending_tell"] = serde_json::json!(true);
    rl_doc["roles"][0]["queue"] = serde_json::json!({
        "count": 1,
        "reason": "rate_limited",
        "detail": "resets 18:30"
    });
    let rl = render_with(true, rl_doc, false);
    assert!(
        rl.contains("queued") && rl.contains("rate limited") && rl.contains("resets 18:30"),
        "rate limited queued reason:\n{rl}"
    );

    // 5. Queued with OutOfQuota reason
    let mut quota_doc = sample_studio_doc();
    quota_doc["pending_tell"] = serde_json::json!(true);
    quota_doc["roles"][0]["queue"] = serde_json::json!({
        "count": 1,
        "reason": "out_of_quota",
        "detail": "weekly limit reached"
    });
    let quota = render_with(true, quota_doc, false);
    assert!(
        quota.contains("queued") && quota.contains("out of quota") && quota.contains("recast"),
        "out of quota queued reason:\n{quota}"
    );

    // 6. Stage down derived locally when stage terminal is stopped
    let mut down_doc = sample_studio_doc();
    down_doc["pending_tell"] = serde_json::json!(true);
    let down = render_with(false, down_doc, false);
    assert!(
        down.contains("queued") && down.contains("stage stopped") && down.contains("[s] start"),
        "stage down derived reason:\n{down}"
    );

    // 7. Delivered while waiting -> `✓ delivered`
    let delivered = render_with(true, sample_studio_doc(), false);
    assert!(
        delivered.contains("delivered"),
        "delivered confirmation:\n{delivered}"
    );

    // 8. Delivered + role working -> `⠶ thinking`
    let mut thinking_doc = sample_studio_doc();
    thinking_doc["roles"][0]["work"] = serde_json::json!("working");
    let thinking = render_with(true, thinking_doc, false);
    assert!(
        thinking.contains("thinking"),
        "thinking after delivery:\n{thinking}"
    );

    // 9. Receiver replied -> delivery cue clears cleanly
    let mut replied_doc = sample_studio_doc();
    replied_doc["exchange"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "seq": 11, "who": "producer", "to": "user", "text": "Switched to v2 schema."
        }));
    let replied = render_with(true, replied_doc, false);
    assert!(!replied.contains("queued"), "{replied}");
    assert!(!replied.contains("delivered"), "{replied}");
}
