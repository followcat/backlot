//! Focused-panel keys: movement, flipping, group collapse, the Enter
//! actions (focus, the closed-seat notice, non-resumable refusal) and the
//! injected `priority` runner with its failure notice.
use super::*;
use crate::panel::Target;
use crate::studio::StudioKey;
use crate::test_support::TestDir;

/// The role-group rules are informative rows — the cursor steps over
/// them from one person to the next in group order.
#[tokio::test]
async fn the_cursor_skips_role_group_dividers() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    // With sidebar open, CalmSummary folds calm director + writer onto one summary row,
    // so Down from producer skips both CalmSummary and the actor-group divider.
    draw_to(&mut app, 120, 24);
    let producer = Target::Person("acme-shop".into(), "producer".into());
    app.panel.selected = Some(producer.clone());
    app.panel.selected_row = app.panel_targets().iter().position(|t| *t == producer);
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(
        app.panel.selected,
        Some(Target::Person("acme-shop".into(), "actor-alice".into())),
        "CalmSummary and Divider are never cursor stops"
    );

    // With sidebar collapsed, all 4 groups render separated by three `├──┤` dividers.
    app.shell_state.sidebar_collapsed = true;
    app.panel.sidebar_collapsed = true;
    draw_to(&mut app, 120, 24);
    app.panel.selected = Some(producer.clone());
    app.panel.selected_row = app.panel_targets().iter().position(|t| *t == producer);
    for next in ["director", "writer", "actor-alice"] {
        key(&mut app, hc_vt::Key::ArrowDown);
        assert_eq!(
            app.panel.selected,
            Some(Target::Person("acme-shop".into(), next.into())),
            "the divider is never a cursor stop"
        );
    }
}

/// Bump `updated_ms` in both documents and land the publish — the
/// republish rhythm a live two-machine client sees between presses.
fn republish(app: &mut App) {
    for key in [
        doc_key(),
        DocKey {
            machine: Some("host-b".into()),
            name: "backlot".into(),
        },
    ] {
        if let Some(doc) = app.view.documents.get(&key) {
            let mut value = doc.value.clone();
            if let Some(ms) = value["updated_ms"].as_u64() {
                value["updated_ms"] = (ms + 1).into();
            }
            app.view.documents_mut().insert(
                key,
                Document {
                    value,
                    ..Document::default()
                }
                .into(),
            );
        }
    }
    app.reconcile_published_view();
}

/// A `terminal.run`-recording fake core for the remote runner —
/// the same wire `panel/tests/remote_tests` drives.
fn serve_runs(
    calls: std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
) -> impl FnMut(crate::testcore::FakeCore, bool) -> hc_proto::watch::BoxFuture<()> + Send + Sync + 'static
{
    move |mut core, framed| {
        let calls = calls.clone();
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
                        let address = if params["argv"][0] == "sh" {
                            "host-b/r1"
                        } else {
                            "host-b/r2"
                        };
                        let output = if params["argv"][0] == "sh" {
                            "/home/user/.local/bin/backlot\n"
                        } else {
                            ""
                        };
                        let mut result = crate::testcore::minimal_terminal();
                        result["address"] = address.into();
                        result["output"] = output.into();
                        result["wait"] = serde_json::json!({
                            "terminal": address, "ended": true,
                            "reason": "exit", "turn": 1, "exit_code": 0,
                        });
                        result
                    }
                    "terminal.rm" => serde_json::json!({"removed": true}),
                    _ => serde_json::Value::Null,
                };
                core.reply(&req["id"], result).await;
            }
        })
    }
}

/// The focused terminal leaves the inventory: the pane goes dead
/// (input refused) while the dock stays as it is.
fn kill_focused_terminal(app: &mut App) {
    let terms: Vec<_> = app
        .view
        .inventory
        .terminals()
        .values()
        .filter(|&t| t.address != "t1")
        .cloned()
        .collect();
    app.view.inventory_mut().replace(2, terms);
    let view = app.view.clone();
    app.pane.on_view(&view);
}

/// The focused terminal returns with its original incarnation: the
/// pane takes input again, no respawn in between.
fn revive_focused_terminal(app: &mut App) {
    let mut terms: Vec<_> = app.view.inventory.terminals().values().cloned().collect();
    let mut t1 = crate::sidebar::fixture::term("t1");
    t1.pid = Some(1);
    t1.created_ms = 0;
    terms.push(t1);
    app.view.inventory_mut().replace(2, terms);
    let view = app.view.clone();
    app.pane.on_view(&view);
}

#[tokio::test]
async fn focused_panel_keys_move_flip_collapse_and_do_not_leak() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    // Opening selects the first actionable row — the block top, not
    // the title (A2.2).
    assert_eq!(app.panel.selected, Some(Target::Studio("acme-shop".into())));
    // j lands on the first person, k steps back to the block — never
    // on the title or a divider.
    key(&mut app, hc_vt::Key::Char('j'));
    assert_eq!(
        app.panel.selected,
        Some(Target::Person("acme-shop".into(), "producer".into()))
    );
    key(&mut app, hc_vt::Key::Char('k'));
    assert_eq!(app.panel.selected, Some(Target::Studio("acme-shop".into())));
    key(&mut app, hc_vt::Key::ArrowUp);
    // PageDown leaps a viewport — the hit map comes from a real draw.
    draw_to(&mut app, 120, 24);
    key(&mut app, hc_vt::Key::PageDown);
    assert!(matches!(
        app.panel.selected,
        Some(
            Target::Studio(..)
                | Target::Block(..)
                | Target::Person(..)
                | Target::Task(..)
                | Target::Fold(..)
                | Target::Input(..)
        )
    ));
    // A leap that lands on the `›` line is `Resting` — there is no
    // editing to peel back and `c` reads as the mode key at once
    // (R3-B2's explicit `Compose`).
    assert!(!app.panel.editing);
    // c collapses the panel to its strip.
    key(&mut app, hc_vt::Key::Char('c'));
    assert_eq!(app.panel.mode, PanelMode::Collapsed);
    key(&mut app, hc_vt::Key::Char('c'));
    assert_eq!(app.panel.mode, PanelMode::Expanded);
    // Nothing reached the terminal.
    assert_eq!(app.pane.input_backlog().0, 0, "focused keys leaked");
}

#[tokio::test]
async fn space_collapses_and_expands_the_selected_block() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    // Draw once so `panel_hits.body` carries a real height — the
    // stacking decision reads it.
    draw_to(&mut app, 120, 24);
    app.panel.selected = Some(Target::Studio("acme-shop".into()));
    key(&mut app, hc_vt::Key::Char(' '));
    assert!(
        app.panel
            .collapsed_blocks
            .contains(&StudioKey::local("acme-shop"))
    );
    key(&mut app, hc_vt::Key::Char(' '));
    assert!(
        app.panel
            .expanded_blocks
            .contains(&StudioKey::local("acme-shop"))
    );
}

#[tokio::test]
async fn enter_on_a_person_focuses_their_terminal() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-alice".into()));
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(app.focused.as_deref(), Some("t4"));
    assert!(!app.panel.focused);
    assert_eq!(app.panel.mode, PanelMode::Expanded, "beside stays open");
}

#[tokio::test]
async fn a_full_screen_panel_owns_the_keyboard() {
    let mut app = app_with_panel_doc();
    app.area = (40, 20);
    open_panel(&mut app);
    assert!(app.panel_fullscreen());
    assert!(app.panel_focused());
    key(&mut app, hc_vt::Key::Char('j'));
    // At 40 cols the focused overlay stays Expanded (`R3-B4`), so `j`
    // steps into the expanded block rows and no key leaks to the pane.
    assert!(
        matches!(app.panel.selected, Some(Target::Person(..))),
        "{:?}",
        app.panel.selected
    );
    assert_eq!(app.pane.input_backlog().0, 0, "keys leaked to the pane");
    // When unfocused at < 100 cols (`R3-1`), the dock auto-hides to 0
    // cols and returns keyboard focus to the pane.
    app.panel.focused = false;
    assert_eq!(app.panel_mode_at(40), PanelMode::Closed);
    assert!(!app.panel_fullscreen());
    assert!(!app.panel_focused());
}

#[tokio::test]
async fn esc_closes_a_full_screen_panel() {
    let mut app = app_with_panel_doc();
    app.area = (40, 20);
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(app.panel.mode, PanelMode::Closed);
    // Beside the panes Esc only leaves focus, panel stays open.
    app.area = (200, 24);
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.panel.focused);
    assert_eq!(app.panel.mode, PanelMode::Expanded);
}

#[tokio::test]
async fn the_chord_closes_full_screen_on_the_first_press() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    // Shrink under the threshold while focused: the overlay stays open
    // and the next chord closes on the first press.
    app.area = (40, 20);
    assert!(app.panel_fullscreen());
    prefix(&mut app);
    key(&mut app, hc_vt::Key::Char('y'));
    assert_eq!(app.panel.mode, PanelMode::Closed);
    // If unfocused before shrinking, the dock auto-hides at 40 cols so
    // the chord opens the Expanded overlay cleanly.
    app.area = (120, 24);
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.panel.focused);
    app.area = (40, 20);
    assert_eq!(app.panel_mode_at(40), PanelMode::Closed);
    prefix(&mut app);
    key(&mut app, hc_vt::Key::Char('y'));
    assert_eq!(app.panel.mode, PanelMode::Expanded);
    assert!(app.panel.focused);
}

#[tokio::test]
async fn enter_on_the_strip_row_closes_a_full_screen_dock() {
    let mut app = app_with_panel_doc();
    app.area = (40, 20);
    open_panel(&mut app);
    // Part 1 §1.2.3 & §1.2.4: Enter on Studio header enters InsideStudio
    // landing on priority row (`actor-bob` / `t5`, blocked); 2nd Enter on that
    // person row opens its terminal and closes the full-screen overlay.
    key(&mut app, hc_vt::Key::Enter);
    assert!(app.panel.focused);
    assert_eq!(app.panel.mode, PanelMode::Expanded);
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(app.focused.as_deref(), Some("t5"));
    assert_eq!(app.panel.mode, PanelMode::Closed);
}

#[tokio::test]
async fn a_closed_persons_enter_notices_instead_of_reopening() {
    // R3-M11: `o` owns the reopen — Enter on a closed seat explains
    // itself and never respawns the agent.
    let mut app = app_with_panel_doc();
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    app.set_panel_runner(std::sync::Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok(()) })
    }));
    app.area = (120, 24);
    open_panel(&mut app);
    app.panel.closed_folds.insert("acme-shop".into());
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-dave".into()));
    key(&mut app, hc_vt::Key::Enter);
    assert!(
        recorded.lock().unwrap().is_empty(),
        "Enter never runs reopen"
    );
    assert!(app.notice.is_some(), "the closed notice names `o`");
}

#[tokio::test]
async fn a_bad_panel_key_still_toggles_with_y() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    let dir = TestDir::new("panel-key");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "[panel]\nkey = \"nope+\"\n").unwrap();
    let (config, notice) = crate::panel::load(&path);
    app.set_panel_config(config, notice, &path);
    assert!(app.notice.is_some(), "the bad key notices once");
    // The notice names the file, never the bogus value — the screen
    // shows no "nope".
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(!text.contains("nope"), "{text}");
    // The combo fell back to `y` — the chord still toggles.
    prefix(&mut app);
    key(&mut app, hc_vt::Key::Char('y'));
    assert_eq!(app.panel.mode, PanelMode::Expanded);
}

#[tokio::test]
async fn esc_backs_out_of_the_dock() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    assert!(app.panel.focused);
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.panel.focused, "the keys went back to the pane");
}

#[tokio::test]
async fn a_dead_panes_refused_keys_hand_off_to_the_drawn_dock() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    // `esc back`: the dock stays painted beside the panes, unfocused.
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.panel.focused);
    assert!(app.panel_beside(120));
    // The focused terminal leaves the inventory: the pane goes dead
    // while the dock still sits beside it.
    kill_focused_terminal(&mut app);
    // A plain key is refused — and the drawn dock takes the keys.
    let backlog = app.pane.input_backlog();
    key(&mut app, hc_vt::Key::Char('m'));
    assert_eq!(app.pane.input_backlog().0, backlog.0, "the key was refused");
    assert!(
        app.pane.input_backlog().1 > backlog.1,
        "the key was refused"
    );
    assert!(app.panel.focused, "the dock took the keys");
    // The notice says where the keys went; the dock's footer reads
    // `esc close` while the pane refuses input.
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(text.contains("Pane exited"), "{text}");
    assert!(text.contains("keys go to the dock"), "{text}");
    assert!(text.contains("esc close"), "{text}");
    // The dock's own Esc now closes it outright.
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(app.panel.mode, PanelMode::Closed);
}

#[tokio::test]
async fn a_handoff_selects_a_row_on_a_dock_never_toggled() {
    // The launch default draws the dock open, never toggled: no row
    // cursor exists. A dead pane's refused key hands the dock the
    // keys — the footer's `m`/`⏎` promise must land on a real row,
    // not fall back to the title with no menu and no action (the
    // "looks interactive but isn't" class).
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    // The fixture boots --plain (closed); the field is the launch
    // default a non-plain run starts with — open, unfocused, no row.
    app.panel.mode = PanelMode::Expanded;
    assert!(app.panel.open() && !app.panel.focused);
    assert_eq!(app.panel.selected, None);
    kill_focused_terminal(&mut app);
    // A plain key is refused — the dock takes the keys and a real
    // row is selected.
    key(&mut app, hc_vt::Key::Char('m'));
    assert!(app.panel.focused, "the dock took the keys");
    assert_eq!(
        app.panel.selected,
        Some(Target::Studio("acme-shop".into())),
        "the hand-off left a row cursor"
    );
    // `m` now opens the selected row's menu.
    key(&mut app, hc_vt::Key::Char('m'));
    assert!(app.menu.is_some(), "the selected row's menu opened");
}

#[tokio::test]
async fn a_chord_focused_dock_over_a_dead_pane_reads_esc_close_and_esc_closes() {
    // No hand-off ever fired: the pane died first, then the chord
    // focused the dock. Esc must still close — the keys have no live
    // pane to go back to — and the footer must say so, not `esc back`.
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    app.panel.mode = PanelMode::Expanded;
    kill_focused_terminal(&mut app);
    prefix(&mut app);
    key(&mut app, hc_vt::Key::Char('y'));
    assert!(app.panel.focused, "the chord focused the dock");
    assert!(app.pane.input_refused());
    let text = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(text.contains("esc close"), "{text}");
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(app.panel.mode, PanelMode::Closed);
}

#[tokio::test]
async fn a_revived_pane_flips_the_dock_footer_back_live() {
    // At this width the focused dock overlays the panes. Open it
    // while the pane refuses input: Esc closes — the keys have
    // nowhere to go back to. The pane comes back and the footer
    // flips to `esc back` live — focus stays in the dock (one-way,
    // no latch), and Esc still closes the overlay outright.
    let mut app = app_with_panel_doc();
    app.area = (50, 24);
    open_panel(&mut app);
    draw_to(&mut app, 50, 24);
    assert!(!app.panel_beside(50), "the dock overlays at 50");
    kill_focused_terminal(&mut app);
    // A left-click on the overlay's body — below every row — keeps
    // focus without acting like Enter. The pane's gone-card paints
    // the footer row at this width, so the closed-out rule is read
    // from the predicate the key and the label share.
    let area = app.panel_hits.area;
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        area.x + 1,
        area.y + area.height - 2,
    ));
    assert!(app.panel.focused, "the click kept dock focus");
    assert_eq!(app.focused.as_deref(), Some("t1"), "no row acted");
    assert!(app.panel_esc_closes(), "Esc closes over a refusing pane");
    // The terminal returns: the pane takes input again and the
    // footer flips back.
    revive_focused_terminal(&mut app);
    assert!(!app.pane.input_refused(), "the pane takes input again");
    let text = screen_lines(&draw_to(&mut app, 50, 24)).join("\n");
    assert!(text.contains("esc back"), "{text}");
    assert!(!text.contains("esc close"), "{text}");
    // Focus stayed in the dock; Esc closes the overlay outright.
    assert!(app.panel.focused);
    key(&mut app, hc_vt::Key::Escape);
    assert_eq!(app.panel.mode, PanelMode::Closed);
}

#[tokio::test]
async fn enter_on_a_studio_focuses_its_producer() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Studio("acme-shop".into()));
    // Part 1 §1.2.3 & §1.2.4: 1st Enter enters InsideStudio landing on
    // priority row (`actor-bob` / `t5`); 2nd Enter on that live person focuses
    // its terminal and leaves Lot.
    key(&mut app, hc_vt::Key::Enter);
    assert!(app.panel.focused);
    assert!(matches!(
        app.focus.workspace,
        crate::focus::WorkspaceRegion::Lot(crate::focus::LotLevel::InsideStudio(_))
    ));
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(app.focused.as_deref(), Some("t5"));
    assert!(!app.panel.focused);
}

#[tokio::test]
async fn a_closed_person_reopens_on_o() {
    let mut app = app_with_panel_doc();
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    app.set_panel_runner(std::sync::Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok(()) })
    }));
    app.area = (120, 24);
    open_panel(&mut app);
    app.panel.closed_folds.insert("acme-shop".into());
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-dave".into()));
    key(&mut app, hc_vt::Key::Char('o'));
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec![
            "reopen".to_string(),
            "--person".into(),
            "actor-dave".into(),
            "--studio".into(),
            "acme-shop".into(),
            "--as".into(),
            "user".into(),
        ]]
    );
}

#[tokio::test]
async fn a_non_resumable_person_says_why_instead_of_reopening() {
    let mut app = app_with_panel_doc();
    app.area = (120, 24);
    open_panel(&mut app);
    app.panel.closed_folds.insert("acme-shop".into());
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-eve".into()));
    key(&mut app, hc_vt::Key::Enter);
    assert!(app.notice.is_some(), "the refusal notices");
}

#[tokio::test]
async fn plus_and_minus_raise_and_lower_an_actors_priority() {
    let mut app = app_with_panel_doc();
    let recorded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<String>>::new()));
    let sink = recorded.clone();
    app.set_panel_runner(std::sync::Arc::new(move |argv| {
        sink.lock().unwrap().push(argv);
        Box::pin(async { Ok(()) })
    }));
    app.area = (120, 24);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-alice".into()));
    key(&mut app, hc_vt::Key::Char('+'));
    key(&mut app, hc_vt::Key::Char('-'));
    let argv = recorded.lock().unwrap();
    assert_eq!(argv.len(), 2);
    // `priority --task task-1 --level high|low --studio acme-shop --as user`
    assert_eq!(argv[0][..2], ["priority", "--task"]);
    assert_eq!(argv[0][2], "task-1");
    assert_eq!(argv[0][4], "high");
    // The second press reads the document's (still `normal`) level —
    // the stage has not reported the raise back yet.
    assert_eq!(argv[1][4], "low");
    assert_eq!(argv[0][6], "acme-shop");
    // Bob is already low — minus there is a no-op.
    drop(argv);
    app.panel.selected = Some(Target::Person("acme-shop".into(), "actor-bob".into()));
    key(&mut app, hc_vt::Key::Char('-'));
    assert_eq!(recorded.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn a_failed_action_shows_its_first_stderr_line() {
    let mut app = app_with_panel_doc();
    app.on_panel_action(("priority alice high".into(), Err("studio went away".into())));
    assert!(app.action_toast.is_some());
    let backend = draw_to(&mut app, 120, 40);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("studio went away"), "{text}");
}

#[tokio::test]
async fn keys_reach_a_remote_actor_across_publishes() {
    let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::<serde_json::Value>::new()));
    let (accepted, rx) = mpsc::channel(4);
    let (rpc, _sub) = crate::testcore::openers(accepted);
    let fake = crate::testcore::spawn_fake(rx, serve_runs(calls.clone()));
    let mut app = app_with_two_machines();
    app.set_remote_runner(crate::panel::RemoteRunner::new(rpc));
    app.area = (160, 47);
    open_panel(&mut app);
    draw_to(&mut app, 160, 47);
    // `↓` once per target past acme-shop's block to `actor-a` on
    // `host-b` — a document republish lands between every press, so
    // the selection must survive rebuilt row lists to get there.
    // (`↓`, not `j`: landing on a `›` line makes the keys text —
    // the arrow is what steps through, the same as the Call Sheet.)
    let actor = Target::Person(
        StudioKey {
            machine: Some("host-b".into()),
            id: "beta-tools".into(),
        },
        "actor-a".into(),
    );
    for _ in 0..35 {
        if app.panel.selected == Some(actor.clone()) {
            break;
        }
        if matches!(app.panel.selected, Some(Target::Block(_))) {
            key(&mut app, hc_vt::Key::Enter);
            continue;
        }
        republish(&mut app);
        key(&mut app, hc_vt::Key::ArrowDown);
    }
    assert_eq!(app.panel.selected, Some(actor.clone()));
    // `-` lowers the actor's high task to normal — on host-b.
    key(&mut app, hc_vt::Key::Char('-'));
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            if calls.lock().unwrap().len() >= 2 {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the remote action reached the core");
    let runs = calls.lock().unwrap();
    let action = &runs[1];
    assert_eq!(action["on"], "host-b");
    assert_eq!(
        action["argv"],
        serde_json::json!([
            "/home/user/.local/bin/backlot",
            "priority",
            "--task",
            "t1",
            "--level",
            "normal",
            "--studio",
            "beta-tools",
            "--as",
            "user",
        ])
    );
    drop(runs);
    // Enter focuses the actor's machine-qualified terminal.
    republish(&mut app);
    key(&mut app, hc_vt::Key::Enter);
    assert_eq!(app.focused.as_deref(), Some("host-b/t10"));
    fake.abort();
}
