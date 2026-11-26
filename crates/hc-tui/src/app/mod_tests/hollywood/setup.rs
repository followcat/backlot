//! The in-TUI Hollywood setup dialog: the no-agent band's setup
//! row, the panel's `hollywood setup` hint and `c` on a configured
//! bar all open the same modal; it probes `runners --json`, offers
//! the runner / that runner's model catalog (`auto` first, free
//! text kept) / its effort list, and applies through `backlot
//! hollywood setup` — the same write the CLI does.
use super::*;

/// Two installed runners and a missing one: `fake` (one model, two
/// efforts), `claude` (one model, three efforts), `cur` (no effort
/// list), `codex` missing — only the found rows are pickable.
fn setup_runners_json() -> String {
    serde_json::json!([
        { "runner": "fake", "status": "found", "version": "1",
          "models": ["m"], "efforts": ["low", "high"] },
        { "runner": "claude", "status": "found", "version": "1",
          "models": ["sonnet", "opus"], "efforts": ["low", "medium", "high"] },
        { "runner": "cursor", "status": "found", "version": "1",
          "models": ["x-1"], "efforts": [] },
        { "runner": "codex", "status": "missing", "version": null,
          "models": [], "efforts": [] }
    ])
    .to_string()
}

/// The dialog's own runner seam: records every argv, answers
/// `runners --json` with the fixture and `hollywood setup` with the
/// write's own line.
fn install(app: &mut App) -> std::sync::Arc<std::sync::Mutex<Vec<Vec<String>>>> {
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    app.set_ns_run(std::sync::Arc::new(move |_machine, argv, _stdin| {
        sink.lock().unwrap().push(argv.clone());
        let out = if argv.first().map(String::as_str) == Some("runners") {
            Ok(setup_runners_json())
        } else {
            Ok("[hollywood] written".to_string())
        };
        Box::pin(async move { out })
    }));
    recorded
}

/// Let the spawned probe send, then land every reply.
async fn drain_ns(app: &mut App) {
    for _ in 0..20 {
        tokio::task::yield_now().await;
        while let Ok(reply) = app.ns_rx.try_recv() {
            app.on_ns_reply(reply);
        }
    }
}

fn down(app: &mut App) {
    key(app, hc_vt::Key::ArrowDown);
}

#[tokio::test]
async fn the_setup_row_opens_the_dialog_and_apply_writes_the_table() {
    let mut app = app_with_doc();
    app.area = (120, 24);
    app.set_hollywood(Ok(None), std::path::Path::new("config.toml"));
    let recorded = install(&mut app);
    alt_y(&mut app);
    app.hollywood.selected = Some(Target::Setup);
    key(&mut app, hc_vt::Key::Enter);
    assert!(
        app.hw_switch.is_some(),
        "Enter on the setup row opens the dialog"
    );
    drain_ns(&mut app).await;
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec!["runners".to_string(), "--json".to_string()]],
        "the dialog probes runners --json on this machine"
    );
    // The first found runner's defaults are seated: fake, auto, low.
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(text.contains("Hollywood setup"), "{text}");
    assert!(text.contains("fake") && text.contains("auto"), "{text}");
    // runner, model, effort, then apply.
    down(&mut app);
    down(&mut app);
    down(&mut app);
    key(&mut app, hc_vt::Key::Enter);
    assert!(app.hw_switch.is_none(), "Apply closes the dialog");
    assert_eq!(
        recorded.lock().unwrap().last().unwrap(),
        &vec![
            "hollywood".to_string(),
            "setup".to_string(),
            "--runner".to_string(),
            "fake".to_string(),
            "--model".to_string(),
            "auto".to_string(),
            "--effort".to_string(),
            "low".to_string(),
            "--as".to_string(),
            "user".to_string(),
        ],
        "the apply rides hollywood setup as the user"
    );
    // The write's Ok configures the bar — the setup row is gone and
    // the tell line takes its place.
    let result = app.action_rx.recv().await.expect("the setup reply");
    app.on_action_reply(result);
    assert!(app.has_hollywood_cfg, "a written table configures the bar");
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(text.contains("tell hollywood"), "{text}");
    assert!(!text.contains("hollywood setup"), "{text}");
}

#[tokio::test]
async fn c_is_inert_and_the_settings_path_reopens_the_dialog_prefilled() {
    let mut app = app_with_hollywood();
    app.area = (120, 24);
    let _recorded = install(&mut app);
    alt_y(&mut app);
    key(&mut app, hc_vt::Key::Escape);
    // R3-5: `c` is the dock's collapse key only — inert on the Call
    // Sheet; Hollywood settings live on the pill menu and `Ctrl+B s
    // → studio → overseer → ⏎`.
    key(&mut app, hc_vt::Key::Char('c'));
    assert!(app.hw_switch.is_none(), "c is inert on the Call Sheet");
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    app.hw_setup_open();
    assert!(app.hw_switch.is_some(), "the settings path opened it");
    drain_ns(&mut app).await;
    // The cfg's own values are seated: runner fake (found index 0),
    // model model-h verbatim, effort high (index 1 of low/high).
    let d = app.hw_switch.as_ref().unwrap();
    assert_eq!(d.model, "model-h");
    assert_eq!(d.effort, 1, "high is the second effort");
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(text.contains("model-h"), "the cfg's model prefills: {text}");
    assert!(text.contains("high"), "the cfg's effort prefills: {text}");
}

#[tokio::test]
async fn the_model_row_offers_the_catalog_auto_first_and_free_text() {
    let mut app = app_with_doc();
    app.area = (120, 24);
    app.set_hollywood(Ok(None), std::path::Path::new("config.toml"));
    let _recorded = install(&mut app);
    alt_y(&mut app);
    app.hollywood.selected = Some(Target::Setup);
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;
    // → claude on the runner row, then ⏎ on the model row opens its
    // catalog — auto first, the runner's models after.
    key(&mut app, hc_vt::Key::ArrowRight);
    down(&mut app);
    key(&mut app, hc_vt::Key::Enter);
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(text.contains("auto") && text.contains("sonnet"), "{text}");
    key(&mut app, hc_vt::Key::Escape);
    // `e` keeps free input — a name the catalog lacks is the model.
    key(&mut app, hc_vt::Key::Char('e'));
    key(&mut app, hc_vt::Key::Char('z'));
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(app.hw_switch.as_ref().unwrap().model, "autoz");
}

#[tokio::test]
async fn a_runner_without_efforts_hides_the_effort_row() {
    let mut app = app_with_doc();
    app.area = (120, 24);
    app.set_hollywood(Ok(None), std::path::Path::new("config.toml"));
    let recorded = install(&mut app);
    alt_y(&mut app);
    app.hollywood.selected = Some(Target::Setup);
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;
    // fake → claude → cursor: the effort row is gone for `cursor`,
    // and the apply argv carries no --effort flag.
    key(&mut app, hc_vt::Key::ArrowRight);
    key(&mut app, hc_vt::Key::ArrowRight);
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(!text.contains("effort"), "no effort row for cursor: {text}");
    down(&mut app);
    down(&mut app);
    key(&mut app, hc_vt::Key::Enter);
    let argv = recorded.lock().unwrap().last().unwrap().clone();
    assert!(
        !argv.iter().any(|a| a == "--effort"),
        "no --effort for a runner without a list: {argv:?}"
    );
}

#[tokio::test]
async fn the_panels_setup_hint_opens_the_dialog() {
    let mut app = app_with_doc();
    app.area = (200, 24);
    app.set_hollywood(Ok(None), std::path::Path::new("config.toml"));
    let _recorded = install(&mut app);
    app.panel.mode = crate::panel::PanelMode::Expanded;
    app.panel.focused = true;
    app.panel.selected = Some(crate::panel::Target::Setup);
    key(&mut app, hc_vt::Key::Enter);
    assert!(
        app.hw_switch.is_some(),
        "Enter on the panel's setup hint opens the dialog"
    );
    drain_ns(&mut app).await;
}

#[tokio::test]
async fn the_settings_overseer_row_opens_the_dialog() {
    let mut app = app_with_doc();
    app.area = (120, 24);
    app.set_hollywood(Ok(None), std::path::Path::new("config.toml"));
    let _recorded = install(&mut app);
    app.open_studio_settings();
    app.select_settings_choice(crate::chrome::settings::STUDIO_OVERSEER);
    app.apply_settings_choice();
    assert!(
        app.hw_switch.is_some(),
        "Enter on the overseer row opens the dialog"
    );
}

#[tokio::test]
async fn esc_closes_the_dialog_without_writing() {
    let mut app = app_with_doc();
    app.area = (120, 24);
    app.set_hollywood(Ok(None), std::path::Path::new("config.toml"));
    let recorded = install(&mut app);
    alt_y(&mut app);
    app.hollywood.selected = Some(Target::Setup);
    key(&mut app, hc_vt::Key::Enter);
    drain_ns(&mut app).await;
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.hw_switch.is_none());
    assert_eq!(
        recorded.lock().unwrap().len(),
        1,
        "only the probe ran — no setup write"
    );
}
