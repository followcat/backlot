use super::*;

#[tokio::test]
async fn draw_error_propagates() {
    let mut app = app();
    app.sched.mark_dirty(Instant::now());
    let mut term = Terminal::new(FailBackend).unwrap();
    assert!(app.maybe_draw(&mut term, Instant::now()).is_err());
}
#[tokio::test]
async fn draw_ok_resolves_cursor_shape_param() {
    let mut app = app();
    app.sched.mark_dirty(Instant::now());
    let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
    app.maybe_draw(&mut term, Instant::now()).unwrap();
    app.want_cursor.pos = Some((3, 5));
    app.want_cursor.shape = hc_vt::CursorShape::Bar;
    app.want_cursor.blinking = true;
    let cursor = app.resolve_host_cursor();
    assert_eq!(
        (cursor.x, cursor.y, cursor.visible, cursor.shape),
        (3, 5, true, 5)
    );
}
#[tokio::test]
async fn an_unemitted_draw_unarms_the_deadline_and_counts_no_draw() {
    let mut app = app();
    app.sched.mark_dirty(Instant::now());
    let mut term = Terminal::new(QuietBackend).unwrap();
    app.maybe_draw(&mut term, Instant::now()).unwrap();
    assert_eq!(app.sched.deadline(), None);
    assert_eq!(app.sched.draws, 0);
}
#[tokio::test]
async fn navigate_unknown_press_release_is_swallowed() {
    let mut app = app();
    app.mode = Mode::Navigate;
    let key = |k, action| {
        input::TerminalKey::new(hc_vt::KeyInput {
            key: k,
            mods: hc_vt::Mods::NONE,
            action,
        })
    };
    app.on_key(key(hc_vt::Key::Char('x'), hc_vt::Action::Press), 80);
    app.on_key(key(hc_vt::Key::Enter, hc_vt::Action::Press), 80);
    assert_eq!(app.mode, Mode::Navigate);
    assert!(app.notice.is_some());
    // `x` release after the mode switch: consumed tail, not input.
    app.on_key(key(hc_vt::Key::Char('x'), hc_vt::Action::Release), 80);
    let (pending, _) = app.pane.input_backlog();
    assert_eq!(pending, 0, "selector key tail leaked into the pane");
    // And Enter's own release is swallowed too.
    app.on_key(key(hc_vt::Key::Enter, hc_vt::Action::Release), 80);
    assert_eq!(app.pane.input_backlog().0, 0);
}
#[tokio::test]
async fn navigate_highlight_only_then_commit_or_cancel() {
    let mut app = app_with(&["t1", "t2"]);
    app.focused = Some("t1".to_string());
    app.mode = Mode::Normal;
    app.refresh_projection();
    app.sync_target();
    let spec = |app: &App| app.desired_spec().map(|s| s.terminal);

    // Enter via the real action so `before_navigate` is armed.
    app.on_action(crate::input::KeybindAction::WorkspacePicker, 80);
    assert_eq!(app.mode, Mode::Navigate);
    app.on_navigate(input::Navigate::Move(1));
    assert_eq!(
        spec(&app).as_deref(),
        Some("t1"),
        "highlight moved the pane"
    );

    app.on_navigate(input::Navigate::Accept);
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(spec(&app).as_deref(), Some("t2"), "Enter didn't commit");

    // A second Navigate + Esc retains the current focus unchanged.
    app.on_action(crate::input::KeybindAction::WorkspacePicker, 80);
    app.on_navigate(input::Navigate::Move(-1));
    assert_eq!(spec(&app).as_deref(), Some("t2"));
    app.on_navigate(input::Navigate::Cancel);
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(spec(&app).as_deref(), Some("t2"), "Esc didn't retain");
}
#[tokio::test]
async fn remote_endpoint_shows_qualified_identity() {
    let mut app = app_with(&["t1", "host-b/t3"]);
    app.focused = Some("t1".to_string());
    app.refresh_projection();
    let backend = draw_to(&mut app, 80, 24);
    let whole = screen_lines(&backend).join("\n");
    assert!(whole.contains("host-b"), "{whole}");
}
#[tokio::test]
async fn navigate_scrolls_highlight_into_view() {
    let mut app = app_with(&["t1", "t2", "t3", "t4", "t5", "t6", "t7", "t8"]);
    app.focused = Some("t1".to_string());
    app.refresh_projection();
    app.on_action(crate::input::KeybindAction::WorkspacePicker, 80);
    // `reveal_workspace` clamps against the last frame's hit map —
    // a frame has always preceded input in the real loop.
    let _ = draw_to(&mut app, 80, 8);
    for _ in 0..7 {
        app.on_navigate(input::Navigate::Move(1));
    }
    assert_eq!(
        app.navigate_target
            .as_ref()
            .map(|t| t.workspace_id.as_str()),
        Some("space:8:space-t8")
    );
    let backend = draw_to(&mut app, 80, 8);
    let whole = screen_lines(&backend).join("\n");
    assert!(whole.contains("space-t8"), "{whole}");
}
#[tokio::test]
async fn pane_inner_matches_measured_reference_grid() {
    let mut app = app_with(&["t1"]);
    app.focused = Some("t1".to_string());
    app.refresh_projection();
    for (w, h, ew, eh) in [(80, 24, 53, 23), (120, 40, 93, 39), (200, 50, 173, 49)] {
        let inner = draw::pane_inner(&mut app, w, h);
        assert_eq!(
            (inner.width, inner.height),
            (ew, eh),
            "{w}x{h} inner must be {ew}x{eh}"
        );
    }
}
#[tokio::test]
async fn navigate_draws_bounded_at_tiny_areas() {
    let mut app = app_with(&["t1"]);
    app.focused = Some("t1".to_string());
    app.refresh_projection();
    app.on_action(crate::input::KeybindAction::WorkspacePicker, 80);
    for (w, h) in [(1, 1), (4, 2), (8, 3), (12, 4)] {
        let _ = draw_to(&mut app, w, h);
    }
    assert_eq!(
        app.navigate_target
            .as_ref()
            .map(|t| t.workspace_id.as_str()),
        Some("space:8:space-t1")
    );
}
#[tokio::test]
async fn tiny_and_empty_areas_do_not_panic() {
    let mut app = app_with(&["t1", "t2"]);
    app.focused = Some("t1".to_string());
    app.refresh_projection();
    for (w, h) in [(0, 0), (1, 1), (4, 1), (20, 3), (39, 2)] {
        let backend = draw_to(&mut app, w, h);
        assert_eq!(screen_lines(&backend).len(), h as usize, "{w}x{h}");
    }
    // An empty inventory still draws a coherent shell.
    let mut empty = app_with_terms(vec![]);
    let backend = draw_to(&mut empty, 80, 6);
    let whole = screen_lines(&backend).join("\n");
    assert!(whole.contains("spaces"), "{whole}");
}
#[tokio::test]
async fn focused_row_stays_visible_in_a_long_list() {
    let addresses: Vec<String> = (1..=30).map(|n| format!("t{n}")).collect();
    let spaced: Vec<(&str, String)> = addresses
        .iter()
        .map(|a| (a.as_str(), format!("space-{a}")))
        .collect();
    let pairs: Vec<(&str, &str)> = spaced.iter().map(|(a, s)| (*a, s.as_str())).collect();
    let mut app = app_with_spaces(&pairs);
    app.focused = Some("t30".to_string());
    app.refresh_projection();
    app.shell_state.reveal_focused_workspace = true;
    let backend = draw_to(&mut app, 80, 10);
    let whole = screen_lines(&backend).join("\n");
    assert!(
        whole.contains("space-t30"),
        "focused row off-screen: {whole}"
    );
}
#[tokio::test]
async fn wide_and_combining_labels_fit_on_boundaries() {
    use unicode_width::UnicodeWidthStr;
    let mut wide = crate::sidebar::fixture::agent("t1", "working");
    wide.name = Some("エージェント".into());
    wide.labels.insert("herdr.space".into(), "名前".into());
    let mut comb = crate::sidebar::fixture::agent("t2", "idle");
    comb.name = Some("e\u{301}x\u{308}treme".into());
    let mut app = app_with_terms(vec![wide, comb]);
    app.focused = Some("t1".to_string());
    app.refresh_projection();
    let backend = draw_to(&mut app, 80, 12);
    let sidebar_w = app
        .shell
        .layout_config()
        .layout(
            80,
            12,
            app.shell_state.sidebar_collapsed,
            1,
            app.sidebar_width,
        )
        .sidebar
        .width;
    let whole = screen_lines(&backend).join("\n");
    // Wide glyphs draw one per cell (tail cells pad) — assert the
    // grapheme parts render rather than a collapsed blob.
    assert!(whole.contains('名') && whole.contains('前'), "{whole}");
    for line in whole.split('\n') {
        assert!(UnicodeWidthStr::width(line) <= 80, "{line:?}");
    }
    // The reference sidebar includes its divider in the configured
    // width. Its next column already belongs to the pane content.
    assert_eq!(app.hit_map.sidebar_divider.x, sidebar_w - 1);
    let buf = backend.buffer();
    for y in 0..backend.size().unwrap().height {
        let cell = buf[(sidebar_w - 1, y)].symbol();
        assert!(
            cell == " " || cell == "│" || cell == "▕" || cell.is_empty(),
            "{cell:?}"
        );
    }
}
#[tokio::test]
async fn control_characters_never_reach_the_buffer() {
    let mut t = crate::sidebar::fixture::agent("t1", "working");
    t.name = Some("a\x1b[31mb\x07".into());
    t.title = "x\x1b]8;;http://e\x07y".into();
    t.cwd = "/home/user/\x1bevil".into();
    t.labels.insert("herdr.space".into(), "sp\x1b[2Jace".into());
    let mut app = app_with_terms(vec![t]);
    app.focused = Some("t1".to_string());
    app.refresh_projection();
    for (w, h) in [(80, 12), (40, 6)] {
        let backend = draw_to(&mut app, w, h);
        let (bw, bh) = (
            backend.size().unwrap().width,
            backend.size().unwrap().height,
        );
        for y in 0..bh {
            for x in 0..bw {
                let sym = backend.buffer()[(x, y)].symbol();
                assert!(!sym.chars().any(|c| c.is_control()), "{sym:?}@{x},{y}");
            }
        }
    }
    let whole = screen_lines(&draw_to(&mut app, 80, 12)).join("\n");
    // `clean` strips the ESC byte; the remaining printable text
    // still renders verbatim (`[31mb`), it just can't execute.
    assert!(whole.contains("[31mb"), "sanitized text renders: {whole}");
}
#[tokio::test]
async fn a_long_title_cannot_erase_the_pane_identity() {
    let mut t = crate::sidebar::fixture::agent("t7", "working");
    t.name = Some("fix-tests".into());
    t.status.as_mut().unwrap().title = "x".repeat(200);
    let mut app = app_with_terms(vec![t]);
    app.focused = Some("t7".to_string());
    app.refresh_projection();
    let whole = screen_lines(&draw_to(&mut app, 80, 12)).join("\n");
    assert!(
        whole.contains("fix-tests") || whole.contains("fix"),
        "identity erased by title: {whole}"
    );
}
#[tokio::test]
async fn focus_survives_reorder_and_falls_back_on_removal() {
    let mut app = app_with(&["t1", "t9"]);
    app.focused = Some("t9".to_string());
    app.refresh_projection();
    assert_eq!(app.focused.as_deref(), Some("t9"));

    // A reordered publish keeps t9 focused — focus tracks the
    // terminal, not the row position.
    let terms: Vec<_> = ["t9", "t1"]
        .iter()
        .map(|a| {
            let mut t = crate::sidebar::fixture::term(a);
            t.labels.insert("herdr.space".into(), format!("space-{a}"));
            t
        })
        .collect();
    app.view.inventory.replace(2, terms);
    app.refresh_projection();
    assert_eq!(app.focused.as_deref(), Some("t9"));

    // Removing it selects the projection's first pane — the
    // deterministic fallback `on_view` performs.
    let terms: Vec<_> = vec![{
        let mut t = crate::sidebar::fixture::term("t1");
        t.labels.insert("herdr.space".into(), "space-t1".into());
        t
    }];
    app.view.inventory.replace(3, terms);
    app.mode = Mode::Normal;
    app.reconcile_published_view();
    assert_eq!(app.focused.as_deref(), Some("t1"));
}
#[tokio::test]
async fn host_text_commit_routes_only_plain_text_under_report_all() {
    let mut app = app();
    let key = |ch: char, action: hc_vt::Action, vt: &[u8]| {
        let mut k = input::TerminalKey::new(hc_vt::KeyInput {
            key: hc_vt::Key::Char(ch),
            mods: hc_vt::Mods::NONE,
            action,
        });
        k.text_commit = Some(ch.to_string());
        k.vt_bytes = vt.to_vec();
        k
    };
    let jie = key('界', hc_vt::Action::Press, "界".as_bytes());
    // Report-all off: plain text never becomes a commit.
    assert_eq!(app.host_text_commit(&jie), None);
    app.host_reports_all_keys = true;
    // Non-ASCII commits even without release reporting.
    assert_eq!(app.host_text_commit(&jie).as_deref(), Some("界"));
    // ASCII plain text is ambiguous until releases arrive —
    // a typed key could have produced it on a weak host.
    let e_text = key('e', hc_vt::Action::Press, b"e");
    assert_eq!(app.host_text_commit(&e_text), None);
    app.host_reports_key_releases = true;
    assert_eq!(app.host_text_commit(&e_text).as_deref(), Some("e"));
    // A key report (ESC-first bytes) stays a key — the fixture's
    // `CSI 101;;101u` `e` forwards through the encoder.
    let e_report = key('e', hc_vt::Action::Press, b"\x1b[101;;101u");
    assert_eq!(app.host_text_commit(&e_report), None);
    // Releases and repeats never commit text.
    let e_up = key('e', hc_vt::Action::Release, b"\x1b[101;;101:3u");
    assert_eq!(app.host_text_commit(&e_up), None);
    // Binding modes and overlays keep their keys.
    app.mode = Mode::Navigate;
    assert_eq!(app.host_text_commit(&jie), None);
    app.mode = Mode::Normal;
    app.help = true;
    assert_eq!(app.host_text_commit(&jie), None);
    // No generated text → nothing to commit.
    let mut bare = key('e', hc_vt::Action::Press, b"e");
    bare.text_commit = None;
    assert_eq!(app.host_text_commit(&bare), None);
}
