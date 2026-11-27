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
        region_focused: false,
        cursor_target: None,
        remembered_target: None,
        active_chip: None,
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
    use ratatui::style::Modifier;
    let summary = summary();
    let palette = config().palette;
    let view = cast_view(&summary, vec![], false, Some("t4"));
    let (buf, ..) = draw(&view, 26, 20);
    let text = lines(&buf, Rect::new(0, 0, 26, 20)).join("\n");
    assert!(text.contains("acme-shop"), "{text}");
    assert!(!text.contains("cast ·"), "{text}");
    assert!(text.contains("1/1"), "{text}");
    // Header stands out in bold `palette.text` even while the cast itself is unfocused.
    assert_eq!(buf[(1, 1)].fg, palette.text);
    assert!(buf[(1, 1)].modifier.contains(Modifier::BOLD));
    assert_eq!(buf[(25, 1)].fg, palette.text);
    assert!(buf[(25, 1)].modifier.contains(Modifier::BOLD));
}

#[test]
fn open_roles_show_runner_model_effort_subline_while_closed_and_missing_stay_one_line() {
    let mut val = summary_json();
    val["studios"][0]["roles"][0]["effort"] = serde_json::json!("xhigh");
    // Closed actor carrying runner/model still stays one line with no model row.
    val["studios"][0]["roles"][3]["runner"] = serde_json::json!("closed-runner");
    val["studios"][0]["roles"][3]["model"] = serde_json::json!("closed-model");
    let summary: Summary = serde_json::from_value(val).unwrap();
    let palette = Palette::from_name("catppuccin").unwrap();
    let view = cast_view(&summary, vec![], false, None);

    let (buf, ..) = draw(&view, 40, 20);
    let rows = lines(&buf, Rect::new(0, 0, 40, 20));
    let text = rows.join("\n");
    assert!(
        text.contains("devin · anthropic/claude-opus-4-7 ·"),
        "{text}"
    );
    assert!(text.contains("agent-a · claude-sonnet-4-5"), "{text}");
    // Bob has runner `agent-b` and missing model -> renders `agent-b` only (no trailing `·`).
    let bob_meta = rows
        .iter()
        .find(|l| l.contains("agent-b"))
        .expect("bob meta row");
    assert_eq!(bob_meta.trim(), "agent-b");
    let meta_x = bob_meta.find("agent-b").unwrap() as u16;
    let bob_y = rows.iter().position(|l| l.contains("agent-b")).unwrap() as u16;
    assert_eq!(buf[(meta_x, bob_y)].fg, palette.subtext0);

    // Closed dave never renders a model line.
    assert!(!text.contains("closed-runner"), "{text}");
    assert!(!text.contains("closed-model"), "{text}");

    // At 26 cols, long metadata truncates cleanly with `…`.
    let (nbuf, ..) = draw(&view, 26, 20);
    let ntext = lines(&nbuf, Rect::new(0, 0, 26, 20)).join("\n");
    assert!(ntext.contains("devin · anthropic/clau…"), "{ntext}");
    assert!(ntext.contains("agent-a · claude-sonne…"), "{ntext}");

    // Role with neither runner, model, nor effort stays strictly 1 line.
    let mut bare = summary_json();
    bare["studios"][0]["roles"][2]["runner"] = serde_json::Value::Null;
    let bare_summary: Summary = serde_json::from_value(bare).unwrap();
    let bare_view = cast_view(&bare_summary, vec![], false, None);
    let built = cast_rows(&bare_view);
    assert_eq!(built[2].lines(), 1);
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
fn role_labels_are_coloured_badges_with_the_state_glyph_kept() {
    let summary = summary();
    let view = cast_view(&summary, vec![], false, None);
    let (buf, ..) = draw(&view, 40, 24);
    let rows = lines(&buf, Rect::new(0, 0, 40, 24));
    let text = rows.join("\n");
    assert!(text.contains("Ⓟ"), "{text}");
    assert!(text.contains("Ⓐ task-1"), "{text}");
    let palette = Palette::from_name("catppuccin").unwrap();
    let col = |row: &str, needle: &str| row[..row.find(needle).unwrap()].chars().count() as u16;
    let producer = rows.iter().position(|l| l.contains("Ⓟ")).unwrap() as u16;
    assert_eq!(
        buf[(col(&rows[producer as usize], "Ⓟ"), producer)].fg,
        palette.mauve
    );
    assert_eq!(
        buf[(1, producer)].symbol(),
        crate::studio::glyphs::Person::Working.glyph(0)
    );
    let alice = rows.iter().position(|l| l.contains("Ⓐ task-1")).unwrap() as u16;
    assert_eq!(
        buf[(col(&rows[alice as usize], "Ⓐ"), alice)].fg,
        palette.peach
    );
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
    assert_eq!(alice.height, 2);
    assert_eq!(buf[(alice.x, alice.y)].symbol(), "▎");
    assert_eq!(buf[(alice.x, alice.y)].fg, palette.overlay1);
    assert_eq!(buf[(alice.x, alice.y)].bg, palette.active_row_bg);
    assert_eq!(buf[(alice.x + 3, alice.y + 1)].bg, palette.active_row_bg);
    assert_eq!(buf[(alice.x + 3, alice.y + 1)].fg, palette.subtext0);
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

#[test]
fn height_pressure_drops_unfocused_model_lines_before_focused_and_cjk_truncates_safely() {
    let mut val = summary_json();
    val["studios"][0]["roles"][1]["name"] = serde_json::json!("张伟核心");
    val["studios"][0]["roles"][1]["task"] = serde_json::Value::Null;
    val["studios"][0]["roles"][1]["effort"] = serde_json::json!("high");
    let summary: Summary = serde_json::from_value(val).unwrap();
    // Alice (`t4`) is focused; producer, alice, bob, closed divider, dave = 8 rows total when expanded.
    let view = cast_view(&summary, vec![], false, Some("t4"));

    // Generous height (`3` header + `8` body = 11): all open roles show their model line.
    let (wide_buf, ..) = draw(&view, 40, 11);
    let wide_text = lines(&wide_buf, Rect::new(0, 0, 40, 11)).join("\n");
    assert!(
        wide_text.contains("devin · anthropic/claude-opus-4-7"),
        "{wide_text}"
    );
    assert!(
        wide_text.contains("agent-a · claude-sonnet-4-5 · high"),
        "{wide_text}"
    );
    assert!(wide_text.contains("agent-b"), "{wide_text}");

    // Tight height (`3` header + `6` body = 9): unfocused roles (`bob`, `producer`) fall back to
    // 1 line first so focused `alice` keeps her model line and every role stays visible!
    let (tight_buf, ..) = draw(&view, 40, 9);
    let tight_text = lines(&tight_buf, Rect::new(0, 0, 40, 9)).join("\n");
    assert!(
        tight_text.contains("agent-a · claude-sonnet-4-5 · high"),
        "{tight_text}"
    );
    assert!(
        !tight_text.contains("devin"),
        "unfocused producer dropped model first: {tight_text}"
    );
    assert!(
        !tight_text.contains("agent-b"),
        "unfocused bob dropped model first: {tight_text}"
    );
    assert!(tight_text.contains("Ⓟ"), "producer role itself still shown");
    assert!(tight_text.contains("dave"), "closed actor still shown");

    // Ultra-tight height (`3` header + `5` body = 8): focused `alice` drops her model line too
    // so NO role is hidden to keep a model line.
    let (cramped_buf, ..) = draw(&view, 40, 8);
    let cramped_text = lines(&cramped_buf, Rect::new(0, 0, 40, 8)).join("\n");
    assert!(!cramped_text.contains("claude-sonnet"), "{cramped_text}");
    assert!(cramped_text.contains("Ⓟ"), "{cramped_text}");
    assert!(cramped_text.contains("张 伟 核 心"), "{cramped_text}");
    assert!(cramped_text.contains("dave"), "{cramped_text}");

    // CJK wide name + long CJK-friendly model line both truncate safely at 26 columns.
    let (cjk_buf, ..) = draw(&view, 26, 20);
    let cjk_rows = lines(&cjk_buf, Rect::new(0, 0, 26, 20));
    let alice_row = cjk_rows
        .iter()
        .find(|l| l.contains("张"))
        .expect("alice CJK row");
    assert!(alice_row.contains("work"), "{alice_row}");
    let meta_row = cjk_rows
        .iter()
        .find(|l| l.contains("agent-a"))
        .expect("alice meta");
    assert!(meta_row.ends_with('…'), "{meta_row}");
}

#[test]
fn deco_cast_divider_closed_rule_scroll_and_herdr_bypass() {
    let summary = summary();
    let mut cfg = config();
    cfg.palette = Palette::backlot();
    cfg.studio_chrome = true;
    let pal = &cfg.palette;
    let elev = pal.elevation();
    let area = Rect::new(0, 0, 26, 20);

    // 1. Unfocused Deco Cast: `──╡ ✦ CAST ╞────` in `gilt` on row 0,
    // `── · closed (1) · ───────` divider, 0 extra rows, click hits preserved.
    let unfocused = cast_view(&summary, Vec::new(), false, None);
    let mut ubuf = Buffer::empty(area);
    let mut uhits = ShellHitMap::default();
    let mut scroll = 0usize;
    let mut reveal = false;
    super::render_cast(
        &mut ubuf,
        area,
        &unfocused,
        &cfg,
        &mut scroll,
        &mut reveal,
        &mut uhits,
    );
    let ulines = lines(&ubuf, area);
    assert_eq!(ulines[0], "──╡ ✦ CAST ╞──────────────");
    assert_eq!(ubuf[(0, 0)].fg, elev.gilt);
    assert_eq!(ubuf[(4, 0)].fg, elev.gilt);
    assert_eq!(ubuf[(25, 0)].fg, elev.gilt);
    assert!(
        ulines[1].contains("acme-shop"),
        "costs 0 rows: {:?}",
        ulines[1]
    );
    let div_y = ulines
        .iter()
        .position(|l| l.contains("closed (1)"))
        .unwrap() as u16;
    assert!(
        ulines[div_y as usize].starts_with("── · closed (1) · ─"),
        "{:?}",
        ulines[div_y as usize]
    );
    assert_eq!(ubuf[(0, div_y)].fg, elev.border_muted);
    assert_eq!(ubuf[(3, div_y)].fg, elev.gilt);
    assert_eq!(ubuf[(5, div_y)].fg, elev.muted_surface);
    assert!(!uhits.agents.is_empty());

    // 2. Focused Deco Cast (`region_focused = true`): `━━╡ ▸ CAST ╞━━━━` in `accent` + `▌` bar.
    let mut focused = cast_view(&summary, Vec::new(), false, Some("t1"));
    focused.region_focused = true;
    focused.cursor_target = Some(crate::focus::CastTarget::Role {
        studio: crate::studio::StudioKey::local("acme-shop"),
        role: "producer".into(),
    });
    let mut fbuf = Buffer::empty(area);
    let mut fhits = ShellHitMap::default();
    super::render_cast(
        &mut fbuf,
        area,
        &focused,
        &cfg,
        &mut scroll,
        &mut reveal,
        &mut fhits,
    );
    let flines = lines(&fbuf, area);
    assert_eq!(flines[0], "━━╡ ▸ CAST ╞━━━━━━━━━━━━━━");
    assert_eq!(fbuf[(0, 0)].fg, pal.accent);
    assert_eq!(fbuf[(4, 0)].fg, pal.accent);
    assert!(
        flines.iter().any(|l| l.starts_with('▌')),
        "focused role bar kept:\n{}",
        flines.join("\n")
    );

    // 3. Scroll under tight height works cleanly with Deco top rule.
    let tight = Rect::new(0, 0, 26, 7);
    let mut tbuf = Buffer::empty(tight);
    let mut thits = ShellHitMap::default();
    let mut tscroll = 1usize;
    let mut treveal = true;
    super::render_cast(
        &mut tbuf,
        tight,
        &focused,
        &cfg,
        &mut tscroll,
        &mut treveal,
        &mut thits,
    );
    assert_eq!(lines(&tbuf, tight)[0], "━━╡ ▸ CAST ╞━━━━━━━━━━━━━━");
    assert!(!thits.agents.is_empty());
    assert!(thits.agent_scroll_metrics.is_some());
    if std::env::var_os("BABYLON_LOT_DUMP").is_some() {
        eprintln!(
            "=== CAST UNFOCUSED ===\n{}\n=== CAST FOCUSED ===\n{}",
            ulines.join("\n"),
            flines.join("\n")
        );
    }

    // 4. Plain / `source = "herdr"` (`studio_chrome = false`): old `┄ closed (1) ┄` & plain `─`.
    cfg.studio_chrome = false;
    let mut hbuf = Buffer::empty(area);
    let mut hhits = ShellHitMap::default();
    super::render_cast(
        &mut hbuf,
        area,
        &unfocused,
        &cfg,
        &mut scroll,
        &mut reveal,
        &mut hhits,
    );
    let hlines = lines(&hbuf, area);
    assert_eq!(hlines[0], "─".repeat(26));
    assert!(hlines.iter().any(|l| l.contains("┄ closed (1) ┄")));
}
