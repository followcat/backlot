//! S5's `Ctrl+B ?` action launcher: the `waiting on you` group first
//! with runnable rows, the reference rows that focus surfaces, the
//! `/pause` filter into the target picker and the compact picker's
//! reason dialog. Plain keeps herdr's overlay untouched.

use super::studio::doc_key;
use super::*;
use crate::app::input::{MouseButton, MouseEventKind, MouseInput, UiEvent};
use crate::hollywood::LayerMode;
use hc_proto::watch::Document;

/// acme-shop: alice at a user-owned prompt #7 on task `t1`, `dave`
/// closed-resumable, tasks `t1` running + `t2` paused; beta-tools
/// ready to finish; one open card `c3`.
pub(super) fn s5_json() -> serde_json::Value {
    serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 1,
        "studios": [
            {
                "id": "acme-shop", "title": "Acme shop", "state": "live",
                "requirement_version": 1, "mode": "standard",
                "open_reviews": 0, "waiting": 1,
                "roles": [
                    { "role": "producer", "name": "producer", "terminal": "t1",
                      "state": "live", "work": "working" },
                    { "role": "actor-alice", "name": "alice", "terminal": "t4",
                      "state": "live", "work": "blocked", "task": "t1",
                      "attention_seq": 7, "attention_user": true,
                      "attention_detail": "rm -rf build/" },
                    { "role": "actor-dave", "name": "dave",
                      "state": "closed", "resumable": true }
                ],
                "tasks": [
                    { "id": "t1", "title": "build api", "status": "running",
                      "priority": "normal", "requirement_version": 1,
                      "rework_rounds": 0, "rework_cap": 3 },
                    { "id": "t2", "title": "docs", "status": "paused",
                      "priority": "high", "requirement_version": 1,
                      "rework_rounds": 0, "rework_cap": 3 }
                ]
            },
            {
                "id": "beta-tools", "title": "Beta tools", "state": "live",
                "requirement_version": 1, "mode": "standard",
                "open_reviews": 0, "waiting": 0,
                "ready_to_finish": true, "tasks_done": 2, "tasks_total": 2,
                "roles": []
            }
        ],
        "hollywood": {
            "state": "live", "runner": "fake", "model": "m", "effort": "high",
            "headline": "acme-shop needs a look",
            "cards": [
                { "id": "c3", "studio": "acme-shop",
                  "action": "pause t1", "why": "the actor is looping" }
            ]
        }
    })
}

pub(super) fn s5_app() -> App {
    let mut app = app_with(&["t1", "t2", "t3", "t4", "t5"]);
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
        .chain([stage])
        .collect();
    app.view.inventory_mut().replace(2, terminals);
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: s5_json(),
            ..Document::default()
        }
        .into(),
    );
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    app.set_hollywood(
        Ok(Some(backlot_stage::config::HollywoodCfg {
            runner: "fake".into(),
            model: "m".into(),
            effort: None,
            key: None,
            default_cast: None,
        })),
        std::path::Path::new("config.toml"),
    );
    app
}

fn recorder() -> (
    std::sync::Arc<std::sync::Mutex<Vec<Vec<String>>>>,
    crate::app::action_tracker::TestRunner,
) {
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    let runner: crate::app::action_tracker::TestRunner = std::sync::Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok(String::new()) })
    });
    (recorded, runner)
}

fn key(app: &mut App, k: hc_vt::Key) {
    app.on_input(
        UiEvent::Key(term_key(k, hc_vt::Mods::NONE, hc_vt::Action::Press)),
        app.area.0,
    );
}

fn mouse(kind: MouseEventKind, col: u16, row: u16) -> MouseInput {
    MouseInput {
        kind,
        col,
        row,
        mods: hc_vt::Mods::NONE,
        sgr_position: None,
    }
}

fn row_labels(app: &App) -> Vec<String> {
    app.help_filtered_groups()
        .into_iter()
        .flat_map(|g| g.rows.into_iter().map(|r| r.label))
        .collect()
}

fn select_row(app: &mut App, needle: &str) {
    let labels = row_labels(app);
    app.help_state.selected = labels
        .iter()
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("a row containing {needle:?}: {labels:?}"));
}

#[tokio::test]
async fn the_waiting_group_leads_with_runnable_rows() {
    let mut app = s5_app();
    app.help = true;
    let groups = app.help_groups();
    assert_eq!(groups[0].title, "waiting on you (↵ runs immediately)");
    let labels: Vec<&str> = groups[0].rows.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(labels[0], "Allow prompt #7 · acme-shop · alice");
    assert_eq!(labels[1], "Deny prompt #7 · acme-shop · alice");
    assert!(labels.contains(&"Apply card c3: pause t1 · acme-shop"));
    assert!(labels.contains(&"Cancel card c3…"));
    assert!(labels.contains(&"Finish studio · beta-tools"));
    // The studio groups and herdr's sections follow — the four
    // studio keys lead the actions group (R3-N6), not a `studio`
    // tail section.
    assert_eq!(groups[1].title, "studio & task actions");
    assert!(
        groups[1]
            .rows
            .iter()
            .any(|r| r.label.starts_with("next studio"))
    );
    // The lifecycle group (W4b) follows — a live studio's roles offer
    // restart — then hollywood and the machines.
    let titles: Vec<&str> = groups.iter().map(|g| g.title.as_str()).collect();
    let hollywood = titles
        .iter()
        .position(|t| *t == "hollywood")
        .unwrap_or_else(|| panic!("{titles:?}"));
    assert!(hollywood > 1, "{titles:?}");
    assert!(!groups.iter().any(|g| g.title == "studio"));
    let text = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(text.contains("Actions & Keybinds"), "{text}");
    assert!(text.contains("waiting on you"), "{text}");
    assert!(text.contains("Allow prompt #7"), "{text}");
    assert!(text.contains("run selected action"), "the footer: {text}");
}

#[tokio::test]
async fn enter_on_a_waiting_row_runs_through_the_tracker() {
    let mut app = s5_app();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.help = true;
    select_row(&mut app, "Allow prompt #7");
    app.help_launcher_activate();
    assert!(!app.help, "the overlay closes");
    let argv = recorded.lock().unwrap();
    assert_eq!(argv.len(), 1, "one action ran: {argv:?}");
    let line = argv[0].join(" ");
    assert!(line.contains("approve"), "{line}");
    assert!(line.contains("actor-alice"), "{line}");
    assert!(line.contains("allow"), "{line}");
}

#[tokio::test]
async fn slash_pause_finds_the_task_row_then_the_picker() {
    let mut app = s5_app();
    app.help = true;
    app.help_state.query.insert("pause");
    select_row(&mut app, "Pause / resume / abandon / take over task…");
    app.help_launcher_activate();
    let picker = app.help_state.picker.as_ref().expect("the picker opened");
    let labels: Vec<&str> = picker.rows.iter().map(|r| r.label.as_str()).collect();
    assert!(labels.contains(&"Pause task t1 · acme-shop"), "{labels:?}");
    assert!(labels.contains(&"Resume task t2 · acme-shop"), "{labels:?}");
    // Enter on the pause row opens S3's reason dialog.
    app.help_picker_run();
    assert!(!app.help);
    let crate::studio_dialog::StudioDialog::Reason(dialog) =
        app.studio_dialog.as_ref().expect("the reason dialog")
    else {
        panic!("a reason dialog");
    };
    let title = dialog.title.to_lowercase();
    assert!(title.contains("pause"), "{}", dialog.title);
    assert!(title.contains("t1"), "{}", dialog.title);
    assert!(title.contains("acme-shop"), "{}", dialog.title);
}

#[tokio::test]
async fn an_enter_while_the_filter_is_focused_runs_the_row() {
    // S5 A1.1: `/pause` then a single Enter opens the task-verb picker —
    // herdr's Enter-closes-overlay rule only survives on the plain path.
    let mut app = s5_app();
    app.help = true;
    key(&mut app, hc_vt::Key::Char('/'));
    for ch in "pause".chars() {
        key(&mut app, hc_vt::Key::Char(ch));
    }
    // `pause` also matches the card row (`pause t1`); arrow to the
    // task-verb row, then one Enter opens its picker.
    key(&mut app, hc_vt::Key::ArrowDown);
    key(&mut app, hc_vt::Key::Enter);
    let picker = app.help_state.picker.as_ref().expect("the picker opened");
    let labels: Vec<&str> = picker.rows.iter().map(|r| r.label.as_str()).collect();
    assert!(labels.contains(&"Pause task t1 · acme-shop"), "{labels:?}");
    assert!(app.help, "the picker floats inside the open overlay");
}

#[tokio::test]
async fn surface_rows_focus_their_surfaces() {
    let mut app = s5_app();
    app.help = true;
    // The dock toggle opens it focused — the keys are the dock's.
    select_row(&mut app, "Toggle Unified Studio Dock");
    app.help_launcher_activate();
    assert!(!app.help);
    assert!(app.panel.open() && app.panel.focused, "the dock owns keys");
    // The Call Sheet row expands the bar — its keys are the sheet's.
    app.help = true;
    select_row(&mut app, "Open Call Sheet");
    app.help_launcher_activate();
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    // `Tell hollywood…` opens the bar editing its input line.
    app.help = true;
    select_row(&mut app, "Tell hollywood what you need");
    app.help_launcher_activate();
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    assert!(app.hollywood.editing, "the tell line is editing");
}

#[tokio::test]
async fn the_machine_and_person_pickers_act() {
    let mut app = s5_app();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.help = true;
    // `Reopen closed person…` lists dave and reopens him directly.
    select_row(&mut app, "Reopen closed person");
    app.help_launcher_activate();
    let picker = app.help_state.picker.as_ref().expect("the picker opened");
    let labels: Vec<&str> = picker.rows.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(labels, ["Reopen dave · acme-shop"]);
    app.help_picker_run();
    let argv = recorded.lock().unwrap();
    assert_eq!(argv.len(), 1, "{argv:?}");
    assert!(
        argv[0].join(" ").contains("reopen"),
        "{}",
        argv[0].join(" ")
    );
    assert!(argv[0].join(" ").contains("dave"), "{}", argv[0].join(" "));
}

#[tokio::test]
async fn the_legend_stays_pinned_and_the_sizes_hold() {
    let mut app = s5_app();
    app.help = true;
    let text = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(text.contains("◐ work"), "{text}");
    assert!(text.contains("· wait"), "{text}");
    app.help_state.scroll = usize::MAX;
    let text = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(text.contains("◐ work"), "scrolled: {text}");
    // Centered inside `pane_surface` at desktop widths (132x46 at 160x47,
    // 92x35 at 120x36), falling back to full area on narrow screens.
    for (cols, rows, want) in [
        (160u16, 47u16, (96u16, 35u16)),
        (120, 36, (86, 31)),
        (80, 24, (80, 22)),
        (60, 24, (60, 22)),
        (40, 24, (40, 22)),
    ] {
        draw_to(&mut app, cols, rows);
        assert_eq!(
            (app.help_hits.area.width, app.help_hits.area.height),
            want,
            "{cols}x{rows}"
        );
    }
}

#[tokio::test]
async fn arrows_move_the_selection_and_the_row_hit_runs() {
    let mut app = s5_app();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    app.help = true;
    app.area = (120, 36);
    draw_to(&mut app, 120, 36);
    // Down moves into the second waiting row — Deny.
    app.help_launcher_move(1);
    assert_eq!(app.help_state.selected, 1);
    // A click on a row selects and runs it.
    let (rect, idx) = app
        .help_hits
        .rows
        .first()
        .cloned()
        .expect("a row hit in the overlay");
    assert_eq!(idx, 0);
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        rect.x + 2,
        rect.y,
    ));
    assert!(!app.help, "the click ran the row");
    assert_eq!(recorded.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn studio_mode_without_studios_is_herdrs_overlay() {
    let mut app = app_with(&["t1"]);
    app.set_plain(false);
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: serde_json::json!({ "schema": "backlot/1", "studios": [] }),
            ..Document::default()
        }
        .into(),
    );
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    // U2: studio mode is always latched off `--plain`; D6 keeps the
    // no-studios frame on herdr's overlay.
    assert!(!app.help_launcher());
    app.help = true;
    let text = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(text.contains("keybinds"), "{text}");
    assert!(!text.contains("waiting on you"), "{text}");
    assert!(!text.contains("Actions & Keybinds"), "{text}");
}

#[tokio::test]
async fn plain_keeps_enter_as_close() {
    let mut app = s5_app();
    app.set_plain(true);
    app.help = true;
    key(&mut app, hc_vt::Key::Enter);
    assert!(!app.help, "plain's Enter still just closes");
}

#[tokio::test]
async fn an_applied_filter_shows_and_esc_clears_then_closes() {
    let mut app = s5_app();
    app.help = true;
    app.help_state.query.insert("pause");
    // The applied query stays on the header line once the editor is
    // unfocused — `/ pause` dimmed, `esc clears` while it fits.
    let text = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(text.contains("/ pause"), "{text}");
    assert!(text.contains("esc clears"), "{text}");
    // Esc clears the filter first; the overlay stays open.
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.help, "first Esc clears, not closes");
    assert!(app.help_state.query.as_str().is_empty());
    let text = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(text.contains("press / to filter"), "{text}");
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.help, "second Esc closes");
}
