//! The cast's rows and renderer: two-line entries in cast order,
//! the focused person's `▌` and active background, attention/failed
//! rows, the title fallbacks, the `others` row and the spaces cap.
use super::rows::{CastRow, cast_rows};
use super::*;
use crate::chrome::agents::AgentRow;
use crate::chrome::palette::Palette;
use crate::chrome::render::ShellHitMap;
use crate::chrome::status::AgentStatus;
use crate::chrome::tokens::{ResolvedToken, ResolvedTokenKind};
use backlot_model::summary::Summary;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

fn summary_json() -> serde_json::Value {
    serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 1,
        "studios": [{
            "id": "acme-shop", "title": "Acme shop", "state": "live",
            "requirement_version": 1, "mode": "standard",
            "open_reviews": 0, "waiting": 0,
            "roles": [
                { "role": "producer", "name": "producer", "terminal": "t1",
                  "state": "live", "work": "working", "runner": "devin",
                  "model": "anthropic/claude-opus-4-7" },
                { "role": "actor-alice", "name": "alice", "terminal": "t4",
                  "state": "live", "work": "working", "task": "task-1",
                  "runner": "agent-a", "model": "claude-sonnet-4-5" },
                { "role": "actor-bob", "name": "bob", "terminal": "t5",
                  "state": "live", "work": "blocked", "task": "task-2",
                  "runner": "agent-b" },
                { "role": "actor-dave", "name": "dave",
                  "state": "closed", "resumable": true }
            ]
        }]
    })
}

fn summary() -> Summary {
    serde_json::from_value(summary_json()).unwrap()
}

fn config() -> ClientShellConfig {
    ClientShellConfig::from_config(
        &crate::config::Config::default(),
        None,
        None,
        std::path::PathBuf::new(),
    )
}

fn agent_row(pane_id: &str, label: &str) -> AgentRow {
    AgentRow {
        pane_id: pane_id.into(),
        status: AgentStatus::Idle,
        focused: false,
        rows: vec![vec![
            ResolvedToken {
                kind: ResolvedTokenKind::StateIcon,
                style: Default::default(),
            },
            ResolvedToken {
                kind: ResolvedTokenKind::Agent(label.into()),
                style: Default::default(),
            },
        ]],
    }
}

fn cast_view<'a>(
    summary: &'a Summary,
    others: Vec<AgentRow>,
    others_expanded: bool,
    focused: Option<&'a str>,
) -> CastView<'a> {
    CastView {
        studio: Some(StudioRef {
            machine: None,
            studio: &summary.studios[0],
        }),
        label: Some(crate::panel::data::StudioLabel {
            full: "acme-shop".into(),
            id: "acme-shop".into(),
            machine: None,
        }),
        index: 0,
        total: 1,
        focused_terminal: focused,
        titles: [
            ("t1".to_string(), Some("stage console".to_string())),
            ("t4".to_string(), Some("feature".to_string())),
        ]
        .into_iter()
        .collect(),
        shells: Vec::new(),
        others,
        others_expanded,
        spin: 0,
        now_ms: 1,
        studio_focused: false,
        hint: false,
        stopped: false,
        server_stopped: false,
    }
}

fn lines(buf: &Buffer, area: Rect) -> Vec<String> {
    (area.y..area.y + area.height)
        .map(|y| {
            (area.x..area.x + area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

fn draw(view: &CastView<'_>, width: u16, height: u16) -> (Buffer, ShellHitMap, usize) {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    let mut hits = ShellHitMap::default();
    let mut scroll = 0usize;
    let mut reveal = false;
    render_cast(
        &mut buf,
        area,
        view,
        &config(),
        &mut scroll,
        &mut reveal,
        &mut hits,
    );
    (buf, hits, scroll)
}

#[test]
fn the_header_names_the_studio_and_its_position() {
    let summary = summary();
    let view = cast_view(&summary, vec![], false, Some("t4"));
    let (buf, ..) = draw(&view, 26, 20);
    let text = lines(&buf, Rect::new(0, 0, 26, 20)).join("\n");
    assert!(text.contains("acme-shop"), "{text}");
    assert!(!text.contains("cast ·"), "{text}");
    assert!(text.contains("1/1"), "{text}");
}

#[test]
fn a_person_draws_one_line_with_role_badge_state_and_omits_runner_model() {
    let summary = summary();
    let view = cast_view(&summary, vec![], false, None);
    for width in [26u16, 20] {
        let (buf, ..) = draw(&view, width, 20);
        let text = lines(&buf, Rect::new(0, 0, width, 20)).join("\n");
        assert!(text.contains("Ⓟ"), "w{width}: {text}");
        assert!(text.contains("Ⓐ"), "w{width}: {text}");
        assert!(text.contains("dave"), "w{width}: {text}");
        assert!(!text.contains("agent-a"), "w{width}: {text}");
        assert!(!text.contains("opus"), "w{width}: {text}");
        assert!(!text.contains("sonnet"), "w{width}: {text}");
    }
    let (buf, ..) = draw(&view, 32, 20);
    let text = lines(&buf, Rect::new(0, 0, 32, 20)).join("\n");
    assert!(text.contains("Ⓐ task-1"), "{text}");
    assert!(text.contains("working"), "{text}");
    assert!(text.contains("dir"), "{text}");
    assert!(text.contains("closed"), "{text}");
}

#[test]
fn the_model_short_form_cuts_to_ten_cells() {
    assert_eq!(
        crate::studio::glyphs::short_model("anthropic/claude-opus-4-7"),
        "opus-4-7"
    );
    assert_eq!(
        crate::studio::glyphs::short_model("claude-verylongmodelname"),
        "verylongm…"
    );
}

#[test]
fn idle_detail_prefers_doing_then_terminal_title_excluding_idle() {
    let mut value = summary_json();
    value["studios"][0]["roles"][0]["work"] = serde_json::json!("waiting");
    value["studios"][0]["roles"][0]["doing"] = serde_json::json!("idle");
    let summary: Summary = serde_json::from_value(value.clone()).unwrap();
    let view = cast_view(&summary, vec![], false, None);
    let (buf, ..) = draw(&view, 40, 20);
    let rows = lines(&buf, Rect::new(0, 0, 40, 20));
    let prod = rows.iter().find(|l| l.contains("Ⓟ")).unwrap();
    assert!(prod.contains("idle · stage console"), "{prod}");

    value["studios"][0]["roles"][0]["doing"] = serde_json::json!("mends tests");
    let summary: Summary = serde_json::from_value(value).unwrap();
    let view2 = cast_view(&summary, vec![], false, None);
    let (buf, ..) = draw(&view2, 40, 20);
    let rows = lines(&buf, Rect::new(0, 0, 40, 20));
    let prod = rows.iter().find(|l| l.contains("Ⓟ")).unwrap();
    assert!(prod.contains("idle · mends tests"), "{prod}");
}

#[test]
fn attention_wins_over_failed_and_both_sit_inline_without_title() {
    let mut value = summary_json();
    value["studios"][0]["roles"][1]["attention"] = serde_json::json!("needs permission");
    value["studios"][0]["roles"][1]["failed"] = serde_json::json!("rate_limit");
    value["studios"][0]["roles"][2]["failed"] = serde_json::json!("rate_limit");
    let summary: Summary = serde_json::from_value(value).unwrap();
    let view = cast_view(&summary, vec![], false, None);
    let (buf, ..) = draw(&view, 48, 20);
    let rows = lines(&buf, Rect::new(0, 0, 48, 20));
    let alice = rows.iter().find(|l| l.contains("task-1")).unwrap();
    assert!(alice.contains("needs you · needs permission"), "{alice}");
    assert!(!alice.contains("feature"), "{alice}");
    let bob = rows.iter().find(|l| l.contains("task-2")).unwrap();
    assert!(bob.contains("failed · rate_limit"), "{bob}");
}

#[test]
fn long_detail_truncates_with_ellipsis_cjk_aware() {
    let mut value = summary_json();
    value["studios"][0]["roles"][0]["work"] = serde_json::json!("waiting");
    value["studios"][0]["roles"][0]["doing"] =
        serde_json::json!("请确认本批范围是否正确、有无需要增减？");
    let summary: Summary = serde_json::from_value(value).unwrap();
    let view = cast_view(&summary, vec![], false, None);
    let (buf, ..) = draw(&view, 25, 20);
    let rows = lines(&buf, Rect::new(0, 0, 25, 20));
    let prod = rows.iter().find(|l| l.contains("Ⓟ")).unwrap();
    assert!(prod.ends_with('…'), "expected ellipsis on cut: {prod}");
}

#[test]
fn the_focused_person_gets_the_overlay_bar_and_active_background() {
    let summary = summary();
    let view = cast_view(&summary, vec![], false, Some("t4"));
    let palette = Palette::from_name("catppuccin").unwrap();
    let (buf, hits, ..) = draw(&view, 26, 20);
    let alice = hits
        .agents
        .iter()
        .find(|(_, t)| t == "t4")
        .map(|(rect, _)| *rect)
        .expect("alice's hit row");
    assert_eq!(alice.height, 1);
    assert_eq!(buf[(alice.x, alice.y)].symbol(), "▎");
    assert_eq!(buf[(alice.x, alice.y)].fg, palette.overlay1);
    assert_eq!(buf[(alice.x, alice.y)].bg, palette.active_row_bg);
}

#[test]
fn cast_order_and_hit_rows_cover_the_people_and_others() {
    let summary = summary();
    let view = cast_view(&summary, vec![agent_row("t99", "outside")], false, None);
    let rows = cast_rows(&view);
    // 3 open people + closed divider + 1 closed person (dave) + the others row.
    assert_eq!(rows.len(), 6);
    assert!(matches!(rows[3], CastRow::ClosedDivider { count: 1 }));
    let order: Vec<&str> = rows
        .iter()
        .filter_map(|row| match row {
            CastRow::Person { role, .. } => Some(role.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(order, vec!["producer", "alice", "bob", "dave"]);

    let (buf, hits, ..) = draw(&view, 26, 20);
    let text = lines(&buf, Rect::new(0, 0, 26, 20)).join("\n");
    assert!(text.contains("┄ closed (1) ┄"), "{text}");
    assert!(!hits.cast_others.is_empty(), "the others row hit");
    // Person clicks carry the terminal, the others row its own rect.
    let terminals: Vec<&str> = hits.agents.iter().map(|(_, t)| t.as_str()).collect();
    assert!(terminals.contains(&"t4"), "{terminals:?}");
    assert!(!terminals.contains(&"t99"), "collapsed others draw no rows");
}

#[test]
fn the_expanded_others_row_lists_herdr_agent_rows() {
    let summary = summary();
    let view = cast_view(&summary, vec![agent_row("t99", "outside")], true, None);
    let (buf, hits, ..) = draw(&view, 30, 20);
    let text = lines(&buf, Rect::new(0, 0, 30, 20)).join("\n");
    assert!(text.contains("others 1 ▾"), "{text}");
    assert!(text.contains("outside"), "{text}");
    let terminals: Vec<&str> = hits.agents.iter().map(|(_, t)| t.as_str()).collect();
    assert!(terminals.contains(&"t99"), "expanded others click through");
}

#[test]
fn the_rows_scroll_and_the_track_draws_on_overflow() {
    let summary = summary();
    // Header is 3 rows; height 6 leaves 3 body rows for 5 cast rows.
    let view = cast_view(&summary, vec![], false, None);
    let (_, hits, scroll) = draw(&view, 26, 6);
    assert_eq!(scroll, 0);
    assert!(!hits.agent_scrollbar.is_empty(), "overflow draws the track");
    let mut buf = Buffer::empty(Rect::new(0, 0, 26, 6));
    let mut hits = ShellHitMap::default();
    let mut scroll = 1usize;
    let mut reveal = false;
    render_cast(
        &mut buf,
        Rect::new(0, 0, 26, 6),
        &view,
        &config(),
        &mut scroll,
        &mut reveal,
        &mut hits,
    );
    let text = lines(&buf, Rect::new(0, 0, 26, 6)).join("\n");
    assert!(!text.contains("Ⓟ"), "{text}");
    assert!(text.contains("task-1"), "{text}");
}

#[test]
fn a_focus_change_scrolls_the_focused_person_into_view() {
    // A body of one line (height 4) shows one person; alice (`t4`) is
    // second in the list, so the reveal moves the start down to her.
    let summary = summary();
    let view = cast_view(&summary, vec![], false, Some("t4"));
    let area = Rect::new(0, 0, 26, 4);
    let mut buf = Buffer::empty(area);
    let mut hits = ShellHitMap::default();
    let mut scroll = 0usize;
    let mut reveal = true;
    render_cast(
        &mut buf,
        area,
        &view,
        &config(),
        &mut scroll,
        &mut reveal,
        &mut hits,
    );
    assert!(!reveal, "the render consumes the armed reveal");
    assert_eq!(scroll, 1, "the list moved to alice's row");
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("task-1"), "{text}");
    assert!(!text.contains("Ⓟ"), "{text}");

    // Without an armed reveal the manual offset holds.
    let mut buf = Buffer::empty(area);
    let mut hits = ShellHitMap::default();
    let mut scroll = 0usize;
    let mut reveal = false;
    render_cast(
        &mut buf,
        area,
        &view,
        &config(),
        &mut scroll,
        &mut reveal,
        &mut hits,
    );
    assert_eq!(scroll, 0, "no focus change, no scroll");
    assert!(lines(&buf, area).join("\n").contains("Ⓟ"));
}

#[test]
fn the_spaces_cap_keeps_a_third_of_the_height_for_spaces() {
    // Plenty of space: the cap is a third of the section.
    let (spaces, cast) = sections(Rect::new(0, 0, 26, 30), 8);
    assert_eq!(spaces.height, 8);
    assert_eq!(cast.height, 22);
    // Too many spaces rows: a third (10) is all they get.
    let (spaces, cast) = sections(Rect::new(0, 0, 26, 30), 40);
    assert_eq!(spaces.height, 10);
    assert_eq!(cast.height, 20);
    // A tiny sidebar still leaves the cast a row.
    let (spaces, cast) = sections(Rect::new(0, 0, 26, 4), 40);
    assert_eq!(spaces.height, 1);
    assert_eq!(cast.height, 3);
}

#[test]
fn stopped_cast_renders_start_untruncated_at_26_cols() {
    let summary = summary();
    let mut view = cast_view(&summary, vec![], false, None);
    view.studio = None;
    view.label = None;
    view.stopped = true;
    let (buf, ..) = draw(&view, 26, 10);
    let text = lines(&buf, Rect::new(0, 0, 26, 10)).join("\n");
    assert!(text.contains("start"), "at 26 cols: {text}");
    assert!(!text.contains('…'), "no ellipsis at 26 cols: {text}");
    assert!(
        text.contains("stage stopped · s start"),
        "compact form at 26 cols: {text}"
    );

    let (wide_buf, ..) = draw(&view, 34, 10);
    let wide = lines(&wide_buf, Rect::new(0, 0, 34, 10)).join("\n");
    assert!(
        wide.contains("stage stopped — s to start"),
        "full form when width fits: {wide}"
    );
}

#[test]
fn cast_divider_uses_border_muted_and_closed_divider_omits_when_homogeneous() {
    let mut s = summary();
    let cfg = config();
    let view = cast_view(&s, vec![], false, None);
    let (buf, ..) = draw(&view, 26, 20);
    assert_eq!(buf[(0, 0)].symbol(), "─");
    assert_eq!(buf[(0, 0)].fg, cfg.palette.elevation().border_muted);

    // Only open roles -> no ClosedDivider.
    s.studios[0].roles.retain(|r| r.state != "closed");
    let open_only = cast_view(&s, vec![], false, None);
    assert!(
        !cast_rows(&open_only)
            .iter()
            .any(|r| matches!(r, CastRow::ClosedDivider { .. }))
    );

    // Only closed roles -> no ClosedDivider.
    for role in &mut s.studios[0].roles {
        role.state = "closed".into();
    }
    let closed_only = cast_view(&s, vec![], false, None);
    assert!(
        !cast_rows(&closed_only)
            .iter()
            .any(|r| matches!(r, CastRow::ClosedDivider { .. }))
    );
}
