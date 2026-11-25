//! The Task & Delivery Sheet's tests (S2 §3): `v` opens it on the
//! dock's selection with the summary instant and the ledger reads
//! out, `Tab`/`1`–`3` switch its tabs, `⏎` streams a delivery's
//! report through G3 — locally or machine-qualified — `Esc` backs
//! out of the report first, and a late reply lands dead.
use super::*;
use crate::app::panel::sheet::SheetReply;
use crate::sheet::{SheetTab, lines_for};
use crate::studio::StudioKey;

/// The recorded `(machine, argv)` call list a test shares with its
/// injected runner.
type Recorded = std::sync::Arc<std::sync::Mutex<Vec<(Option<String>, Vec<String>)>>>;

/// A `(machine, argv)` recorder the sheet's fetches run through —
/// the same shape `new_studio`'s `run_on` records.
fn recorded_run(
    sink: Recorded,
    answers: Vec<(String, Result<String, String>)>,
) -> crate::app::new_studio::NsRun {
    std::sync::Arc::new(move |machine, argv, _stdin| {
        sink.lock().unwrap().push((machine, argv.clone()));
        let joined = argv.join(" ");
        let out = answers
            .iter()
            .find(|(k, _)| joined.starts_with(k.as_str()))
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| Ok(String::new()));
        Box::pin(async move { out })
    })
}

/// Land every queued sheet reply, like the runtime's drain.
async fn drain_sheet(app: &mut App) {
    for _ in 0..20 {
        tokio::task::yield_now().await;
        while let Ok(reply) = app.sheet_rx.try_recv() {
            app.on_sheet_reply(reply);
        }
    }
}

#[tokio::test]
async fn v_opens_the_sheet_on_the_selected_studio_with_the_reads_out() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    let calls: Recorded = Default::default();
    app.set_ns_run(recorded_run(calls.clone(), vec![]));
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Char('v'));
    // Let the spawned fetch tasks record their argv.
    tokio::task::yield_now().await;
    let sheet = app.sheet.as_ref().expect("sheet opened");
    assert_eq!(sheet.studio.id, "acme-shop");
    // The instant state: the modal paints with `loading ledger
    // details…` until the two reads land.
    let text = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(text.contains("1 Overview & Casts"), "{text}");
    assert!(text.contains("loading ledger details"), "{text}");
    // `status --json` and `records --json` on the local studio.
    let argv: Vec<String> = calls
        .lock()
        .unwrap()
        .iter()
        .map(|(_, a)| a.join(" "))
        .collect();
    assert!(
        argv.iter().any(|a| a == "status --studio acme-shop --json"),
        "{argv:?}"
    );
    assert!(
        argv.iter()
            .any(|a| a == "records --studio acme-shop --json"),
        "{argv:?}"
    );
    assert!(
        calls.lock().unwrap().iter().all(|(m, _)| m.is_none()),
        "a local studio reads locally: {argv:?}"
    );
}

#[tokio::test]
async fn tab_and_digits_switch_the_sheets_tabs() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    app.set_ns_run(recorded_run(Default::default(), vec![]));
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Char('v'));
    assert_eq!(app.sheet.as_ref().unwrap().tab, SheetTab::Overview);
    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(app.sheet.as_ref().unwrap().tab, SheetTab::Deliveries);
    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(app.sheet.as_ref().unwrap().tab, SheetTab::Records);
    key(&mut app, hc_vt::Key::Tab);
    assert_eq!(app.sheet.as_ref().unwrap().tab, SheetTab::Overview);
    key(&mut app, hc_vt::Key::Char('3'));
    assert_eq!(app.sheet.as_ref().unwrap().tab, SheetTab::Records);
    key(&mut app, hc_vt::Key::Char('2'));
    assert_eq!(app.sheet.as_ref().unwrap().tab, SheetTab::Deliveries);
}

#[tokio::test]
async fn a_reply_for_a_closed_or_reopened_sheet_lands_dead() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    app.set_ns_run(recorded_run(Default::default(), vec![]));
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Char('v'));
    let epoch = app.sheet.as_ref().unwrap().epoch;
    // A late status reply tagged for this open lands.
    app.on_sheet_reply(SheetReply::Status {
        epoch,
        out: Ok("{}".into()),
    });
    assert!(app.sheet.as_ref().unwrap().status.is_some());
    // Reopen: the epoch bumps, so a reply tagged to the old open is
    // dropped — the sheet still waits on its own reads.
    app.sheet_open(crate::studio::StudioKey::local("acme-shop"), None);
    let new_epoch = app.sheet.as_ref().unwrap().epoch;
    assert!(new_epoch > epoch);
    app.on_sheet_reply(SheetReply::Records {
        epoch,
        out: Ok("[]".into()),
    });
    assert!(app.sheet.as_ref().unwrap().records.is_none());
    // And after a close, a reply has no sheet to land on.
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.sheet.is_none());
    app.on_sheet_reply(SheetReply::Status {
        epoch: new_epoch,
        out: Ok("{}".into()),
    });
    assert!(app.sheet.is_none());
}

#[tokio::test]
async fn the_deliveries_tab_lists_every_delivery_and_enter_reads_its_report() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    let calls: Recorded = Default::default();
    app.set_ns_run(recorded_run(
        calls.clone(),
        vec![(
            "report show --studio acme-shop --file reports/d1.md".to_string(),
            Ok("report body\nsecond line".to_string()),
        )],
    ));
    open_panel(&mut app);
    app.sheet_open(
        crate::studio::StudioKey::local("acme-shop"),
        Some("task-1".into()),
    );
    // A status answer carrying one delivery lands.
    let status = serde_json::json!({
        "schema": "studio/1",
        "id": "acme-shop",
        "requirement_version": 1, "last_seq": 3,
        "tasks": {
            "task-1": {
                "id": "task-1", "title": "t", "status": "delivered",
                "task_kind": "task",
                "requirement_version": 1, "priority": "normal",
                "acceptance": "it works", "rework_rounds": 0,
                "dependencies": [],
                "created_seq": 1, "created_ts_ms": 1, "last_ts_ms": 1,
                "deliveries": [{
                    "n": 1,
                    "actor": {"runner": "agent-a", "model": "m",
                              "permission_profile": "standard"},
                    "commits": "a1..b2", "requirement_version": 1,
                    "report": "reports/d1.md", "checks": [],
                    "seq": 2, "ts_ms": 2
                }],
                "reviews": []
            }
        }
    });
    app.on_sheet_reply(SheetReply::Status {
        epoch: app.sheet.as_ref().unwrap().epoch,
        out: Ok(status.to_string()),
    });
    key(&mut app, hc_vt::Key::Char('2'));
    let text = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(text.contains("d1 agent-a/m · a1..b2"), "{text}");
    // `⏎` on the delivery row: G3 on the local studio — `o` is
    // reopen-only and no longer belongs to the sheet (R3-M11).
    let sheet = app.sheet.as_mut().unwrap();
    sheet.sel = sheet.sel.max(
        lines_for(sheet, None)
            .iter()
            .position(|l| l.file.is_some())
            .unwrap_or(0),
    );
    key(&mut app, hc_vt::Key::Enter);
    drain_sheet(&mut app).await;
    let argv: Vec<String> = calls
        .lock()
        .unwrap()
        .iter()
        .map(|(_, a)| a.join(" "))
        .collect();
    assert!(
        argv.iter()
            .any(|a| a == "report show --studio acme-shop --file reports/d1.md"),
        "{argv:?}"
    );
    let view = app.sheet.as_ref().and_then(|s| s.view.as_ref());
    assert_eq!(view.map(|v| v.title.as_str()), Some("reports/d1.md"));
    // Esc retires the report back to the tab, then closes the sheet.
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.sheet.as_ref().is_some_and(|s| s.view.is_none()));
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.sheet.is_none());
}

#[tokio::test]
async fn a_remote_studio_reads_and_reports_on_its_machine() {
    let mut app = app_with_two_machines();
    app.area = (120, 24);
    let calls: Recorded = Default::default();
    app.set_ns_run(recorded_run(calls.clone(), vec![]));
    open_panel(&mut app);
    // The remote studio's sheet: both reads carry `host-b`.
    app.sheet_open(
        StudioKey {
            machine: Some("host-b".into()),
            id: "beta-tools".into(),
        },
        Some("t1".into()),
    );
    tokio::task::yield_now().await;
    let seen: Vec<(Option<String>, String)> = calls
        .lock()
        .unwrap()
        .iter()
        .map(|(m, a)| (m.clone(), a.join(" ")))
        .collect();
    assert!(
        seen.iter().all(|(m, _)| m.as_deref() == Some("host-b")),
        "{seen:?}"
    );
    assert!(
        seen.iter()
            .any(|(_, a)| a == "status --studio beta-tools --json"),
        "{seen:?}"
    );
    assert!(
        seen.iter()
            .any(|(_, a)| a == "records --studio beta-tools --json"),
        "{seen:?}"
    );
    // An `⏎` read on a check log runs the same G3 remotely.
    let status = serde_json::json!({
        "schema": "studio/1", "id": "beta-tools", "requirement_version": 1,
        "last_seq": 3,
        "tasks": { "t1": {
            "id": "t1", "title": "t", "status": "delivered",
            "task_kind": "task",
            "requirement_version": 1, "priority": "normal",
            "acceptance": "it works", "rework_rounds": 0,
            "dependencies": [],
            "created_seq": 1, "created_ts_ms": 1, "last_ts_ms": 1,
            "deliveries": [{
                "n": 1,
                "actor": {"runner": "a", "model": "m",
                          "permission_profile": "standard"},
                "commits": "c1..c2", "requirement_version": 1,
                "report": "reports/d1.md", "checks": [],
                "seq": 2, "ts_ms": 2
            }],
            "checked": [{
                "delivery": 1, "name": "units", "passed": true,
                "exit_code": 0, "duration_ms": 5,
                "output": "checks/d1-units.txt", "seq": 3, "ts_ms": 3
            }],
            "reviews": []
        }}
    });
    app.on_sheet_reply(SheetReply::Status {
        epoch: app.sheet.as_ref().unwrap().epoch,
        out: Ok(status.to_string()),
    });
    key(&mut app, hc_vt::Key::Char('2'));
    {
        let sheet = app.sheet.as_mut().unwrap();
        let lines = lines_for(sheet, None);
        sheet.sel = lines
            .iter()
            .position(|l| l.file.as_deref() == Some("checks/d1-units.txt"))
            .expect("a check line carries its log file");
    }
    key(&mut app, hc_vt::Key::Enter);
    tokio::task::yield_now().await;
    let seen: Vec<(Option<String>, String)> = calls
        .lock()
        .unwrap()
        .iter()
        .map(|(m, a)| (m.clone(), a.join(" ")))
        .collect();
    assert!(
        seen.iter().any(|(m, a)| m.as_deref() == Some("host-b")
            && a == "report show --studio beta-tools --file checks/d1-units.txt"),
        "{seen:?}"
    );
}

#[tokio::test]
async fn an_open_report_leaves_no_tab_text_inside_the_body() {
    // S2 round-1 review: `⏎` on a populated Deliveries tab must not
    // leave the tab's rows showing under the report.
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    let calls: Recorded = Default::default();
    app.set_ns_run(recorded_run(
        calls.clone(),
        vec![(
            "report show --studio acme-shop --file reports/d1.md".to_string(),
            Ok("short body".to_string()),
        )],
    ));
    open_panel(&mut app);
    app.sheet_open(
        crate::studio::StudioKey::local("acme-shop"),
        Some("task-1".into()),
    );
    let status = serde_json::json!({
        "schema": "studio/1",
        "id": "acme-shop",
        "requirement_version": 1, "last_seq": 3,
        "tasks": {
            "task-1": {
                "id": "task-1", "title": "t", "status": "delivered",
                "task_kind": "task",
                "requirement_version": 1, "priority": "normal",
                "acceptance": "it works", "rework_rounds": 0,
                "dependencies": [],
                "created_seq": 1, "created_ts_ms": 1, "last_ts_ms": 1,
                "deliveries": [{
                    "n": 1,
                    "actor": {"runner": "agent-a", "model": "m",
                              "permission_profile": "standard"},
                    "commits": "5ccee8581111222233334444555566667777aa01..4121d81d9999aaaabbbbccccddddeeeeffff0000",
                    "requirement_version": 1,
                    "report": "reports/d1.md", "checks": [],
                    "seq": 2, "ts_ms": 2
                }],
                "reviews": []
            }
        }
    });
    app.on_sheet_reply(SheetReply::Status {
        epoch: app.sheet.as_ref().unwrap().epoch,
        out: Ok(status.to_string()),
    });
    key(&mut app, hc_vt::Key::Char('2'));
    // Short SHAs on the delivery row — the full range never fits.
    {
        let sheet = app.sheet.as_mut().unwrap();
        let lines = lines_for(sheet, None);
        let d1 = lines.iter().find(|l| l.text.contains("d1 ")).unwrap();
        assert!(
            d1.text.contains("5ccee858..4121d81d"),
            "short range: {}",
            d1.text
        );
        sheet.sel = lines.iter().position(|l| l.file.is_some()).unwrap_or(0);
    }
    key(&mut app, hc_vt::Key::Enter);
    drain_sheet(&mut app).await;
    let buf = draw_to(&mut app, 120, 36);
    let body = app.sheet_hits.body;
    let body_text: String = (body.y..body.y + body.height)
        .map(|y| {
            (body.x..body.x + body.width)
                .map(|x| buf.buffer()[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !body_text.contains("agent-a"),
        "tab text under the report: {body_text}"
    );
    let reader = app
        .hollywood_hits
        .view
        .as_ref()
        .expect("unified Reader rendered")
        .area;
    let reader_text: String = (reader.y..reader.y + reader.height)
        .map(|y| {
            (reader.x..reader.x + reader.width)
                .map(|x| buf.buffer()[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(reader_text.contains("short body"), "{reader_text}");
}

/// R14: an experimental studio's overview head carries `exp` —
/// a standard studio's head line is what it always was.
#[tokio::test]
async fn an_experimental_studio_flags_the_sheet_head() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    let mut value = summary_json();
    value["studios"][0]["mode"] = serde_json::json!("experimental");
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    open_panel(&mut app);
    app.sheet_open(StudioKey::local("acme-shop"), None);
    let text = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(text.contains("acme-shop — live exp"), "{text}");
}

#[tokio::test]
async fn the_pane_cursor_hides_while_the_sheet_is_open() {
    // S2 round-1 review: the focused pane's hardware cursor must not
    // print through the sheet — the same rule the other modals get.
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    assert!(!app.hide_pane_cursor(120));
    app.sheet_open(crate::studio::StudioKey::local("acme-shop"), None);
    assert!(app.hide_pane_cursor(120));
}

#[tokio::test]
async fn the_sheet_draws_inside_the_modal_band_at_every_width() {
    let mut app = app_with_panel_doc();
    app.area = (80, 24);
    app.set_ns_run(recorded_run(Default::default(), vec![]));
    open_panel(&mut app);
    app.sheet_open(crate::studio::StudioKey::local("acme-shop"), None);
    for cols in [160u16, 120, 80, 60, 40] {
        app.area = (cols, 24);
        let buf = draw_to(&mut app, cols, 24);
        let text = screen_lines(&buf).join("\n");
        assert!(text.contains("1 Overview & Casts"), "cols {cols}: {text}");
    }
    // At 80 an overlay dock reserves zero beside columns, so the modal
    // band spans the full 80 columns and the sheet keeps its 76-col width.
    app.area = (80, 24);
    draw_to(&mut app, 80, 24);
    let band = app.modal_band(ratatui::layout::Rect::new(0, 0, 80, 24));
    assert_eq!(band.width, 80);
    assert_eq!(
        (app.sheet_hits.frame.width, app.sheet_hits.frame.height),
        (76, 20)
    );
    assert!(app.sheet_hits.frame.x >= band.x);
    assert!(app.sheet_hits.frame.x + app.sheet_hits.frame.width <= band.x + band.width);
}

#[tokio::test]
async fn stepping_walks_sheet_deliverable_and_check_files_syncing_sel() {
    let mut app = app_with_panel_doc();
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = crate::app::pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
    app.area = (120, 36);
    let status = serde_json::json!({
        "schema": "studio/1",
        "id": "acme-shop",
        "requirement_version": 1,
        "last_seq": 3,
        "tasks": {
            "task-1": {
                "id": "task-1",
                "title": "t",
                "status": "delivered",
                "task_kind": "task",
                "requirement_version": 1,
                "priority": "normal",
                "acceptance": "it works",
                "rework_rounds": 0,
                "dependencies": [],
                "created_seq": 1,
                "created_ts_ms": 1,
                "last_ts_ms": 1,
                "deliveries": [{
                    "n": 1,
                    "actor": {"runner": "agent-a", "model": "m", "permission_profile": "standard"},
                    "commits": "a1..b2",
                    "requirement_version": 1,
                    "report": "reports/d1.md",
                    "checks": [],
                    "seq": 2,
                    "ts_ms": 2
                }],
                "checked": [{
                    "delivery": 1,
                    "name": "units",
                    "passed": false,
                    "exit_code": 1,
                    "duration_ms": 12,
                    "output": "checks/d1-units.txt",
                    "seq": 3,
                    "ts_ms": 3
                }],
                "reviews": []
            }
        }
    });
    let calls: Recorded = Default::default();
    app.set_ns_run(recorded_run(
        calls.clone(),
        vec![
            (
                "status --studio acme-shop --json".to_string(),
                Ok(status.to_string()),
            ),
            (
                "report show --studio acme-shop --file reports/d1.md".to_string(),
                Ok("delivery report body".to_string()),
            ),
            (
                "report show --studio acme-shop --file checks/d1-units.txt".to_string(),
                Ok("error: test failed\nexit status: 1".to_string()),
            ),
        ],
    ));
    open_panel(&mut app);
    app.sheet_open(StudioKey::local("acme-shop"), Some("task-1".into()));
    drain_sheet(&mut app).await;
    key(&mut app, hc_vt::Key::Char('2'));

    let (del_idx, check_idx) = {
        let sheet = app.sheet.as_mut().unwrap();
        let lines = lines_for(sheet, None);
        let d = lines
            .iter()
            .position(|l| l.file.as_deref() == Some("reports/d1.md"))
            .unwrap();
        let c = lines
            .iter()
            .position(|l| l.file.as_deref() == Some("checks/d1-units.txt"))
            .unwrap();
        sheet.sel = d;
        (d, c)
    };
    key(&mut app, hc_vt::Key::Enter);
    drain_sheet(&mut app).await;
    let dtext = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(dtext.contains("■ DELIVERY · acme-shop"), "{dtext}");
    assert!(dtext.contains("◀ 1/2 ▶"), "{dtext}");
    assert!(dtext.contains("delivery report body"), "{dtext}");

    // Step forward with `→` onto the check log (`checks/d1-units.txt`).
    key(&mut app, hc_vt::Key::ArrowRight);
    assert_eq!(app.sheet.as_ref().unwrap().sel, check_idx);
    drain_sheet(&mut app).await;
    let ctext = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(ctext.contains("■ CHECK · acme-shop"), "{ctext}");
    assert!(ctext.contains("◀ 2/2 ▶"), "{ctext}");
    assert!(ctext.contains("checks/d1-units.txt"), "{ctext}");
    assert!(ctext.contains("error: test failed"), "{ctext}");

    // `[` steps back to the delivery line.
    key(&mut app, hc_vt::Key::Char('['));
    assert_eq!(app.sheet.as_ref().unwrap().sel, del_idx);
    drain_sheet(&mut app).await;

    // `]` moves back onto check line, and `Esc` closes only Reader keeping Sheet open at check_idx.
    key(&mut app, hc_vt::Key::Char(']'));
    drain_sheet(&mut app).await;
    key(&mut app, hc_vt::Key::Escape);
    let sheet = app.sheet.as_ref().expect("Sheet remains open");
    assert!(sheet.view.is_none());
    assert_eq!(sheet.sel, check_idx);
}
