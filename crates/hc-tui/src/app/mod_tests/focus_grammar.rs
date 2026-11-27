//! Group 2 fail-first tests: universal in-region key grammar (§1.2.3),
//! Lot Studios/InsideStudio/RowActions navigation, Tree & Cast actions,
//! and modal bug fixes (TaskSheet BackTab, UpdateCard PageUp + buttons,
//! NewStudio Step 2 BackTab/ArrowUp, Recast/Restore Esc step-back,
//! ContextMenu j/k/Tab/BackTab/Left).

use super::*;
use crate::app::input::UiEvent;
use crate::focus::{CallSheetZone, LotLevel, WorkspaceRegion};
use crate::new_studio::{Probe, RunnerRow, Step, Step2Field};
use crate::panel::Target;
use crate::sheet::SheetTab;
use crate::studio::StudioKey;
use crate::studio_dialog::{RecastDialog, RecastStep, RestoreDialog, RestoreStep, StudioDialog};
use crate::update_state::CachedRelease;
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

fn alt_l(app: &mut App) {
    key_mods(app, hc_vt::Key::Char('l'), hc_vt::Mods::ALT);
}

fn prefix_key(app: &mut App, ch: char) {
    key_mods(app, hc_vt::Key::Char('b'), hc_vt::Mods::CTRL);
    key(app, hc_vt::Key::Char(ch));
}

fn grammar_doc(two_studios: bool, with_tasks_and_talk: bool) -> serde_json::Value {
    let mut studios = vec![serde_json::json!({
        "id": "acme-shop",
        "title": "Acme shop",
        "state": "live",
        "requirement_version": 2,
        "mode": "standard",
        "open_reviews": 1,
        "waiting": 1,
        "tasks_done": 1,
        "tasks_total": if with_tasks_and_talk { 2 } else { 0 },
        "roles": [
            {
                "role": "producer", "name": "producer", "terminal": "t1",
                "state": "live", "work": "waiting",
                "asks": "Ship OAuth v2 or keep v1?", "asks_seq": 48, "since_ms": 1_000
            },
            {
                "role": "director", "name": "director", "terminal": "t2",
                "state": "live", "work": "working"
            }
        ],
        "tasks": if with_tasks_and_talk {
            serde_json::json!([
                {
                    "id": "t1", "title": "auth middleware", "status": "accepted",
                    "priority": "normal", "requirement_version": 2,
                    "rework_rounds": 0, "rework_cap": 3
                },
                {
                    "id": "t2", "title": "checkout webhooks", "status": "created",
                    "priority": "normal", "requirement_version": 2,
                    "rework_rounds": 0, "rework_cap": 3
                }
            ])
        } else {
            serde_json::json!([])
        },
        "exchange": if with_tasks_and_talk {
            serde_json::json!([
                { "seq": 10, "who": "user", "text": "Prioritize v2 schema.", "file": "messages/10.txt" },
                { "seq": 11, "who": "producer", "text": "Updating webhooks.", "file": "reports/11.md" }
            ])
        } else {
            serde_json::json!([])
        }
    })];
    if two_studios {
        studios.push(serde_json::json!({
            "id": "beta-api",
            "title": "Beta API",
            "state": "live",
            "requirement_version": 1,
            "mode": "standard",
            "open_reviews": 1,
            "waiting": 1,
            "roles": [
                {
                    "role": "producer", "name": "beta-prod", "terminal": "t7",
                    "state": "live", "work": "waiting",
                    "attention_user": true, "can_answer_prompt": true,
                    "attention": "Allow bash run?"
                }
            ]
        }));
    }
    serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 1,
        "hollywood": { "state": "live", "runner": "fake", "model": "m", "effort": "high" },
        "studios": studios
    })
}

fn grammar_app(two_studios: bool, with_tasks_and_talk: bool) -> App {
    let mut app = app_with(&["t1", "t2", "t7", "t99"]);
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
            value: grammar_doc(two_studios, with_tasks_and_talk),
            ..Document::default()
        }
        .into(),
    );
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    app.area = (160, 40);
    app
}

#[tokio::test]
async fn lot_studios_tab_cycles_urgent_studios_and_arrows_enter_and_fold() {
    let mut app = grammar_app(true, false);
    prefix_key(&mut app, 'y');
    assert_eq!(app.focus.workspace, WorkspaceRegion::Lot(LotLevel::Studios));
    assert_eq!(app.panel.selected, Some(Target::Studio("acme-shop".into())));

    // Binding Tab rule (#212): Tab from a studio header steps sections inside that studio
    // (`Header -> People -> ... -> next studio Header`, moving focus into the entered studio),
    // and Shift+Tab reverses cleanly.
    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::InsideStudio("acme-shop".into()))
    );
    assert_eq!(
        app.panel.selected,
        Some(Target::Person("acme-shop".into(), "producer".into()))
    );
    key_mods(&mut app, hc_vt::Key::BackTab, hc_vt::Mods::SHIFT);
    assert_eq!(app.panel.selected, Some(Target::Studio("acme-shop".into())));
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(app.focus.workspace, WorkspaceRegion::Lot(LotLevel::Studios));

    // ArrowLeft folds unfolded studio without leaving Studios.
    key(&mut app, hc_vt::Key::ArrowLeft);
    assert!(
        app.panel
            .collapsed_blocks
            .contains(&StudioKey::local("acme-shop"))
    );
    assert_eq!(app.focus.workspace, WorkspaceRegion::Lot(LotLevel::Studios));

    // ArrowRight enters the studio (InsideStudio) and unfolds it.
    key(&mut app, hc_vt::Key::ArrowRight);
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::InsideStudio("acme-shop".into()))
    );
    assert!(
        !app.panel
            .collapsed_blocks
            .contains(&StudioKey::local("acme-shop"))
    );

    // Esc steps back to Studios (>= 2 studios), then `n` opens NewStudio wizard.
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(app.focus.workspace, WorkspaceRegion::Lot(LotLevel::Studios));
    key(&mut app, hc_vt::Key::Char('n'));
    assert!(
        app.new_studio.is_some(),
        "`n` in Lot(Studios) opens NewStudio"
    );
}

#[tokio::test]
async fn lot_inside_studio_bounded_cursor_sections_chips_and_esc() {
    let mut app = grammar_app(true, true);
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    app.set_panel_runner(std::sync::Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok(()) })
    }));

    alt_l(&mut app);
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::InsideStudio("acme-shop".into()))
    );
    // Bounded cursor: Down past bottom Compose row never leaves acme-shop into beta-api.
    for _ in 0..12 {
        key(&mut app, hc_vt::Key::ArrowDown);
        assert_eq!(
            app.panel.selected.as_ref().and_then(Target::studio),
            Some(&StudioKey::local("acme-shop")),
            "cursor must stay bounded inside entered studio"
        );
    }

    // Tab cycles sections inside acme-shop (Header -> People -> Tasks -> Talk -> Input)
    // and then crosses to beta-api's Header, moving InsideStudio focus to beta-api.
    let mut seen_person = false;
    let mut seen_task = false;
    let mut seen_talk = false;
    let mut seen_compose = false;
    let mut crossed_beta = false;
    for _ in 0..8 {
        match &app.panel.selected {
            Some(Target::Person(k, ..)) if k.id == "acme-shop" => seen_person = true,
            Some(Target::Task(k, ..)) if k.id == "acme-shop" => seen_task = true,
            Some(Target::Talk(k, ..)) if k.id == "acme-shop" => seen_talk = true,
            Some(Target::Input(k)) if k.id == "acme-shop" => seen_compose = true,
            Some(Target::Studio(k)) if k.id == "beta-api" => {
                crossed_beta = true;
                assert_eq!(
                    app.focus.workspace,
                    WorkspaceRegion::Lot(LotLevel::InsideStudio("beta-api".into())),
                    "crossing to next studio via Tab must move InsideStudio focus into it"
                );
            }
            _ => {}
        }
        key(&mut app, hc_vt::Key::Tab);
    }
    assert!(seen_person && seen_task && seen_talk && seen_compose && crossed_beta);

    // On producer (open ask), Right enters RowActions cycling inline chips; Enter runs highlighted chip.
    app.panel.selected = Some(Target::Person("acme-shop".into(), "producer".into()));
    app.sync_focus();
    key(&mut app, hc_vt::Key::ArrowRight);
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::RowActions {
            studio: "acme-shop".into(),
            chip: 0,
        })
    );
    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::RowActions {
            studio: "acme-shop".into(),
            chip: 1,
        })
    );
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::InsideStudio("acme-shop".into()))
    );

    // Chip-less row (`Fold`): ArrowLeft steps up to Studios header.
    app.panel.selected = Some(Target::Fold("acme-shop".into()));
    key(&mut app, hc_vt::Key::ArrowLeft);
    assert_eq!(app.focus.workspace, WorkspaceRegion::Lot(LotLevel::Studios));
    assert_eq!(app.panel.selected, Some(Target::Studio("acme-shop".into())));

    // Single-studio Esc: skips Studios straight to Terminal.
    let mut one = grammar_app(false, true);
    alt_l(&mut one);
    assert!(matches!(
        one.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::InsideStudio(_))
    ));
    key(&mut one, hc_vt::Key::Escape);
    assert_eq!(one.focus.workspace, WorkspaceRegion::Terminal);
}

#[tokio::test]
async fn tree_fold_expand_enter_lot_and_new_studio() {
    let mut app = grammar_app(true, false);
    draw_to(&mut app, 160, 40);
    prefix_key(&mut app, 't');
    assert_eq!(app.focus.workspace, WorkspaceRegion::Tree);

    // Select local machine row; ArrowLeft collapses, ArrowRight expands.
    let endpoint = crate::chrome::snapshot::ClientEndpointId::Local;
    app.tree_focus = Some(crate::chrome::studio_tree::SidebarRow::Machine(
        endpoint.clone(),
    ));
    key(&mut app, hc_vt::Key::ArrowLeft);
    assert!(app.shell_state.collapsed_endpoints.contains(&endpoint));
    key(&mut app, hc_vt::Key::ArrowRight);
    assert!(!app.shell_state.collapsed_endpoints.contains(&endpoint));

    // Space toggles fold; `n` opens NewStudio wizard.
    key(&mut app, hc_vt::Key::Char(' '));
    assert!(app.shell_state.collapsed_endpoints.contains(&endpoint));
    key(&mut app, hc_vt::Key::Char('n'));
    assert!(app.new_studio.is_some(), "`n` in Tree opens NewStudio");
    key(&mut app, hc_vt::Key::Escape);

    // ArrowRight on a Tree studio row jumps into Lot entered on that studio.
    app.tree_focus = Some(crate::chrome::studio_tree::SidebarRow::Studio {
        endpoint,
        row: crate::chrome::studio_tree::Row::Studio {
            id: "beta-api".into(),
            kind: crate::chrome::studio_tree::StudioKind::StageDown,
        },
    });
    key(&mut app, hc_vt::Key::ArrowRight);
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::InsideStudio("beta-api".into()))
    );
}

#[tokio::test]
async fn cast_chips_tell_and_action_shortcuts() {
    let mut app = grammar_app(true, false);
    draw_to(&mut app, 160, 40);
    prefix_key(&mut app, 'a');
    assert_eq!(app.focus.workspace, WorkspaceRegion::Cast);

    // `t` on Cast role opens Lot compose targeted at that role.
    key(&mut app, hc_vt::Key::Char('t'));
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::Lot(LotLevel::Compose {
            studio: "acme-shop".into(),
            target: crate::focus::TellTarget::Role("producer".into()),
        })
    );
    key(&mut app, hc_vt::Key::Escape);
    key(&mut app, hc_vt::Key::Escape);
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(app.focus.workspace, WorkspaceRegion::Terminal);

    // Re-enter Cast on beta-api prompt role: Right/Left cycles chips, `y` allows prompt.
    app.focus_studio(&StudioKey::local("beta-api"));
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    app.set_panel_runner(std::sync::Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok(()) })
    }));
    prefix_key(&mut app, 'a');
    assert_eq!(app.focus.workspace, WorkspaceRegion::Cast);
    key(&mut app, hc_vt::Key::ArrowRight);
    assert!(
        app.cast_chip().is_some(),
        "Right activates Cast inline chip"
    );
    key(&mut app, hc_vt::Key::Char('y'));
    assert!(
        !recorded.lock().unwrap().is_empty(),
        "`y` in Cast answers user prompt"
    );
}

#[tokio::test]
async fn modal_grammar_fixes_across_sheet_update_wizard_dialogs_and_menu() {
    // 1. TaskSheet: BackTab cycles backward (Overview -> Records -> Deliveries).
    let mut app = grammar_app(true, true);
    app.sheet_open(StudioKey::local("acme-shop"), Some("t1".into()));
    assert_eq!(app.sheet.as_ref().unwrap().tab, SheetTab::Overview);
    key_mods(&mut app, hc_vt::Key::BackTab, hc_vt::Mods::SHIFT);
    assert_eq!(app.sheet.as_ref().unwrap().tab, SheetTab::Records);
    key_mods(&mut app, hc_vt::Key::BackTab, hc_vt::Mods::SHIFT);
    assert_eq!(app.sheet.as_ref().unwrap().tab, SheetTab::Deliveries);
    key(&mut app, hc_vt::Key::Escape);

    // 2. UpdateCard: PageUp scrolls without Ctrl, and Tab cycles buttons for Enter.
    let mut upd = app_with(&["t1"]);
    upd.update_latest = Some("9.9.9".into());
    upd.update_state.releases = vec![CachedRelease {
        version: "9.9.9".into(),
        date: "2026-11-20".into(),
        body: (0..30)
            .map(|i| format!("- Item {i}"))
            .collect::<Vec<_>>()
            .join("\n"),
    }];
    upd.open_update();
    draw_to(&mut upd, 80, 24);
    key(&mut upd, hc_vt::Key::Char('G'));
    let scrolled = upd.update_modal.as_ref().unwrap().scroll;
    assert!(scrolled > 0);
    key(&mut upd, hc_vt::Key::PageUp);
    assert!(
        upd.update_modal.as_ref().unwrap().scroll < scrolled,
        "plain PageUp must scroll UpdateCard upward"
    );
    key(&mut upd, hc_vt::Key::Tab);
    assert!(upd.update_modal.as_ref().unwrap().focused_button.is_some());

    // 3. NewStudio Step 2: BackTab and ArrowUp step fields backward (Id -> Build -> Mode -> Title).
    let mut wiz = grammar_app(false, false);
    wiz.new_studio_open(None);
    wiz.new_studio.as_mut().unwrap().step = Step::IdTitle;
    assert_eq!(wiz.new_studio.as_ref().unwrap().field, Step2Field::Id);
    key_mods(&mut wiz, hc_vt::Key::BackTab, hc_vt::Mods::SHIFT);
    assert_eq!(
        wiz.new_studio.as_ref().unwrap().field,
        Step2Field::Build,
        "BackTab in Step 2 must go backward"
    );
    key(&mut wiz, hc_vt::Key::ArrowUp);
    assert_eq!(
        wiz.new_studio.as_ref().unwrap().field,
        Step2Field::Mode,
        "ArrowUp in Step 2 must go backward"
    );

    // 4. RecastDialog & RestoreDialog: Esc steps back 1 step before closing.
    let mut rec = RecastDialog::new(
        Some(&StudioKey::local("acme-shop")),
        "producer",
        "producer",
        None,
        None,
        None,
    );
    rec.runners = Probe::Done(vec![RunnerRow {
        runner: "claude".into(),
        status: "found".into(),
        version: Some("1.0".into()),
        path: None,
        needs_path: false,
        models: vec!["sonnet".into()],
        efforts: vec!["low".into(), "high".into()],
    }]);
    rec.step = RecastStep::Apply;
    wiz.studio_dialog = Some(StudioDialog::Recast(rec));
    key(&mut wiz, hc_vt::Key::Escape);
    assert!(matches!(
        wiz.studio_dialog,
        Some(StudioDialog::Recast(ref d)) if d.step == RecastStep::Effort
    ));
    key(&mut wiz, hc_vt::Key::Escape);
    assert!(matches!(
        wiz.studio_dialog,
        Some(StudioDialog::Recast(ref d)) if d.step == RecastStep::Model
    ));
    key(&mut wiz, hc_vt::Key::Escape);
    assert!(matches!(
        wiz.studio_dialog,
        Some(StudioDialog::Recast(ref d)) if d.step == RecastStep::Runner
    ));
    key(&mut wiz, hc_vt::Key::Escape);
    assert!(wiz.studio_dialog.is_none());

    let mut res = RestoreDialog::new(None, Vec::new());
    res.step = RestoreStep::Confirm;
    wiz.studio_dialog = Some(StudioDialog::Restore(res));
    key(&mut wiz, hc_vt::Key::Escape);
    assert!(matches!(
        wiz.studio_dialog,
        Some(StudioDialog::Restore(ref d)) if d.step == RestoreStep::Name
    ));
    key(&mut wiz, hc_vt::Key::Escape);
    assert!(matches!(
        wiz.studio_dialog,
        Some(StudioDialog::Restore(ref d)) if d.step == RestoreStep::List
    ));
    key(&mut wiz, hc_vt::Key::Escape);
    assert!(wiz.studio_dialog.is_none());

    // 5. ContextMenu: j/k move, Tab wraps, Left closes.
    let mut menu_app = grammar_app(true, false);
    prefix_key(&mut menu_app, 'y');
    key(&mut menu_app, hc_vt::Key::Char('m'));
    assert!(menu_app.menu.is_some());
    let start_hi = menu_app.menu.as_ref().unwrap().highlighted;
    key(&mut menu_app, hc_vt::Key::Char('j'));
    assert_eq!(menu_app.menu.as_ref().unwrap().highlighted, start_hi + 1);
    key(&mut menu_app, hc_vt::Key::Char('k'));
    assert_eq!(menu_app.menu.as_ref().unwrap().highlighted, start_hi);
    key(&mut menu_app, hc_vt::Key::ArrowLeft);
    assert!(menu_app.menu.is_none(), "ArrowLeft closes ContextMenu");
}

#[tokio::test]
async fn call_sheet_zone_syncs_with_waiting_hollywood_studios_tab_ring_and_opening_rule() {
    let cfg = Ok(Some(backlot_stage::config::HollywoodCfg {
        runner: "fake".into(),
        model: "m".into(),
        effort: Some("high".into()),
        key: None,
        default_cast: None,
    }));

    // 1. With a blocking wait (`!B > 0`), `Alt+Y` opens into `CallSheetZone::Waiting`.
    let mut app = grammar_app(true, true);
    app.set_hollywood(cfg.clone(), std::path::Path::new("/tmp/focus-grammar.toml"));
    draw_to(&mut app, 140, 36);
    key_mods(&mut app, hc_vt::Key::Char('y'), hc_vt::Mods::ALT);
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::CallSheet(CallSheetZone::Waiting)
    );
    assert!(!app.hollywood.editing);
    let backend = draw_to(&mut app, 140, 36);
    let buf = backend.buffer();
    let mode_row: String = (0..140).map(|x| buf[(x, 35)].symbol()).collect();
    assert!(
        mode_row.contains("FOCUS: call sheet › waiting"),
        "modebar should show Waiting breadcrumb: {mode_row:?}"
    );

    // 2. `Tab` cycles `Waiting -> Hollywood (compose) -> Studios -> Waiting`, `Shift+Tab` reverses.
    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::CallSheet(CallSheetZone::Hollywood)
    );
    assert!(app.hollywood.editing);
    let backend = draw_to(&mut app, 140, 36);
    let mode_row: String = (0..140)
        .map(|x| backend.buffer()[(x, 35)].symbol())
        .collect();
    assert!(
        mode_row.contains("FOCUS: call sheet › hollywood"),
        "modebar should show Hollywood breadcrumb: {mode_row:?}"
    );

    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::CallSheet(CallSheetZone::Studios)
    );
    assert!(!app.hollywood.editing);
    let backend = draw_to(&mut app, 140, 36);
    let mode_row: String = (0..140)
        .map(|x| backend.buffer()[(x, 35)].symbol())
        .collect();
    assert!(
        mode_row.contains("FOCUS: call sheet › studios"),
        "modebar should show Studios breadcrumb: {mode_row:?}"
    );

    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::CallSheet(CallSheetZone::Waiting)
    );
    key_mods(&mut app, hc_vt::Key::BackTab, hc_vt::Mods::SHIFT);
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::CallSheet(CallSheetZone::Studios)
    );
    key_mods(&mut app, hc_vt::Key::BackTab, hc_vt::Mods::SHIFT);
    assert_eq!(
        app.focus.workspace,
        WorkspaceRegion::CallSheet(CallSheetZone::Hollywood)
    );

    // 3. Without blocking items (`!B == 0`), `Alt+Y` opens into `CallSheetZone::Hollywood` composing.
    let mut calm_block = grammar_app(false, true);
    calm_block.set_hollywood(cfg, std::path::Path::new("/tmp/focus-grammar.toml"));
    draw_to(&mut calm_block, 140, 36);
    key_mods(&mut calm_block, hc_vt::Key::Char('y'), hc_vt::Mods::ALT);
    assert_eq!(
        calm_block.focus.workspace,
        WorkspaceRegion::CallSheet(CallSheetZone::Hollywood)
    );
    assert!(calm_block.hollywood.editing);
}
