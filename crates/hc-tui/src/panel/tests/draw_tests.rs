//! The U1 renderer: bordered studio blocks, `studios n` title,
//! attention count on the frame, one-line people, stacking lines,
//! the shortcuts row and the collapsed strip.
use super::*;
use crate::chrome::palette::Palette;
use crate::panel::config::StudioKeys;
use crate::panel::{Panel, PanelMode, Target, render};
use crate::studio::glyphs::Person;
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
        |_| true,
        panel,
        panel.mode,
        current.as_ref(),
        None,
        &keys(),
        None,
        false,
        false,
        &Default::default(),
        &palette(),
    );
    (buf, hits)
}

#[test]
fn every_state_has_its_own_mark_and_color() {
    let palette = palette();
    // States that share a glyph — the two `×` prompts, the `!`
    // stall/fail/lost, the two `✓` closed — differ by colour; the
    // rest differ by glyph (interaction spec §1).
    let marks = [
        Person::Working,
        Person::Idle,
        Person::Slot,
        Person::UserPrompt,
        Person::DirPrompt,
        Person::Stalled,
        Person::Failed,
        Person::Resumable,
        Person::Closed,
    ];
    let styles: Vec<_> = marks
        .iter()
        .map(|m| (m.glyph(0), m.color(&palette)))
        .collect();
    for (i, a) in styles.iter().enumerate() {
        for b in &styles[i + 1..] {
            assert_ne!(a, b, "every state reads differently");
        }
    }
}

#[test]
fn the_working_mark_changes_between_frames() {
    let mut seen = std::collections::BTreeSet::new();
    for spin in 0..20 {
        seen.insert(Person::Working.glyph(spin));
    }
    assert!(seen.len() > 1, "the working mark animates");
    for mark in [
        Person::Idle,
        Person::DirPrompt,
        Person::Lost,
        Person::Closed,
    ] {
        assert_eq!(mark.glyph(0), mark.glyph(7));
    }
}

#[test]
fn expanded_block_draws_frame_people_and_state_words() {
    let summary = summary();
    let panel = panel(PanelMode::Expanded);
    for width in [26u16, 22, 40] {
        let area = Rect::new(0, 0, width, 30);
        let (buf, hits) = draw(area, Some(&summary), &panel, Some("acme-shop"));
        let text = lines(&buf, area).join("\n");
        assert!(text.contains("lot · 1"), "w{width}: {text}");
        assert!(text.contains("╭"), "w{width}: {text}");
        assert!(text.contains("╰"), "w{width}: {text}");
        // The id truncates with `…` at w22 — the spec's ellipsis
        // rule, not a clip.
        assert!(text.contains("acme"), "w{width}: {text}");
        assert!(text.contains("working"), "w{width}: {text}");
        assert!(text.contains("alice"), "w{width}: {text}");
        assert!(hits.working_drawn, "w{width}: a working mark was drawn");
        // S2 dropped the `├──┤` group dividers — the people list is
        // one run under the owed subheader.
        let divider_rows = (area.y..area.y + area.height)
            .filter(|&y| buf[(area.x, y)].symbol() == "├")
            .count();
        assert_eq!(divider_rows, 0, "w{width}");
    }
}

#[test]
fn borders_are_honest_about_focus() {
    let summary = summary();
    let area = Rect::new(0, 0, 26, 30);
    let top_y = 1; // title row 0, the block's top border row 1
    let corner = (area.x + area.width - 1, top_y); // `╮`, past the ▌
    // Focused and selected: the block draws in the accent, the ▌ at
    // the row's left edge.
    let mut focused = panel(PanelMode::Expanded);
    focused.focused = true;
    focused.selected = Some(Target::Studio("acme-shop".into()));
    let (buf, _) = draw(area, Some(&summary), &focused, None);
    assert_eq!(buf[corner].fg, palette().accent);
    assert_eq!(buf[(area.x, top_y)].symbol(), "▌");
    // Unfocused, the same selection still draws `surface1` (A2.3) —
    // the focused-terminal studio's mark is the `▸`, not the border,
    // and no part of the selection shows: the `╭` is back, no `▌`.
    let mut unfocused = panel(PanelMode::Expanded);
    unfocused.selected = Some(Target::Studio("acme-shop".into()));
    let (buf, _) = draw(area, Some(&summary), &unfocused, Some("acme-shop"));
    assert_eq!(buf[corner].fg, palette().surface1);
    assert_eq!(buf[(area.x, top_y)].symbol(), "╭");
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("▸"), "the focused studio's ▸ mark: {text}");
    assert!(!text.contains('▌'), "unfocused draws no selection: {text}");
}

#[test]
fn the_waiting_count_shows_only_above_zero() {
    let summary = summary();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 26, 30);
    let (buf, _) = draw(area, Some(&summary), &panel, None);
    assert!(
        !lines(&buf, area)[1].contains('!'),
        "waiting 0 → no bang on the border"
    );
    let mut value = summary_json();
    value["studios"][0]["waiting"] = serde_json::json!(2);
    let summary: Summary = serde_json::from_value(value).unwrap();
    let (buf, _) = draw(area, Some(&summary), &panel, None);
    let top = &lines(&buf, area)[1];
    assert!(top.contains("!2"), "{top}");
}

/// P4a: the block's usage line appears only while `usage` is
/// `Some` — a studio that reported none draws nothing, and the
/// line truncates at a narrow dock like the owed row.
#[test]
fn the_usage_line_shows_only_with_figures() {
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 40, 30);
    // Absent — the fixture carries no usage.
    let summary = summary();
    let (buf, _) = draw(area, Some(&summary), &panel, None);
    let text = lines(&buf, area).join("\n");
    assert!(!text.contains("tok"), "no usage line: {text}");

    let mut value = summary_json();
    value["studios"][0]["usage"] = serde_json::json!({
        "input_tokens": 12_400,
        "output_tokens": 3_100,
        "cost_micros_usd": 80_000
    });
    let summary: Summary = serde_json::from_value(value).unwrap();
    let (buf, _) = draw(area, Some(&summary), &panel, None);
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("12.4k/3.1k tok · $0.08"), "{text}");

    // Narrow: the line truncates inside the block edge.
    let area = Rect::new(0, 0, 22, 30);
    let (buf, _) = draw(area, Some(&summary), &panel, None);
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("12.4k"), "{text}");
    assert!(!text.contains("$0.08"), "cost elides at w22: {text}");
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
    assert!(line.contains("alice · task-1"), "{line}");
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
    assert!(last.contains("m menu"), "{last}");
    // Under 8 rows the line is dropped.
    let area = Rect::new(0, 0, 30, 7);
    let (buf, _) = draw(area, Some(&summary), &unfocused, None);
    let text = lines(&buf, area).join("\n");
    assert!(!text.contains("studio  ←"), "{text}");
    assert!(!text.contains("m menu"), "{text}");
}

#[test]
fn collapsed_strip_draws_health_id_machine_and_attention() {
    let summary = summary();
    let panel = panel(PanelMode::Collapsed);
    let area = Rect::new(0, 0, 16, 12);
    let (buf, hits) = draw(area, Some(&summary), &panel, None);
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("lot"), "{text}");
    // `● acme-shop·loc` — health, the id and the machine's three
    // chars; no tasks and no waiting still say something (A4.3).
    assert!(text.contains("acme-shop·loc"), "{text}");
    assert!(text.contains('●'), "{text}");
    assert!(
        hits.rows
            .iter()
            .any(|(_, t)| matches!(t, Target::Strip(id) if id == "acme-shop")),
        "strip hit"
    );
    let strip = lines(&buf, area)
        .iter()
        .find(|l| l.contains("acme-shop"))
        .cloned()
        .unwrap();
    assert!(!strip.contains('!'), "{strip}");
    // With attention `!n` joins the tail.
    let mut value = summary_json();
    value["studios"][0]["roles"][3]["attention"] = serde_json::json!("needs permission");
    value["studios"][0]["waiting"] = serde_json::json!(1);
    let summary: Summary = serde_json::from_value(value).unwrap();
    let (buf, _) = draw(area, Some(&summary), &panel, None);
    // `!1` takes tail room, so the id truncates to `acme-…·loc`.
    let strip = lines(&buf, area)
        .iter()
        .find(|l| l.contains("·loc"))
        .cloned()
        .unwrap();
    assert!(strip.contains("!1"), "{strip}");
    assert!(strip.contains('…'), "{strip}");
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
        |_| true,
        &panel,
        panel.mode,
        None,
        None,
        &keys(),
        None,
        false,
        false,
        &Default::default(),
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
    let area = Rect::new(0, 0, 30, 20);
    let mut buf = Buffer::empty(area);
    render(
        &mut buf,
        area,
        &machines(Some(&summary)),
        |_| false,
        |_| true,
        &panel,
        panel.mode,
        None,
        None,
        &keys(),
        None,
        false,
        false,
        &Default::default(),
        &palette(),
    );
    let text = lines(&buf, area).join("\n");
    // The stage-down word is the block's owed-step line (S2 §2).
    assert!(text.contains("stage not running"), "{text}");
    assert!(text.contains("acme-shop"), "{text}");
}

#[test]
fn a_selected_row_is_highlighted_inside_its_borders() {
    let summary = summary();
    let palette = palette();
    let mut panel = panel(PanelMode::Expanded);
    panel.focused = true;
    panel.selected = Some(Target::Person("acme-shop".into(), "actor-alice".into()));
    let area = Rect::new(0, 0, 30, 20);
    let (buf, hits) = draw(area, Some(&summary), &panel, None);
    let alice = hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Person(_, r) if r == "actor-alice"))
        .map(|(rect, _)| *rect)
        .expect("alice row hit");
    // The `▌` and the highlight sit inside the block's `│`s: the two
    // border cells keep the border glyph on the panel background.
    assert_eq!(buf[(alice.x + 1, alice.y)].symbol(), "▌");
    assert_eq!(buf[(alice.x + 2, alice.y)].bg, palette.selection_bg);
    for edge in [alice.x, alice.x + alice.width - 1] {
        let cell = &buf[(edge, alice.y)];
        assert_eq!(cell.symbol(), "│", "the border glyph stays");
        assert_ne!(
            cell.bg, palette.selection_bg,
            "the border keeps its own look"
        );
    }
}

#[test]
fn an_unfocused_panel_draws_no_selection_but_keeps_the_row() {
    let summary = summary();
    let palette = palette();
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
    for x in alice.x..alice.x + alice.width {
        let cell = &buf[(x, alice.y)];
        assert_ne!(cell.bg, palette.selection_bg, "no highlight unfocused");
        assert_ne!(cell.symbol(), "▌", "no cursor unfocused");
    }
    // The remembered row survives — focusing again restores it.
    assert_eq!(
        panel.selected,
        Some(Target::Person("acme-shop".into(), "actor-alice".into()))
    );
}

#[test]
fn studios_spanning_machines_name_their_machine() {
    // Two machines each serve `acme-shop`: this core reads
    // `acme-shop · Local` and `acme-shop · host-b` — local first.
    let summary = summary();
    let mut list = machines(Some(&summary));
    list.push(remote(&summary));
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 40, 40);
    let mut buf = Buffer::empty(area);
    render(
        &mut buf,
        area,
        &list,
        |_| true,
        |_| true,
        &panel,
        panel.mode,
        None,
        Some(crate::project::LOCAL_LABEL),
        &keys(),
        None,
        false,
        false,
        &Default::default(),
        &palette(),
    );
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("acme-shop · Local"), "{text}");
    assert!(text.contains("acme-shop · host-b"), "{text}");
    // The title's count spans machines too.
    assert!(text.contains("lot · 2"), "{text}");
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
        |_| true,
        &panel,
        panel.mode,
        None,
        Some(crate::project::LOCAL_LABEL),
        &keys(),
        None,
        false,
        false,
        &Default::default(),
        &palette(),
    );
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("acme-shop"), "{text}");
    assert!(!text.contains("acme-shop ·"), "{text}");
    assert!(text.contains("lot · 1"), "{text}");
}

/// R14: an experimental studio's block header carries `exp` after
/// the id; a standard studio's header is exactly what it was.
#[test]
fn an_experimental_studio_flags_its_header() {
    let mut value = summary_json();
    value["studios"][0]["mode"] = serde_json::json!("experimental");
    let summary: Summary = serde_json::from_value(value).unwrap();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 30, 30);
    let (buf, _) = draw(area, Some(&summary), &panel, Some("acme-shop"));
    let top = &lines(&buf, area)[1];
    assert!(top.contains("acme-shop exp"), "{top}");

    let standard = super::summary();
    let (buf, _) = draw(area, Some(&standard), &panel, Some("acme-shop"));
    let top = &lines(&buf, area)[1];
    assert!(top.contains("acme-shop"), "{top}");
    assert!(!top.contains("acme-shop exp"), "{top}");
}
