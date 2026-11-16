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
        studio: &summary.studios[0],
        index: 0,
        total: 1,
        focused_terminal: focused,
        titles: [
            ("t1".to_string(), Some("stage console".to_string())),
            ("t4".to_string(), Some("feature".to_string())),
        ]
        .into_iter()
        .collect(),
        others,
        others_expanded,
        spin: 0,
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
    render_cast(&mut buf, area, view, &config(), &mut scroll, &mut hits);
    (buf, hits, scroll)
}

#[test]
fn the_header_names_the_studio_and_its_position() {
    let summary = summary();
    let view = cast_view(&summary, vec![], false, Some("t4"));
    let (buf, ..) = draw(&view, 26, 20);
    let text = lines(&buf, Rect::new(0, 0, 26, 20)).join("\n");
    assert!(text.contains("cast · acme-shop"), "{text}");
    assert!(text.contains("1/1"), "{text}");
}

#[test]
fn a_person_draws_two_rows_mark_name_task_runner_title_model() {
    let summary = summary();
    let view = cast_view(&summary, vec![], false, None);
    for width in [26u16, 20] {
        let (buf, ..) = draw(&view, width, 20);
        let text = lines(&buf, Rect::new(0, 0, width, 20)).join("\n");
        // Alice: mark, `alice task-1` (cut at 20), the runner kept.
        assert!(text.contains("alice ta"), "w{width}: {text}");
        assert!(text.contains("agent-a"), "w{width}: {text}");
        // Row 2: the title (cut at 20) and the short model.
        assert!(text.contains("featu"), "w{width}: {text}");
        assert!(text.contains("sonnet"), "w{width}: {text}");
        // The producer's model drops the provider and `claude-`.
        assert!(text.contains("opus-4-7"), "w{width}: {text}");
        assert!(!text.contains("anthropic/"), "w{width}: {text}");
        assert!(!text.contains("claude-sonnet"), "w{width}: {text}");
        // Closed people stay listed, after the live ones.
        assert!(text.contains("dave"), "w{width}: {text}");
    }
    // At 26 the full task and title survive.
    let (buf, ..) = draw(&view, 26, 20);
    let text = lines(&buf, Rect::new(0, 0, 26, 20)).join("\n");
    assert!(text.contains("alice task-1"), "{text}");
    assert!(text.contains("feature"), "{text}");
}

#[test]
fn the_model_short_form_cuts_to_ten_cells() {
    // Producer: `anthropic/claude-opus-4-7` → `opus-4-7` (8 cells).
    // Give alice a long one: `claude-verylongmodelname` → 10 cells `…`.
    let mut value = summary_json();
    value["studios"][0]["roles"][1]["model"] = serde_json::json!("claude-verylongmodelname");
    let summary: Summary = serde_json::from_value(value).unwrap();
    let view = cast_view(&summary, vec![], false, None);
    let (buf, ..) = draw(&view, 40, 20);
    let text = lines(&buf, Rect::new(0, 0, 40, 20)).join("\n");
    assert!(text.contains("verylongm…"), "{text}");
    assert!(!text.contains("verylongmodelname"), "{text}");
}

#[test]
fn the_title_falls_back_to_doing_then_the_state_word() {
    let summary = summary();
    let mut view = cast_view(&summary, vec![], false, None);
    // bob has no title entry → his `doing` when present, else his
    // state word; the fixture gives him neither, so `blocked`.
    view.titles.remove("t5");
    let (buf, ..) = draw(&view, 26, 20);
    let text = lines(&buf, Rect::new(0, 0, 26, 20)).join("\n");
    assert!(text.contains("blocked"), "{text}");
    // With a `doing` it wins over the state word.
    let mut value = summary_json();
    value["studios"][0]["roles"][2]["doing"] = serde_json::json!("mends tests");
    let summary: Summary = serde_json::from_value(value).unwrap();
    let view2 = cast_view(&summary, vec![], false, None);
    let (buf, ..) = draw(&view2, 26, 20);
    let text = lines(&buf, Rect::new(0, 0, 26, 20)).join("\n");
    assert!(text.contains("mends tests"), "{text}");
}

#[test]
fn attention_wins_over_failed_and_both_replace_the_title() {
    let mut value = summary_json();
    value["studios"][0]["roles"][1]["attention"] = serde_json::json!("needs permission");
    value["studios"][0]["roles"][1]["failed"] = serde_json::json!("rate_limit");
    value["studios"][0]["roles"][2]["failed"] = serde_json::json!("rate_limit");
    let summary: Summary = serde_json::from_value(value).unwrap();
    let view = cast_view(&summary, vec![], false, None);
    let (buf, ..) = draw(&view, 40, 20);
    let text = lines(&buf, Rect::new(0, 0, 40, 20)).join("\n");
    assert!(text.contains("! needs permission"), "{text}");
    assert!(text.contains("last turn failed"), "{text}");
    // The attention row hides both the title and the failure line on
    // alice's entry — only bob's failed line shows the words.
    let alice = lines(&buf, Rect::new(0, 0, 40, 20))
        .iter()
        .position(|l| l.contains("alice"))
        .unwrap();
    let row2 = &lines(&buf, Rect::new(0, 0, 40, 20))[alice + 1];
    assert!(row2.contains("! needs permission"), "{row2}");
    assert!(!row2.contains("feature"), "{row2}");
}

#[test]
fn an_alert_takes_the_whole_second_row_at_the_default_width() {
    // At the default 26 columns a model beside the alert left
    // `! permissi…`: the alert replaces row two, model included.
    let mut value = summary_json();
    value["studios"][0]["roles"][1]["attention"] = serde_json::json!("permission prompt");
    value["studios"][0]["roles"][0]["failed"] = serde_json::json!("rate_limit");
    let alerted: Summary = serde_json::from_value(value).unwrap();
    let view = cast_view(&alerted, vec![], false, None);
    let (buf, ..) = draw(&view, 26, 20);
    let rows = lines(&buf, Rect::new(0, 0, 26, 20));
    let below = |name: &str| rows[rows.iter().position(|l| l.contains(name)).unwrap() + 1].clone();
    let alice = below("alice");
    assert!(alice.contains("! permission prompt"), "{alice}");
    assert!(!alice.contains("sonnet"), "{alice}");
    let producer = below("producer");
    assert!(producer.contains("last turn failed"), "{producer}");
    assert!(!producer.contains("opus"), "{producer}");
    // Without an alert the model keeps its place.
    let plain = summary();
    let view = cast_view(&plain, vec![], false, None);
    let (buf, ..) = draw(&view, 26, 20);
    let rows = lines(&buf, Rect::new(0, 0, 26, 20));
    let alice = rows[rows.iter().position(|l| l.contains("alice")).unwrap() + 1].clone();
    assert!(alice.contains("sonnet-4-5"), "{alice}");
}

#[test]
fn the_focused_person_gets_the_accent_bar_and_active_background() {
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
    assert_eq!(buf[(alice.x, alice.y)].symbol(), "▌");
    assert_eq!(buf[(alice.x, alice.y)].fg, palette.accent);
    assert_eq!(buf[(alice.x, alice.y)].bg, palette.active_row_bg);
    assert_eq!(buf[(alice.x, alice.y + 1)].bg, palette.active_row_bg);
}

#[test]
fn cast_order_and_hit_rows_cover_the_people_and_others() {
    let summary = summary();
    let view = cast_view(&summary, vec![agent_row("t99", "outside")], false, None);
    let rows = cast_rows(&view);
    // 4 people (dave closed last) + the others row.
    assert_eq!(rows.len(), 5);
    let order: Vec<&str> = rows
        .iter()
        .filter_map(|row| match row {
            CastRow::Person { role, .. } => Some(role.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(order, vec!["producer", "alice", "bob", "dave"]);

    let (_, hits, ..) = draw(&view, 26, 20);
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
    // A body of six lines shows three of the four people.
    let view = cast_view(&summary, vec![], false, None);
    let (_, hits, scroll) = draw(&view, 26, 10);
    assert_eq!(scroll, 0);
    assert!(!hits.agent_scrollbar.is_empty(), "overflow draws the track");
    // Scrolled down two lines the first person leaves the frame.
    let mut buf = Buffer::empty(Rect::new(0, 0, 26, 10));
    let mut hits = ShellHitMap::default();
    let mut scroll = 2usize;
    render_cast(
        &mut buf,
        Rect::new(0, 0, 26, 10),
        &view,
        &config(),
        &mut scroll,
        &mut hits,
    );
    let text = lines(&buf, Rect::new(0, 0, 26, 10)).join("\n");
    assert!(!text.contains("producer"), "{text}");
    assert!(text.contains("alice"), "{text}");
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
