//! Slice 4 fail-first & regression tests: `OutwardCardSummary` in the Lot
//! Call Sheet and Studio Dock with/without `[hollywood]`, local deduping
//! against `HollywoodSummary.cards`, `a`/`y`/`x`/`d`/`s`/`Enter`, and remote
//! execution through `RemoteRunner`.

use std::path::Path;
use std::sync::{Arc, Mutex};

use backlot_stage::config::HollywoodCfg;
use hc_proto::watch::{DocKey, Document};
use tokio::sync::mpsc;

use super::*;
use crate::app::input::UiEvent;
use crate::hollywood::{LayerMode, status_of, waiting_items};
use crate::panel::PanelMode;

fn press(app: &mut App, k: hc_vt::Key) {
    app.on_input(
        UiEvent::Key(term_key(k, hc_vt::Mods::NONE, hc_vt::Action::Press)),
        app.area.0,
    );
}

fn press_mods(app: &mut App, k: hc_vt::Key, mods: hc_vt::Mods) {
    app.on_input(
        UiEvent::Key(term_key(k, mods, hc_vt::Action::Press)),
        app.area.0,
    );
}

type RecordedNs = Arc<Mutex<Vec<(Option<String>, Vec<String>)>>>;

fn ns_recorder() -> (RecordedNs, crate::app::new_studio::NsRun) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let sink = calls.clone();
    let runner: crate::app::new_studio::NsRun = Arc::new(move |machine, argv, _stdin| {
        sink.lock().unwrap().push((machine, argv.clone()));
        let is_show = argv.windows(2).any(|w| w == ["card", "show"]);
        Box::pin(async move {
            if is_show {
                Ok("card c2: open push\ncommand: git push origin HEAD:main".into())
            } else {
                Ok(String::new())
            }
        })
    });
    (calls, runner)
}

fn outward_card_json(id: &str, cmd: &str, rule: &str, why: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "role": "actor-alice",
        "command": cmd,
        "rule": rule,
        "why": why,
        "repo": "/home/user/repo",
        "remote_url": "git@host-a:acme-shop/app.git",
        "commits": ["abc1234 feat: ship outward ui"],
        "commits_total": 1
    })
}

fn local_summary_json(with_hollywood: bool) -> serde_json::Value {
    let hw = if with_hollywood {
        serde_json::json!({
            "state": "live",
            "runner": "fake",
            "model": "model-h",
            "cards": [
                {
                    "id": "c1",
                    "studio": "acme-shop",
                    "action": "outward: git push origin HEAD:main",
                    "why": "release_main"
                },
                {
                    "id": "c2",
                    "studio": "acme-shop",
                    "action": "pause t1",
                    "why": "looping"
                }
            ]
        })
    } else {
        serde_json::Value::Null
    };
    serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 1,
        "studios": [{
            "id": "acme-shop",
            "title": "Acme shop",
            "state": "live",
            "requirement_version": 1,
            "mode": "standard",
            "open_reviews": 0,
            "waiting": 0,
            "roles": [
                {
                    "role": "producer",
                    "name": "producer",
                    "terminal": "t1",
                    "state": "live",
                    "work": "working"
                }
            ],
            "outward_cards": [
                outward_card_json("c1", "git push origin HEAD:main", "protected branch 'main'", "release_main")
            ]
        }],
        "hollywood": hw
    })
}

fn setup_local_app(with_hollywood: bool) -> App {
    let mut app = app_with(&["t1"]);
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
    app.set_plain(false);
    let mut stage = crate::sidebar::fixture::term("t9");
    stage.name = Some("backlot-stage".into());
    stage.state = "live".into();
    let mut terms: Vec<_> = app.view.inventory.terminals().values().cloned().collect();
    terms.push(stage);
    app.view.inventory_mut().replace(2, terms);
    app.view.documents_mut().insert(
        DocKey {
            machine: None,
            name: "backlot".into(),
        },
        Document {
            value: local_summary_json(with_hollywood),
            ..Document::default()
        }
        .into(),
    );
    app.focused = Some("t1".into());
    app.sync_target();
    app.reconcile_published_view();
    let cfg = with_hollywood.then(|| HollywoodCfg {
        runner: "fake".into(),
        model: "model-h".into(),
        effort: None,
        key: None,
        default_cast: None,
    });
    app.set_hollywood(Ok(cfg), Path::new("config.toml"));
    app.area = (140, 32);
    app
}

fn setup_two_machine_app() -> App {
    let mut app = setup_local_app(false);
    app.view.machines = Some(vec![hc_proto::machine::MachineView {
        name: "host-b".into(),
        target: "host-b".into(),
        id: "id-host-b".into(),
        host: "host-b".into(),
        hostname: None,
        version: "0".into(),
        state: "connected".into(),
        session: None,
        since_ms: None,
        temporary: false,
        via: false,
        why: None,
    }]);
    let mut r_stage = crate::sidebar::fixture::term("host-b/t9");
    r_stage.name = Some("backlot-stage".into());
    r_stage.state = "live".into();
    let r_prod = crate::sidebar::fixture::term("host-b/t1");
    let mut terms: Vec<_> = app.view.inventory.terminals().values().cloned().collect();
    terms.push(r_stage);
    terms.push(r_prod);
    app.view.inventory_mut().replace(3, terms);
    app.view.documents_mut().insert(
        DocKey {
            machine: Some("host-b".into()),
            name: "backlot".into(),
        },
        Document {
            value: serde_json::json!({
                "schema": "backlot/1",
                "updated_ms": 2,
                "studios": [{
                    "id": "beta-tools",
                    "title": "Beta tools",
                    "state": "live",
                    "requirement_version": 1,
                    "mode": "standard",
                    "open_reviews": 0,
                    "waiting": 0,
                    "roles": [
                        {
                            "role": "producer",
                            "name": "producer",
                            "terminal": "t1",
                            "state": "live",
                            "work": "waiting"
                        }
                    ],
                    "outward_cards": [
                        outward_card_json("c2", "gh pr create --title Ship", "gh pr create", "open_pr")
                    ]
                }]
            }),
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    app
}

#[tokio::test]
async fn outward_cards_surface_without_hollywood_and_dedupe_with_hollywood() {
    let mut app = setup_local_app(false);
    assert_eq!(app.studio.lot.counts.urgent, 1, "lot pill urgent count");

    // 1. Call Sheet renders outward card even when `[hollywood]` is absent.
    if app.hollywood.mode != LayerMode::Expanded {
        press_mods(&mut app, hc_vt::Key::Char('y'), hc_vt::Mods::ALT);
    }
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    let lot_text = screen_lines(&draw_to(&mut app, 140, 32)).join("\n");
    assert!(
        lot_text.contains("c1") && lot_text.contains("git push origin HEAD:main"),
        "Call Sheet missing outward card without [hollywood]:\n{lot_text}"
    );

    // 2. Studio Dock renders outward card in the studio's expanded block.
    app.hollywood.mode = LayerMode::Hidden;
    press_mods(&mut app, hc_vt::Key::Char('b'), hc_vt::Mods::CTRL);
    press(&mut app, hc_vt::Key::Char('y'));
    assert_eq!(app.panel.mode, PanelMode::Expanded);
    let dock_text = screen_lines(&draw_to(&mut app, 140, 32)).join("\n");
    assert!(
        dock_text.contains("c1") && dock_text.contains("git push origin HEAD:main"),
        "Studio Dock missing outward card:\n{dock_text}"
    );
    println!(
        "CAPTURED_LOT_ROW_BEGIN\n{lot_text}\nCAPTURED_DOCK_BEGIN\n{dock_text}\nCAPTURED_UI_END"
    );

    // 3. When `[hollywood]` is present, local outward card `c1` dedupes against `hollywood.cards`.
    let app_hw = setup_local_app(true);
    let machines = app_hw.panel_summaries();
    let status = status_of(&app_hw.view);
    let waits = waiting_items(&machines, &status);
    assert_eq!(waits.len(), 2, "expected c1 (deduped) + c2");
}

#[tokio::test]
async fn call_sheet_and_dock_outward_card_keys_route_local_and_remote() {
    let mut app = setup_two_machine_app();
    let (calls, runner) = ns_recorder();
    app.set_ns_run(runner);

    // 1. Call Sheet on remote outward card `c2`: `a`/`y` apply, `x`/`d` cancel, `s`/Enter show.
    if app.hollywood.mode != LayerMode::Expanded {
        press_mods(&mut app, hc_vt::Key::Char('y'), hc_vt::Mods::ALT);
    }
    let remote_lot_target = app
        .hollywood_targets()
        .into_iter()
        .find(|t| format!("{t:?}").contains("\"c2\""))
        .expect("Call Sheet missing remote outward card target for c2");
    app.hollywood_select(remote_lot_target);

    press(&mut app, hc_vt::Key::Char('a'));
    let reply = app.action_rx.recv().await.expect("apply reply");
    app.on_action_reply(reply);
    press(&mut app, hc_vt::Key::Char('y'));
    let reply = app.action_rx.recv().await.expect("y apply reply");
    app.on_action_reply(reply);
    press(&mut app, hc_vt::Key::Char('x'));
    assert!(app.studio_dialog.is_some(), "x opens cancel reason dialog");
    press(&mut app, hc_vt::Key::Enter);
    let reply = app.action_rx.recv().await.expect("x cancel reply");
    app.on_action_reply(reply);
    press(&mut app, hc_vt::Key::Char('d'));
    assert!(app.studio_dialog.is_some(), "d opens cancel reason dialog");
    press(&mut app, hc_vt::Key::Enter);
    let reply = app.action_rx.recv().await.expect("d cancel reply");
    app.on_action_reply(reply);
    press(&mut app, hc_vt::Key::Char('s'));
    let reply = app.action_rx.recv().await.expect("show reply");
    app.on_action_reply(reply);
    assert!(app.hollywood.view.is_some(), "s opens CardView");
    press(&mut app, hc_vt::Key::Escape);
    press(&mut app, hc_vt::Key::Enter);
    let reply = app.action_rx.recv().await.expect("enter show reply");
    app.on_action_reply(reply);
    assert!(app.hollywood.view.is_some(), "Enter opens CardView");
    press(&mut app, hc_vt::Key::Escape);

    {
        let got = calls.lock().unwrap();
        assert_eq!(
            got.as_slice(),
            &[
                (
                    Some("host-b".into()),
                    vec![
                        "card".into(),
                        "apply".into(),
                        "c2".into(),
                        "--as".into(),
                        "user".into()
                    ]
                ),
                (
                    Some("host-b".into()),
                    vec![
                        "card".into(),
                        "apply".into(),
                        "c2".into(),
                        "--as".into(),
                        "user".into()
                    ]
                ),
                (
                    Some("host-b".into()),
                    vec![
                        "card".into(),
                        "cancel".into(),
                        "c2".into(),
                        "--reason".into(),
                        "cancelled from the call sheet".into(),
                        "--as".into(),
                        "user".into()
                    ]
                ),
                (
                    Some("host-b".into()),
                    vec![
                        "card".into(),
                        "cancel".into(),
                        "c2".into(),
                        "--reason".into(),
                        "cancelled from the call sheet".into(),
                        "--as".into(),
                        "user".into()
                    ]
                ),
                (
                    Some("host-b".into()),
                    vec![
                        "card".into(),
                        "show".into(),
                        "c2".into(),
                        "--as".into(),
                        "user".into()
                    ]
                ),
                (
                    Some("host-b".into()),
                    vec![
                        "card".into(),
                        "show".into(),
                        "c2".into(),
                        "--as".into(),
                        "user".into()
                    ]
                ),
            ]
        );
    }
    calls.lock().unwrap().clear();

    // 2. Studio Dock on remote outward card `c2`: moving cursor to `beta-tools` unfolds it, then `a`/`y`/`x`/`d`/`s`/Enter.
    app.hollywood.view = None;
    app.hollywood.mode = LayerMode::Hidden;
    press_mods(&mut app, hc_vt::Key::Char('b'), hc_vt::Mods::CTRL);
    press(&mut app, hc_vt::Key::Char('y'));
    draw_to(&mut app, 140, 40);
    app.panel.selected = Some(crate::panel::Target::Studio(crate::studio::StudioKey {
        machine: Some("host-b".into()),
        id: "beta-tools".into(),
    }));
    draw_to(&mut app, 140, 40);
    let remote_dock_target = app
        .panel_targets()
        .into_iter()
        .find(|t| format!("{t:?}").contains("\"c2\""))
        .expect("Studio Dock missing remote outward card target for c2");
    app.panel.selected = Some(remote_dock_target);

    press(&mut app, hc_vt::Key::Char('a'));
    let reply = app.action_rx.recv().await.expect("dock apply reply");
    app.on_action_reply(reply);
    press(&mut app, hc_vt::Key::Char('y'));
    let reply = app.action_rx.recv().await.expect("dock y apply reply");
    app.on_action_reply(reply);
    press(&mut app, hc_vt::Key::Char('x'));
    assert!(app.studio_dialog.is_some());
    press(&mut app, hc_vt::Key::Enter);
    let reply = app.action_rx.recv().await.expect("dock x cancel reply");
    app.on_action_reply(reply);
    press(&mut app, hc_vt::Key::Char('d'));
    assert!(app.studio_dialog.is_some());
    press(&mut app, hc_vt::Key::Enter);
    let reply = app.action_rx.recv().await.expect("dock d cancel reply");
    app.on_action_reply(reply);
    press(&mut app, hc_vt::Key::Char('s'));
    let reply = app.action_rx.recv().await.expect("dock show reply");
    app.on_action_reply(reply);
    assert!(app.hollywood.view.is_some(), "dock s opens CardView");
    press(&mut app, hc_vt::Key::Escape);
    press(&mut app, hc_vt::Key::Enter);
    let reply = app.action_rx.recv().await.expect("dock enter show reply");
    app.on_action_reply(reply);
    assert!(app.hollywood.view.is_some(), "dock Enter opens CardView");
    press(&mut app, hc_vt::Key::Escape);

    let got = calls.lock().unwrap();
    assert_eq!(got.len(), 6, "{got:?}");
    assert!(
        got.iter().all(|(m, _)| m.as_deref() == Some("host-b")),
        "{got:?}"
    );
    assert_eq!(got[0].1[..3], ["card", "apply", "c2"]);
    assert_eq!(got[1].1[..3], ["card", "apply", "c2"]);
    assert_eq!(got[2].1[..3], ["card", "cancel", "c2"]);
    assert_eq!(got[3].1[..3], ["card", "cancel", "c2"]);
    assert_eq!(got[4].1[..3], ["card", "show", "c2"]);
    assert_eq!(got[5].1[..3], ["card", "show", "c2"]);
}

#[tokio::test]
async fn remote_outward_card_runs_through_remote_runner() {
    let calls = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let (accepted, rx) = mpsc::channel(4);
    let (rpc, _sub) = crate::testcore::openers(accepted);
    let sink = calls.clone();
    let fake = crate::testcore::spawn_fake(rx, move |mut core, framed| {
        let calls = sink.clone();
        Box::pin(async move {
            if !framed {
                std::future::pending::<()>().await;
            }
            core.answer_hello(
                true,
                &[
                    "hello",
                    "terminal.run",
                    "terminal.rm",
                    "terminal.kill",
                    "machine.list",
                ],
                &[],
            )
            .await;
            while let Some(frame) = core.try_read_frame().await {
                if frame.channel != 0 || frame.kind != hc_proto::frame::Kind::Data {
                    continue;
                }
                let req: serde_json::Value = serde_json::from_slice(&frame.payload).unwrap();
                let result = match req["method"].as_str().unwrap_or("") {
                    "terminal.run" => {
                        let params = req["params"].clone();
                        calls.lock().unwrap().push(params.clone());
                        let output = if params["argv"][0] == "sh" {
                            "/home/user/.local/bin/backlot\n"
                        } else {
                            ""
                        };
                        let mut result = crate::testcore::minimal_terminal();
                        result["address"] = "host-b/r1".into();
                        result["output"] = output.into();
                        result["wait"] = serde_json::json!({
                            "terminal": "host-b/r1",
                            "ended": true,
                            "reason": "exit",
                            "turn": 1,
                            "exit_code": 0,
                        });
                        result
                    }
                    "terminal.rm" => serde_json::json!({"removed": true}),
                    _ => serde_json::Value::Null,
                };
                core.reply(&req["id"], result).await;
            }
        })
    });

    let mut app = setup_two_machine_app();
    app.set_remote_runner(crate::panel::RemoteRunner::new(rpc));
    if app.hollywood.mode != LayerMode::Expanded {
        press_mods(&mut app, hc_vt::Key::Char('y'), hc_vt::Mods::ALT);
    }
    let remote_target = app
        .hollywood_targets()
        .into_iter()
        .find(|t| format!("{t:?}").contains("\"c2\""))
        .expect("missing remote outward card target");
    app.hollywood_select(remote_target);
    press(&mut app, hc_vt::Key::Char('a'));

    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            if calls.lock().unwrap().len() >= 2 {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("remote outward action reached RemoteRunner");

    let runs = calls.lock().unwrap();
    assert_eq!(runs[1]["on"], "host-b");
    assert_eq!(
        runs[1]["argv"],
        serde_json::json!([
            "/home/user/.local/bin/backlot",
            "card",
            "apply",
            "c2",
            "--as",
            "user"
        ])
    );
    fake.abort();
}

#[tokio::test]
async fn folded_studio_outward_card_counts_in_folded_line_lot_pill_and_call_sheet() {
    let mut app = setup_local_app(false);
    let fold_doc = serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 2,
        "studios": [
            {
                "id": "acme-shop",
                "title": "Acme shop",
                "state": "live",
                "requirement_version": 1,
                "mode": "standard",
                "open_reviews": 0,
                "waiting": 0,
                "roles": [
                    {
                        "role": "producer",
                        "name": "producer",
                        "terminal": "t1",
                        "state": "live",
                        "work": "working"
                    }
                ]
            },
            {
                "id": "blog",
                "title": "Blog",
                "state": "live",
                "requirement_version": 1,
                "mode": "standard",
                "open_reviews": 0,
                "waiting": 0,
                "roles": [
                    {
                        "role": "producer",
                        "name": "producer",
                        "terminal": "t2",
                        "state": "live",
                        "work": "waiting"
                    },
                    {
                        "role": "director",
                        "name": "director",
                        "terminal": "t3",
                        "state": "live",
                        "work": "waiting"
                    }
                ],
                "outward_cards": [
                    outward_card_json("c1", "git push origin HEAD:main", "protected branch 'main'", "release_main")
                ]
            }
        ]
    });
    app.view.documents_mut().insert(
        DocKey {
            machine: None,
            name: "backlot".into(),
        },
        Document {
            value: fold_doc,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();

    // 1. Outward card in folded `blog` counts on the lot pill and renders in Call Sheet.
    assert_eq!(app.studio.lot.counts.urgent, 1, "lot pill urgent count");
    if app.hollywood.mode != LayerMode::Expanded {
        press_mods(&mut app, hc_vt::Key::Char('y'), hc_vt::Mods::ALT);
    }
    let sheet_text = screen_lines(&draw_to(&mut app, 140, 32)).join("\n");
    assert!(
        sheet_text.contains("c1") && sheet_text.contains("blog · git push origin HEAD:main"),
        "Call Sheet missing folded studio outward card:\n{sheet_text}"
    );

    // 2. Open Studio Dock while `acme-shop` stays selected -> `blog` is folded yet its `!1` counts.
    app.hollywood.mode = LayerMode::Hidden;
    press_mods(&mut app, hc_vt::Key::Char('b'), hc_vt::Mods::CTRL);
    press(&mut app, hc_vt::Key::Char('y'));
    assert_eq!(app.panel.mode, PanelMode::Expanded);
    let dock_lines = screen_lines(&draw_to(&mut app, 140, 32));
    let dock_text = dock_lines.join("\n");
    let blog_top = dock_lines
        .iter()
        .find(|l| l.contains("blog") && l.contains("!1"))
        .unwrap_or_else(|| panic!("expected folded blog header with `!1`:\n{dock_text}"));
    assert!(
        !dock_text.contains("c1 git push origin HEAD:main"),
        "folded studio must not expand outward card rows:\n{dock_text}"
    );
    let _ = blog_top;

    // 3. Cramped height (`body_height = 7 < 8`) stacks `blog` into one line with `!1`.
    let stacked_lines = screen_lines(&draw_to(&mut app, 140, 9));
    let stacked_text = stacked_lines.join("\n");
    assert!(
        stacked_lines
            .iter()
            .any(|l| l.contains("╶") && l.contains("blog") && l.contains("!1")),
        "expected stacked blog line with `!1`:\n{stacked_text}"
    );
}
