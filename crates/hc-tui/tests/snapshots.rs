//! Ratatui TestBackend snapshots: layout geometry and the exact text
//! rows render, across 120/80/79/40 columns and tiny areas.

mod common;

use common::{agent, machine, term, view};
use hc_proto::watch::{Link, View};
use hc_tui::sidebar::{EXPAND_MIN, Place, Sidebar, ViewMode, draw, place};
use ratatui::Terminal as RtTerminal;
use ratatui::backend::TestBackend;
use unicode_width::UnicodeWidthStr;

/// Buffer cells as position-indexed symbol strings, one entry per
/// physical cell. Cells a wide glyph's tail covers stay empty strings,
/// so `row.len()` is the cell count and slicing stays column-true.
fn cells(sb: &Sidebar, w: u16, h: u16) -> Vec<Vec<String>> {
    let mut t = RtTerminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| draw(sb, f.area(), f.buffer_mut())).unwrap();
    let buf = t.backend().buffer().clone();
    (0..h)
        .map(|y| {
            let mut row = Vec::with_capacity(w as usize);
            let mut skip = 0usize;
            for x in 0..w {
                if skip > 0 {
                    skip -= 1;
                    row.push(String::new());
                    continue;
                }
                let sym = buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" ");
                row.push(sym.to_string());
                skip = UnicodeWidthStr::width(sym).saturating_sub(1);
            }
            row
        })
        .collect()
}

fn row_text(row: &[String]) -> String {
    row.concat().trim_end().to_string()
}

/// What a terminal shows on each row.
fn screen(sb: &Sidebar, w: u16, h: u16) -> Vec<String> {
    cells(sb, w, h).iter().map(|r| row_text(r)).collect()
}

/// Sidebar column text only — rows beyond it are the pane placeholder.
fn col(sb: &Sidebar, w: u16, h: u16) -> Vec<String> {
    let width = match place(sb, ratatui::layout::Rect::new(0, 0, w, h)) {
        Place::Expanded { sidebar, .. } => sidebar.width,
        Place::Compact { .. } => w,
    };
    cells(sb, w, h)
        .iter()
        .map(|r| row_text(&r[..(width as usize).min(r.len())]))
        .collect()
}

fn mixed() -> View {
    let mut labeled = agent("t1", "blocked");
    labeled.labels.insert("herdr.space".into(), "webapp".into());
    labeled.name = Some("fix-tests".into());
    let mut worker = agent("t7", "working");
    worker.parent = Some("t1".into());
    worker.status.as_mut().unwrap().progress = Some(42);
    let mut down = machine("host-b", "unreachable");
    down.why = Some("ssh: connection refused".into());
    view(
        vec![
            labeled,
            worker,
            agent("t2", "done"),
            term("t3"),
            agent("host-b/t4", "working"),
        ],
        vec![down, machine("host-c", "connected")],
    )
}

#[test]
fn expanded_at_80_shows_machines_spaces_and_pane() {
    let v = mixed();
    let mut sb = Sidebar::new();
    sb.update(&v);
    let s = screen(&sb, 80, 14);
    assert_eq!(s[0].chars().next(), Some(' '), "title row starts clear");
    assert!(s[0].contains(" hc-tui "));
    assert!(s[0].contains("┌ terminal "), "pane area is reserved");
    assert!(s[1].contains("spaces"));
    assert!(s[1].contains("agents"));
    let s = col(&sb, 80, 14);
    assert_eq!(s[3], ">host-a       connected│");
    assert_eq!(s[4], "   proj (3) working 42%│");
    assert_eq!(s[7], "     t7 wo… working 42%│");
    assert_eq!(s[8], "   webapp (1)   blocked│");
    assert_eq!(s[9], "     t1 fix-te… blocked│");
    assert_eq!(s[10], " host-b ss… unreachable│");
    assert_eq!(s[13], " host-c       connected│");
}

#[test]
fn expanded_at_120_is_wider() {
    let v = mixed();
    let mut sb = Sidebar::new();
    sb.update(&v);
    let s = col(&sb, 120, 14);
    assert_eq!(s[2], "1 turn · 2 work · 1 ready    │");
    assert_eq!(s[7], "     t7 worker o… working 42%│");
    assert_eq!(s[10], " host-b ssh: con… unreachable│");
    assert_eq!(s[13], " host-c             connected│");
}

#[test]
fn below_80_collapses_to_one_line() {
    let v = mixed();
    let mut sb = Sidebar::new();
    sb.update(&v);
    let s79 = screen(&sb, 79, 14);
    assert_eq!(s79[0], " spaces │ 1 turn · 2 work · 1 ready · 1 offline");
    assert!(s79[1].starts_with("┌ terminal "), "pane keeps the rest");
    let s40 = screen(&sb, 40, 10);
    assert_eq!(s40[0], " spaces │ 1 turn · 2 work · 1 ready · 1…");
}

#[test]
fn toggle_forces_expanded_below_the_fold() {
    let v = mixed();
    let mut sb = Sidebar::new();
    sb.update(&v);
    sb.toggle(60);
    assert!(sb.expanded(60));
    let s = screen(&sb, 60, 12);
    assert!(s[3].contains("host-a"), "expanded list renders at 60");
    sb.toggle(60);
    assert!(!sb.expanded(60));
    assert_eq!(EXPAND_MIN, 80);
}

#[test]
fn agents_view_lists_all_agents_across_machines() {
    let v = mixed();
    let mut sb = Sidebar::new();
    sb.set_mode(ViewMode::Agents);
    sb.update(&v);
    let s = col(&sb, 80, 14);
    assert_eq!(s[3], ">t1 fix-tests   blocked│");
    assert_eq!(s[4], " t7 worker… working 42%│");
    assert_eq!(s[5], " host-b/t4      working│");
    assert_eq!(s[6], " t2                done│");
    assert_eq!(s[7], "                       │");
}

#[test]
fn long_lists_scroll_to_the_selection() {
    let terms: Vec<_> = (1..=30).map(|n| agent(&format!("t{n}"), "idle")).collect();
    let v = view(terms, vec![]);
    let mut sb = Sidebar::new();
    sb.update(&v);
    sb.last();
    assert_eq!(sb.selected_terminal(), Some("t30"));
    let s = col(&sb, 80, 10);
    let visible: Vec<&str> = s.iter().map(String::as_str).collect();
    assert!(
        visible
            .iter()
            .any(|l| l.starts_with(">") && l.contains("t30")),
        "selected row stays visible: {visible:?}"
    );
}

#[test]
fn tiny_and_empty_areas_do_not_panic() {
    let v = mixed();
    let mut sb = Sidebar::new();
    sb.update(&v);
    for (w, h) in [(0, 0), (1, 1), (4, 1), (20, 3), (39, 2)] {
        let s = screen(&sb, w, h);
        assert_eq!(s.len(), h as usize);
    }
    // An empty inventory still shows the machine heading.
    let mut empty = Sidebar::new();
    empty.update(&view(vec![], vec![]));
    let s = col(&empty, 80, 6);
    assert_eq!(s[3], ">host-a       connected│");
}

#[test]
fn wide_and_combining_labels_fit_on_boundaries() {
    let mut wide = agent("t1", "working");
    wide.name = Some("エージェント".into());
    wide.labels.insert("herdr.space".into(), "名前".into());
    let mut comb = agent("t2", "idle");
    comb.name = Some("e\u{301}x\u{308}treme".into());
    let v = view(vec![wide, comb], vec![]);
    let mut sb = Sidebar::new();
    sb.update(&v);
    let s = col(&sb, 80, 12);
    // Nothing overflows: every rendered row stays inside the sidebar
    // column (24 cells including its border). Cells, not scalars —
    // combining marks and wide glyphs count by display width.
    for line in &s {
        assert!(UnicodeWidthStr::width(line.as_str()) <= 24, "{line:?}");
    }
    assert!(s.iter().any(|l| l.contains("名前")));
    assert!(s.iter().any(|l| l.contains("e\u{301}x")));
}

#[test]
fn control_characters_never_reach_the_buffer() {
    let mut t = agent("t1", "working");
    t.name = Some("a\x1b[31mb\x07".into());
    t.title = "x\x1b]8;;http://e\x07y".into();
    t.cwd = "/home/user/\x1bevil".into();
    let mut v = view(vec![t], vec![]);
    v.link = Link::Offline {
        attempt: 1,
        error: "bad\x1b[2Jerr".into(),
    };
    let mut sb = Sidebar::new();
    sb.update(&v);
    // Inspect actual buffer cells — a control reaching the buffer must
    // fail even if text extraction would mask it.
    for (w, h) in [(80, 12), (40, 6)] {
        for row in cells(&sb, w, h) {
            for sym in row {
                assert!(!sym.chars().any(|c| c.is_control()), "{sym:?}");
            }
        }
    }
    // Sanitized text renders: the name's letters show, controls don't.
    assert!(col(&sb, 80, 12).iter().any(|l| l.contains("[31mb")));
}

#[test]
fn a_long_badge_cannot_erase_the_terminal_identity() {
    let mut t = agent("t7", "working");
    t.name = Some("fix-tests".into());
    t.status.as_mut().unwrap().msg = "x".repeat(200);
    t.status.as_mut().unwrap().extra.insert(
        "source".into(),
        serde_json::Value::String("a-very-long-source-extension-name".into()),
    );
    let mut other = agent("t8", "working");
    other.records.insert(
        "record-with-a-rather-long-name".into(),
        common::status("blocked"),
    );
    let v = view(vec![t, other], vec![]);
    let mut sb = Sidebar::new();
    sb.update(&v);
    let lines = col(&sb, 80, 12);
    // Identity keeps its reserved cells: the address and the start of
    // the name survive while the runaway badge is ellipsized.
    assert!(
        lines.iter().any(|l| l.contains("t7 fix")),
        "identity erased by badge: {lines:?}"
    );
}

#[test]
fn long_remote_alias_never_erases_terminal_ids() {
    let alias = "a-very-long-remote-alias";
    let mut t7 = agent(&format!("{alias}/t7"), "working");
    t7.name = Some("one".into());
    let mut t8 = agent(&format!("{alias}/t8"), "working");
    t8.name = Some("two".into());
    t8.records.insert(
        "record-with-a-rather-long-name".into(),
        common::status("blocked"),
    );
    let v = view(vec![t7, t8], vec![machine(alias, "connected")]);

    // Spaces view: the redundant `alias/` prefix is dropped under the
    // machine's own heading — `t7`/`t8` show in full at any width, and
    // each row still carries its reported word.
    for (w, force) in [(80, false), (40, true)] {
        let mut s = Sidebar::new();
        s.update(&v);
        if force {
            s.toggle(w);
        }
        let lines = col(&s, w, 10);
        for (id, word) in [(" t7 ", "working"), (" t8 ", "blocked")] {
            assert!(
                lines.iter().any(|l| l.contains(id) && l.contains(word)),
                "spaces@{w}: `{id}` + `{word}` missing under {alias}: {lines:?}"
            );
        }
    }

    // Agents view: the qualified address is the only machine info — its
    // `…/tN` suffix survives a long badge, and the badge leads with the
    // word so `working`/`blocked` still read on the same row.
    for (w, force) in [(80, false), (40, true)] {
        let mut s = Sidebar::new();
        s.set_mode(ViewMode::Agents);
        s.update(&v);
        if force {
            s.toggle(w);
        }
        let lines = col(&s, w, 10);
        for (id, word) in [("/t7", "working"), ("/t8", "blocked")] {
            assert!(
                lines.iter().any(|l| l.contains(id) && l.contains(word)),
                "agents@{w}: `{id}` + `{word}` missing under {alias}: {lines:?}"
            );
        }
    }
}
