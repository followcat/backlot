//! The U1 renderer: bordered studio blocks, `studios n` title,
//! attention count on the frame, one-line people, stacking lines,
//! the shortcuts row and the collapsed strip.
use super::*;
use crate::chrome::palette::Palette;
use crate::panel::config::StudioKeys;
use crate::panel::rows::Mark;
use crate::panel::{Panel, PanelMode, Target, mark_color, mark_symbol, render};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

fn palette() -> Palette {
    Palette::from_name("catppuccin").unwrap()
}

fn keys() -> StudioKeys {
    StudioKeys::default()
}

fn panel(mode: PanelMode) -> Panel {
    Panel {
        mode,
        ..Panel::new()
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

fn draw(
    area: Rect,
    summary: Option<&Summary>,
    panel: &Panel,
    current: Option<&str>,
) -> (Buffer, crate::panel::PanelRender) {
    let mut buf = Buffer::empty(area);
    let current = current.map(crate::studio::StudioKey::local);
    let hits = render(
        &mut buf,
        area,
        &machines(summary),
        |_| true,
        panel,
        current.as_ref(),
        None,
        &keys(),
        &palette(),
    );
    (buf, hits)
}

#[test]
fn every_state_has_its_own_mark_and_color() {
    let palette = palette();
    let marks = [
        Mark::Working,
        Mark::Waiting,
        Mark::Blocked,
        Mark::Lost,
        Mark::Closed,
    ];
    let symbols: Vec<_> = marks.iter().map(|m| mark_symbol(*m, 0)).collect();
    for (i, a) in symbols.iter().enumerate() {
        for b in &symbols[i + 1..] {
            assert_ne!(a, b, "marks must differ, not only colors");
        }
    }
    let colors: Vec<_> = marks.iter().map(|m| mark_color(*m, &palette)).collect();
    for (i, a) in colors.iter().enumerate() {
        for b in &colors[i + 1..] {
            assert_ne!(a, b, "each state gets its own color");
        }
    }
}

#[test]
fn the_working_mark_changes_between_frames() {
    let mut seen = std::collections::BTreeSet::new();
    for spin in 0..20 {
        seen.insert(mark_symbol(Mark::Working, spin));
    }
    assert!(seen.len() > 1, "the working mark animates");
    for mark in [Mark::Waiting, Mark::Blocked, Mark::Lost, Mark::Closed] {
        assert_eq!(mark_symbol(mark, 0), mark_symbol(mark, 7));
    }
}

#[test]
fn expanded_block_draws_frame_people_dividers_and_state_words() {
    let summary = summary();
    let panel = panel(PanelMode::Expanded);
    for width in [26u16, 22, 40] {
        let area = Rect::new(0, 0, width, 30);
        let (buf, hits) = draw(area, Some(&summary), &panel, Some("acme-shop"));
        let text = lines(&buf, area).join("\n");
        assert!(text.contains("studios 1"), "w{width}: {text}");
        assert!(text.contains("╭"), "w{width}: {text}");
        assert!(text.contains("╰"), "w{width}: {text}");
        assert!(text.contains("├"), "w{width}: {text}");
        assert!(text.contains("acme-shop"), "w{width}: {text}");
        assert!(text.contains("working"), "w{width}: {text}");
        assert!(text.contains("alice"), "w{width}: {text}");
        assert!(hits.working_drawn, "w{width}: a working mark was drawn");
        // Dividers only between groups: 4 groups → 3 `├` rows.
        let divider_rows = (area.y..area.y + area.height)
            .filter(|&y| buf[(area.x, y)].symbol() == "├")
            .count();
        assert_eq!(divider_rows, 3, "w{width}");
    }
}

#[test]
fn the_current_studio_frame_is_accent_others_are_muted() {
    let summary = summary();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 26, 30);
    let (buf, _) = draw(area, Some(&summary), &panel, Some("acme-shop"));
    let top_y = 1; // title row 0, the block's top border row 1
    assert_eq!(buf[(area.x, top_y)].fg, palette().accent);
    // Without a current studio the frame is muted surface.
    let (buf, _) = draw(area, Some(&summary), &panel, None);
    assert_eq!(buf[(area.x, top_y)].fg, palette().surface_dim);
}

#[test]
fn the_attention_count_shows_only_above_zero() {
    let summary = summary();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 26, 30);
    let (buf, _) = draw(area, Some(&summary), &panel, None);
    assert!(
        !lines(&buf, area)[1].contains('!'),
        "no attention → no bang on the border"
    );
    let mut value = summary_json();
    value["studios"][0]["roles"][3]["attention"] = serde_json::json!("needs permission");
    value["studios"][0]["roles"][4]["attention"] = serde_json::json!("needs permission");
    let summary: Summary = serde_json::from_value(value).unwrap();
    let (buf, _) = draw(area, Some(&summary), &panel, None);
    let top = &lines(&buf, area)[1];
    assert!(top.contains("!2"), "{top}");
}

#[test]
fn a_person_draws_one_line_with_the_state_word_right() {
    let summary = summary();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 40, 30);
    let (buf, hits) = draw(area, Some(&summary), &panel, None);
    let alice = hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Person(_, r) if r == "actor-alice"))
        .map(|(rect, _)| *rect)
        .expect("alice row hit");
    let line = (area.x..area.x + area.width)
        .map(|x| buf[(x, alice.y)].symbol())
        .collect::<String>();
    assert!(line.contains("alice task-1"), "{line}");
    assert!(line.trim_end().ends_with("working│"), "{line}");
    // No runner/model/doing on the person line.
    assert!(!line.contains("agent-a"), "{line}");
    assert!(!line.contains("writes code"), "{line}");
}

#[test]
fn an_attention_adds_a_bang_line_under_the_person() {
    let mut value = summary_json();
    value["studios"][0]["roles"][3]["attention"] = serde_json::json!("needs permission");
    let summary: Summary = serde_json::from_value(value).unwrap();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 40, 30);
    let (buf, hits) = draw(area, Some(&summary), &panel, None);
    let alice = hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Person(_, r) if r == "actor-alice"))
        .map(|(rect, _)| *rect)
        .expect("alice row hit");
    let line = (area.x..area.x + area.width)
        .map(|x| buf[(x, alice.y + 1)].symbol())
        .collect::<String>();
    assert!(line.contains("! needs permission"), "{line}");
}

#[test]
fn a_failed_role_draws_its_failed_line_when_no_attention() {
    let mut value = summary_json();
    value["studios"][0]["roles"][3]["failed"] = serde_json::json!("rate_limit");
    let summary: Summary = serde_json::from_value(value).unwrap();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 40, 30);
    let (buf, hits) = draw(area, Some(&summary), &panel, None);
    let alice = hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Person(_, r) if r == "actor-alice"))
        .map(|(rect, _)| *rect)
        .expect("alice row hit");
    let line = (area.x..area.x + area.width)
        .map(|x| buf[(x, alice.y + 1)].symbol())
        .collect::<String>();
    assert!(line.contains("! last turn failed: rate_limit"), "{line}");
}

#[test]
fn short_height_stacks_other_blocks_to_single_lines() {
    // Two studios: the fixture plus a second one.
    let mut value = summary_json();
    let mut second = value["studios"][0].clone();
    second["id"] = serde_json::json!("second-studio");
    value["studios"].as_array_mut().unwrap().push(second);
    let summary: Summary = serde_json::from_value(value).unwrap();
    let panel = panel(PanelMode::Expanded);
    // Tall enough for one expanded block but not two → the
    // non-current studio draws its `╶` line with mark counts.
    let area = Rect::new(0, 0, 30, 20);
    let (buf, hits) = draw(area, Some(&summary), &panel, Some("acme-shop"));
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("╶ second-studio"), "{text}");
    assert!(
        hits.rows
            .iter()
            .any(|(_, t)| matches!(t, Target::Block(id) if id == "second-studio")),
        "a single-line block hit"
    );
    // The stacked line carries mark counts.
    let stacked = lines(&buf, area)
        .iter()
        .find(|l| l.contains("╶ second-studio"))
        .cloned()
        .unwrap();
    assert!(stacked.contains("3"), "working count: {stacked}");
    // And at even shorter heights the body must scroll.
    let area = Rect::new(0, 0, 30, 6);
    let (_, hits) = draw(area, Some(&summary), &panel, Some("acme-shop"));
    assert!(
        hits.metrics.map(|m| m.max_offset_from_bottom).unwrap_or(0) > 0,
        "short body scrolls"
    );
}

#[test]
fn the_shortcuts_line_shows_keys_and_drops_under_eight_rows() {
    let summary = summary();
    let unfocused = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 30, 20);
    let (buf, _) = draw(area, Some(&summary), &unfocused, None);
    let last = &lines(&buf, area)[area.height as usize - 1];
    assert!(last.contains("alt"), "{last}");
    assert!(last.contains("studio"), "{last}");
    assert!(last.contains("role"), "{last}");
    // Focused panel shows its own keys.
    let mut focused = panel(PanelMode::Expanded);
    focused.focused = true;
    let (buf, _) = draw(area, Some(&summary), &focused, None);
    let last = &lines(&buf, area)[area.height as usize - 1];
    assert!(last.contains("⏎ open"), "{last}");
    // Under 8 rows the line is dropped.
    let area = Rect::new(0, 0, 30, 7);
    let (buf, _) = draw(area, Some(&summary), &unfocused, None);
    let text = lines(&buf, area).join("\n");
    assert!(!text.contains("studio  ←"), "{text}");
    assert!(!text.contains("⏎ open"), "{text}");
}

#[test]
fn collapsed_strip_draws_id_working_count_and_attention() {
    let summary = summary();
    let panel = panel(PanelMode::Collapsed);
    let area = Rect::new(0, 0, 16, 12);
    let (buf, hits) = draw(area, Some(&summary), &panel, None);
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("studios"), "{text}");
    assert!(text.contains("acme-shop"), "{text}");
    assert!(
        hits.rows
            .iter()
            .any(|(_, t)| matches!(t, Target::Strip(id) if id == "acme-shop")),
        "strip hit"
    );
    // `3` working, no `!` (no attention in the fixture).
    let strip = lines(&buf, area)
        .iter()
        .find(|l| l.contains("acme-shop"))
        .cloned()
        .unwrap();
    assert!(strip.trim_end().ends_with('3'), "{strip}");
    assert!(!strip.contains('!'), "{strip}");
    // With attention `!n` joins the tail.
    let mut value = summary_json();
    value["studios"][0]["roles"][3]["attention"] = serde_json::json!("needs permission");
    let summary: Summary = serde_json::from_value(value).unwrap();
    let (buf, _) = draw(area, Some(&summary), &panel, None);
    let strip = lines(&buf, area)
        .iter()
        .find(|l| l.contains("acme-shop"))
        .cloned()
        .unwrap();
    assert!(strip.contains("!1"), "{strip}");
}

#[test]
fn a_render_over_drawn_cells_leaves_no_symbol_behind() {
    let summary = summary();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 40, 20);
    let mut buf = Buffer::empty(area);
    for y in area.y..area.y + area.height {
        // `█` is a glyph the panel itself can never draw, so a leak
        // is unambiguous (`│` is a real border glyph now).
        buf.set_string(area.x, y, "█9█8█6█4", Style::default());
    }
    render(
        &mut buf,
        area,
        &machines(Some(&summary)),
        |_| true,
        &panel,
        None,
        None,
        &keys(),
        &palette(),
    );
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            let symbol = buf[(x, y)].symbol();
            assert!(
                !"█9864".contains(symbol),
                "frame symbol {symbol:?} leaked at ({x},{y})"
            );
        }
    }
}

#[test]
fn a_dead_stage_says_so_on_one_line_and_keeps_the_document() {
    let summary = summary();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 30, 10);
    let mut buf = Buffer::empty(area);
    render(
        &mut buf,
        area,
        &machines(Some(&summary)),
        |_| false,
        &panel,
        None,
        None,
        &keys(),
        &palette(),
    );
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("stage not running"), "{text}");
    assert!(text.contains("acme-shop"), "{text}");
}

#[test]
fn a_selected_row_is_highlighted() {
    let summary = summary();
    let mut panel = panel(PanelMode::Expanded);
    panel.selected = Some(Target::Person("acme-shop".into(), "actor-alice".into()));
    let area = Rect::new(0, 0, 30, 20);
    let (buf, hits) = draw(area, Some(&summary), &panel, None);
    let alice = hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Person(_, r) if r == "actor-alice"))
        .map(|(rect, _)| *rect)
        .expect("alice row hit");
    let cell = &buf[(alice.x, alice.y)];
    assert_eq!(cell.bg, palette().active_row_bg);
}

#[test]
fn studios_spanning_machines_name_their_machine() {
    // Two machines each serve `acme-shop`: with this core named
    // `here`, the headers read `acme-shop · here` and
    // `acme-shop · host-b` — local first.
    let summary = summary();
    let mut list = machines(Some(&summary));
    list.push(remote(&summary));
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 30, 40);
    let mut buf = Buffer::empty(area);
    render(
        &mut buf,
        area,
        &list,
        |_| true,
        &panel,
        None,
        Some("here"),
        &keys(),
        &palette(),
    );
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("acme-shop · here"), "{text}");
    assert!(text.contains("acme-shop · host-b"), "{text}");
    // The title's count spans machines too.
    assert!(text.contains("studios 2"), "{text}");
}

#[test]
fn one_machine_keeps_the_bare_id_headers() {
    // Two machines' documents, but only this core lists studios:
    // nothing on screen changes from before machines existed.
    let summary = summary();
    let mut list = machines(Some(&summary));
    let mut empty = summary.clone();
    empty.studios.clear();
    list.push(remote(&empty));
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 30, 20);
    let mut buf = Buffer::empty(area);
    render(
        &mut buf,
        area,
        &list,
        |_| true,
        &panel,
        None,
        Some("here"),
        &keys(),
        &palette(),
    );
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("acme-shop"), "{text}");
    assert!(!text.contains("·"), "{text}");
    assert!(text.contains("studios 1"), "{text}");
}
