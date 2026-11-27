//! Slice 4 Reader action tests: `y` raw OSC 52 copy, `d` inert,
//! closed-role `[o] reopen` / hidden `[p]`, prompt-less `p` peek, `v` sheet,
//! `Ctrl+B ?` help while reading, scroll keys, and clickable chip rects.

use super::*;
use std::sync::{Arc, Mutex};

type RecordedCalls = Arc<Mutex<Vec<(Option<String>, Vec<String>)>>>;

fn attach_mock(app: &mut App, body: &'static str) -> RecordedCalls {
    let calls: RecordedCalls = Default::default();
    let sink = calls.clone();
    app.set_ns_run(Arc::new(move |machine, argv, _stdin| {
        sink.lock().unwrap().push((machine, argv));
        Box::pin(async move { Ok(body.to_owned()) })
    }));
    calls
}

fn capture_host(app: &mut App) -> Arc<Mutex<Vec<u8>>> {
    let out: Arc<Mutex<Vec<u8>>> = Default::default();
    let sink = out.clone();
    app.host_control = Box::new(move |bytes| {
        sink.lock().unwrap().extend_from_slice(bytes);
        Ok(())
    });
    out
}

async fn drain(app: &mut App) {
    tokio::task::yield_now().await;
    while let Ok(reply) = app.action_rx.try_recv() {
        app.on_action_reply(reply);
    }
}

fn reader_fixture_app() -> App {
    let mut app = app_with_panel_doc();
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = crate::app::pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
    let mut doc = summary_json();
    doc["studios"][0]["roles"] = serde_json::json!([
        {
            "role": "producer",
            "name": "producer",
            "terminal": "t1",
            "runner": "agy",
            "model": "pro",
            "state": "live",
            "work": "blocked",
            "attention": "permission prompt — needs the user",
            "attention_user": true,
            "attention_seq": 99,
            "attention_detail": "run cargo test"
        },
        {
            "role": "actor-dave",
            "name": "dave",
            "runner": "agy",
            "model": "pro",
            "state": "closed",
            "resumable": true
        },
        {
            "role": "actor-eve",
            "name": "eve",
            "runner": "agy",
            "model": "pro",
            "state": "closed",
            "resumable": false
        },
        {
            "role": "director",
            "name": "director",
            "runner": "agy",
            "model": "pro",
            "state": "closed",
            "resumable": false
        }
    ]);
    doc["studios"][0]["exchange"] = serde_json::json!([
        {
            "seq": 12,
            "who": "producer",
            "to": "user",
            "kind": "report",
            "task": "task-1",
            "text": "wrapped body ready",
            "file": "reports/12.md"
        },
        {
            "seq": 20,
            "who": "actor-dave",
            "to": "user",
            "kind": "outcome",
            "text": "dave closed resumable",
            "file": "reports/20.md"
        },
        {
            "seq": 21,
            "who": "director",
            "to": "user",
            "kind": "outcome",
            "text": "director fixed seat closed",
            "file": "reports/21.md"
        },
        {
            "seq": 22,
            "who": "actor-eve",
            "to": "user",
            "kind": "outcome",
            "text": "eve closed non-resumable",
            "file": "reports/22.md"
        }
    ]);
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: doc,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    app.hollywood.mode = crate::hollywood::LayerMode::Hidden;
    app
}

#[tokio::test]
async fn y_copies_exact_unwrapped_raw_source_via_osc52_and_d_is_inert_on_prompted_role() {
    let mut app = reader_fixture_app();
    let long_raw = "## Heading One\n第一段超级长的中文报告内容需要跨越多行显示在窄屏窗口里测试是否完整复制原始文本无截断，并保留反引号 `crates/hc-tui/src/hollywood/view.rs:42` 和路径。\n\nfinal line without wrap";
    let calls = attach_mock(&mut app, long_raw);
    let host_bytes = capture_host(&mut app);
    app.shell.clipboard_toast_enabled = true;
    app.area = (80, 24);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Talk("acme-shop".into(), 12));
    key(&mut app, hc_vt::Key::Enter);
    drain(&mut app).await;
    let rendered = screen_lines(&draw_to(&mut app, 80, 24)).join("\n");
    // Confirms wrapping visibly occurred at 80x24 without keeping the full raw single line unbroken.
    assert!(!rendered.contains(
        "第一段超级长的中文报告内容需要跨越多行显示在窄屏窗口里测试是否完整复制原始文本无截断"
    ));

    let before_calls = calls.lock().unwrap().len();
    // `d` stays completely inert even when author role has `attention_user == true`.
    key(&mut app, hc_vt::Key::Char('d'));
    assert!(app.hollywood.view.is_some());
    assert_eq!(calls.lock().unwrap().len(), before_calls);

    // `y` copies exact unwrapped raw UTF-8 via OSC 52 and never approves the prompt.
    key(&mut app, hc_vt::Key::Char('y'));
    assert_eq!(calls.lock().unwrap().len(), before_calls);
    let expected = crate::selection::osc52_sequence(long_raw.as_bytes());
    assert_eq!(host_bytes.lock().unwrap().as_slice(), expected.as_bytes());
    assert_eq!(
        app.copy_feedback.as_ref().map(|f| f.message.as_str()),
        Some("copied to clipboard")
    );
}

#[tokio::test]
async fn closed_author_hides_peek_shows_reopen_for_resumable_or_fixed_and_ignores_unresumable() {
    let mut app = reader_fixture_app();
    let calls = attach_mock(&mut app, "closed role report body");
    app.area = (120, 36);
    open_panel(&mut app);

    // 1. Closed resumable actor (`actor-dave`, seq 20): `□ closed`, `[o] reopen`, hides `[p]`, `o` dispatches `reopen`.
    app.panel.selected = Some(Target::Talk("acme-shop".into(), 20));
    key(&mut app, hc_vt::Key::Enter);
    drain(&mut app).await;
    let screen20 = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(screen20.contains("□ closed"), "{screen20}");
    assert!(screen20.contains(" o reopen "), "{screen20}");
    assert!(!screen20.contains(" peek"), "{screen20}");
    key(&mut app, hc_vt::Key::Char('p'));
    assert!(app.peek.is_none());
    key(&mut app, hc_vt::Key::Char('o'));
    assert!(
        calls
            .lock()
            .unwrap()
            .iter()
            .any(|(_, argv)| argv.join(" ").contains("reopen --person actor-dave")),
        "{:?}",
        calls.lock().unwrap()
    );
    key(&mut app, hc_vt::Key::Escape);

    // 2. Closed fixed seat (`director`, seq 21): `□ closed`, `[o] reopen`, `o` dispatches `reopen`.
    let before21 = calls.lock().unwrap().len();
    app.panel.selected = Some(Target::Talk("acme-shop".into(), 21));
    key(&mut app, hc_vt::Key::Enter);
    drain(&mut app).await;
    let screen21 = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(screen21.contains("□ closed"), "{screen21}");
    assert!(screen21.contains(" o reopen "), "{screen21}");
    assert!(!screen21.contains(" peek"), "{screen21}");
    key(&mut app, hc_vt::Key::Char('o'));
    assert!(
        calls.lock().unwrap()[before21..]
            .iter()
            .any(|(_, argv)| argv.join(" ").contains("reopen --person director")),
        "{:?}",
        calls.lock().unwrap()
    );
    key(&mut app, hc_vt::Key::Escape);

    // 3. Closed non-resumable actor (`actor-eve`, seq 22): hides both `[o]` and `[p]`, `o` is inert.
    app.panel.selected = Some(Target::Talk("acme-shop".into(), 22));
    key(&mut app, hc_vt::Key::Enter);
    drain(&mut app).await;
    let screen22 = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(screen22.contains("□ closed"), "{screen22}");
    assert!(
        !screen22.contains(" o reopen ") && !screen22.contains(" o focus "),
        "{screen22}"
    );
    assert!(!screen22.contains(" peek"), "{screen22}");
    let before_o = calls.lock().unwrap().len();
    key(&mut app, hc_vt::Key::Char('o'));
    key(&mut app, hc_vt::Key::Char('p'));
    assert_eq!(calls.lock().unwrap().len(), before_o);
    assert!(app.peek.is_none());
}

#[tokio::test]
async fn reader_p_opens_promptless_peek_v_opens_sheet_o_focuses_and_ctrl_b_question_opens_help() {
    let mut app = reader_fixture_app();
    let long_body = (1..=45)
        .map(|i| format!("report line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let leak: &'static str = Box::leak(long_body.into_boxed_str());
    let _calls = attach_mock(&mut app, leak);
    app.area = (120, 36);
    open_panel(&mut app);
    app.panel.selected = Some(Target::Talk("acme-shop".into(), 12));
    key(&mut app, hc_vt::Key::Enter);
    drain(&mut app).await;
    draw_to(&mut app, 120, 36);

    // Scroll shortcuts: `Ctrl+D`, `Space`, `b`, `Ctrl+U`, `G`, `g`.
    app.on_input(
        UiEvent::Key(term_key(
            hc_vt::Key::Char('d'),
            hc_vt::Mods::CTRL,
            hc_vt::Action::Press,
        )),
        120,
    );
    assert!(app.hollywood.view.as_ref().unwrap().scroll > 0);
    key(&mut app, hc_vt::Key::Char(' '));
    let mid = app.hollywood.view.as_ref().unwrap().scroll;
    key(&mut app, hc_vt::Key::Char('b'));
    assert!(app.hollywood.view.as_ref().unwrap().scroll < mid);
    app.on_input(
        UiEvent::Key(term_key(
            hc_vt::Key::Char('u'),
            hc_vt::Mods::CTRL,
            hc_vt::Action::Press,
        )),
        120,
    );
    key(&mut app, hc_vt::Key::Char('G'));
    let bottom_scroll = app.hollywood.view.as_ref().unwrap().scroll;
    assert!(bottom_scroll > 0);
    key(&mut app, hc_vt::Key::Char('g'));
    assert_eq!(app.hollywood.view.as_ref().unwrap().scroll, 0);

    // `p` opens read-only `open_peek` on `"t1"` without setting `peek.prompt`.
    key(&mut app, hc_vt::Key::Char('p'));
    let peek = app.peek.as_ref().expect("peek opened from Reader");
    assert_eq!(peek.terminal, "t1");
    assert!(
        peek.prompt.is_none(),
        "Reader peek must never set peek.prompt"
    );
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.peek.is_none());

    // `Ctrl+B ?` while reading opens help.
    prefix(&mut app);
    key(&mut app, hc_vt::Key::Char('?'));
    assert!(app.help, "Ctrl+B ? must open help while reading");
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.help);
    assert!(app.hollywood.view.is_some());

    // Clicking `[v]` chip closes Reader and opens Task Sheet on `task-1`.
    draw_to(&mut app, 120, 36);
    let v_rect = app
        .hollywood_hits
        .view
        .as_ref()
        .unwrap()
        .chips
        .iter()
        .find(|(_, c)| *c == 'v')
        .map(|(r, _)| *r)
        .expect("[v] chip hit rect");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        v_rect.x + 1,
        v_rect.y,
    ));
    assert!(app.hollywood.view.is_none());
    let sheet = app.sheet.as_ref().expect("Task Sheet opened by [v]");
    assert_eq!(sheet.studio.id, "acme-shop");
    assert_eq!(sheet.task.as_deref(), Some("task-1"));
    key(&mut app, hc_vt::Key::Escape);
    assert!(app.sheet.is_none());

    // Reopen #12 and press `o`: focuses author live terminal `t1` and closes Reader.
    app.panel.selected = Some(Target::Talk("acme-shop".into(), 12));
    key(&mut app, hc_vt::Key::Enter);
    drain(&mut app).await;
    app.focused = Some("t2".into());
    key(&mut app, hc_vt::Key::Char('o'));
    assert!(app.hollywood.view.is_none());
    assert_eq!(app.focused.as_deref(), Some("t1"));
}

#[tokio::test]
async fn slice4_visual_shots() {
    use unicode_width::UnicodeWidthStr;
    let Ok(dir) = std::env::var("LOT_READER_SHOTS") else {
        return;
    };
    let dump = |buf: &ratatui::buffer::Buffer, w: u16, h: u16| -> String {
        (0..h)
            .map(|y| {
                let mut row = String::new();
                let mut x = 0u16;
                while x < w {
                    let sym = buf[(x, y)].symbol();
                    row.push_str(sym);
                    x += UnicodeWidthStr::width(sym).max(1) as u16;
                }
                row
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    std::fs::create_dir_all(&dir).unwrap();

    // s4-80x24: narrow fallback strip `[t] [o] [y]  ? keys` (via small body forcing min_w 52 or squeeze).
    {
        let mut app = reader_fixture_app();
        let _calls = attach_mock(&mut app, "ok");
        app.area = (80, 24);
        open_panel(&mut app);
        app.panel.selected = Some(Target::Talk("acme-shop".into(), 12));
        key(&mut app, hc_vt::Key::Enter);
        drain(&mut app).await;
        let backend = draw_to(&mut app, 80, 24);
        std::fs::write(
            format!("{dir}/s4-80x24.txt"),
            dump(backend.buffer(), 80, 24),
        )
        .unwrap();
    }

    // s4-120x36: actionable CARD c3 + undimmed beside Lot Dock.
    {
        let mut app = reader_fixture_app();
        app.hollywood.mode = crate::hollywood::LayerMode::Collapsed;
        app.area = (120, 36);
        open_panel(&mut app);
        app.hollywood.view = Some(crate::hollywood::CardView {
            card: Some("c3".into()),
            title: "c3".into(),
            lines: vec![
                "Action: pause `t2` in `acme-shop` (host-a)".into(),
                "Why: repeated check failure on `t2`.".into(),
                "- Base check: `t2` status is `in_progress`".into(),
            ],
            raw_body: "Action: pause t2".into(),
            scroll: 0,
            meta: crate::hollywood::ReaderMeta {
                kind: crate::hollywood::ReaderKind::Card,
                studio: Some("acme-shop".into()),
                machine: Some("host-a".into()),
                from: Some("hollywood".into()),
                card_action: Some("pause t2".into()),
                card_status: Some("open".into()),
                nav_index: Some(1),
                nav_total: Some(2),
                origin: crate::hollywood::ReaderOrigin::Card("c3".into()),
                ..crate::hollywood::ReaderMeta::default()
            },
            ..crate::hollywood::CardView::default()
        });
        let backend = draw_to(&mut app, 120, 36);
        std::fs::write(
            format!("{dir}/s4-120x36.txt"),
            dump(backend.buffer(), 120, 36),
        )
        .unwrap();
    }

    // s4-200x44: full Tier 0 strip on Markdown outcome report + undimmed beside dock.
    {
        let mut app = reader_fixture_app();
        let body = (1..=20)
            .map(|i| {
                if i == 1 {
                    "## T1 Baseline & Checks".to_string()
                } else {
                    format!("- Verified line {i} in `crates/hc-tui/src/hollywood/reader_chrome.rs`")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let leak: &'static str = Box::leak(body.into_boxed_str());
        let _calls = attach_mock(&mut app, leak);
        app.area = (200, 44);
        open_panel(&mut app);
        app.panel.selected = Some(Target::Talk("acme-shop".into(), 12));
        key(&mut app, hc_vt::Key::Enter);
        drain(&mut app).await;
        let backend = draw_to(&mut app, 200, 44);
        std::fs::write(
            format!("{dir}/s4-200x44.txt"),
            dump(backend.buffer(), 200, 44),
        )
        .unwrap();
    }
}
