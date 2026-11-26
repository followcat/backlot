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
        current.as_ref(),
        2,
        None,
        &keys(),
        None,
        false,
        false,
        true,
        &Default::default(),
        &palette(),
        1,
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
        Person::NeedsYou,
        Person::Asks,
        Person::Stuck,
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
        let work_word = if width < 24 { "work" } else { "working" };
        assert!(text.contains(work_word), "w{width}: {text}");
        assert!(text.contains("Ⓐ task-1"), "w{width}: {text}");
        assert!(hits.working_drawn, "w{width}: a working mark was drawn");
        // S2 dropped role-group `├──┤` dividers; only the compose
        // `InputDivider` directly above `› tell producer` emits `├`.
        let divider_rows = (area.y..area.y + area.height)
            .filter(|&y| buf[(area.x + 1, y)].symbol() == "├")
            .count();
        assert_eq!(divider_rows, 1, "w{width}");
    }
}

#[test]
fn borders_are_honest_about_focus() {
    let summary = summary();
    let area = Rect::new(0, 0, 27, 30);
    let top_y = 1; // title row 0, the block's top border row 1
    let corner = (area.x + area.width - 1, top_y); // `╮`
    // Focused and selected: the block draws in the accent, the ▌ at
    // the block's left edge (`area.x + 1`, right of the dock `│` seam).
    let mut focused = panel(PanelMode::Expanded);
    focused.focused = true;
    focused.selected = Some(Target::Studio("acme-shop".into()));
    let (buf, _) = draw(area, Some(&summary), &focused, None);
    assert_eq!(buf[(area.x, top_y)].symbol(), "│");
    assert_eq!(buf[corner].fg, palette().accent);
    assert_eq!(buf[(area.x + 1, top_y)].symbol(), "▌");
    // Unfocused, the row cursor hides (`╭` is back, no `▌`), and the
    // unfocused studio frame demotes to `border_muted` (single focus ring).
    let mut unfocused = panel(PanelMode::Expanded);
    unfocused.selected = Some(Target::Studio("acme-shop".into()));
    let (buf, _) = draw(area, Some(&summary), &unfocused, Some("acme-shop"));
    assert_eq!(buf[corner].fg, palette().elevation().border_muted);
    assert_eq!(buf[(area.x + 1, top_y)].symbol(), "╭");
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
    let summary: Summary = serde_json::from_value(value.clone()).unwrap();
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
    let summary: Summary = serde_json::from_value(value.clone()).unwrap();
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
fn a_person_draws_one_line_with_role_badge_and_state() {
    let summary = summary();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 44, 30);
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
    assert!(line.contains("Ⓐ task-1"), "{line}");
    assert!(line.contains("working · writes code"), "{line}");
    // Neither runner nor model leaks onto regular lot rows.
    assert!(!line.contains("agent-a"), "{line}");
}

#[test]
fn lot_person_rows_use_coloured_role_badges() {
    let summary = summary();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 40, 30);
    let (buf, hits) = draw(area, Some(&summary), &panel, None);
    let palette = palette();
    let cases = [
        ("producer", "Ⓟ", palette.mauve),
        ("director", "Ⓓ", palette.blue),
        ("writer", "Ⓦ", palette.teal),
        ("actor-alice", "Ⓐ", palette.peach),
    ];
    for (role_key, badge, color) in cases {
        let rect = hits
            .rows
            .iter()
            .find(|(_, t)| matches!(t, Target::Person(_, r) if r == role_key))
            .map(|(rect, _)| *rect)
            .unwrap_or_else(|| panic!("{role_key} row"));
        let line = (area.x..area.x + area.width)
            .map(|x| buf[(x, rect.y)].symbol())
            .collect::<String>();
        assert!(line.contains(badge), "{role_key}: {line}");
        let x = line[..line.find(badge).unwrap()].chars().count() as u16;
        assert_eq!(buf[(area.x + x, rect.y)].fg, color, "{role_key}");
    }
}

/// Narrow rows keep the state word visible, switching `working` to
/// `work` below 24 cols so state never drops off the line.
#[test]
fn a_narrow_row_keeps_the_state_and_drops_runner() {
    let summary = summary();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 22, 30);
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
    assert!(line.contains("work"), "{line}");
    assert!(!line.contains("agent-a"), "{line}");
}

/// A long task keeps the cells available and never pushes state off.
#[test]
fn a_long_task_keeps_its_cells_and_omits_runner() {
    let mut value = summary_json();
    value["studios"][0]["roles"][3]["task"] = serde_json::json!("task-1234567890");
    let summary: Summary = serde_json::from_value(value).unwrap();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 37, 30);
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
    assert!(line.contains("Ⓐ task-1234567890"), "{line}");
    assert!(!line.contains("agent-a"), "{line}");
    assert!(line.contains("working"), "{line}");
}

/// S2: a newer install puts one dim line at the foot of the lot.
#[test]
fn a_newer_install_draws_the_restart_line_at_the_foot() {
    let summary = summary();
    let mut panel = panel(PanelMode::Expanded);
    panel.update = Some(crate::panel::UpdateHint {
        version: "0.3.1".into(),
    });
    let area = Rect::new(0, 0, 100, 30);
    let (buf, _) = draw(area, Some(&summary), &panel, None);
    let text = lines(&buf, area).join("\n");
    assert!(
        text.contains("backlot 0.3.1 installed — restart the client to update (q, then backlot)"),
        "{text}"
    );
}

#[test]
fn an_attention_renders_inline_on_the_person_row() {
    let mut value = summary_json();
    value["studios"][0]["roles"][3]["attention"] = serde_json::json!("needs permission");
    let summary: Summary = serde_json::from_value(value.clone()).unwrap();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 48, 30);
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
    assert!(line.contains("needs you · needs permission"), "{line}");
}

#[test]
fn a_failed_role_draws_failed_inline_when_no_attention() {
    let mut value = summary_json();
    value["studios"][0]["roles"][3]["failed"] = serde_json::json!("rate_limit");
    let summary: Summary = serde_json::from_value(value.clone()).unwrap();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 48, 30);
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
    assert!(line.contains("failed · rate_limit"), "{line}");
}

#[test]
fn short_height_stacks_other_blocks_to_single_lines() {
    // Two studios: the fixture plus a second one.
    let mut value = summary_json();
    let mut second = value["studios"][0].clone();
    second["id"] = serde_json::json!("second-studio");
    value["studios"].as_array_mut().unwrap().push(second);
    let summary: Summary = serde_json::from_value(value.clone()).unwrap();
    let panel = panel(PanelMode::Expanded);
    // Tall enough for one expanded block but not two → the
    // non-current studio draws its `╶` line with mark counts.
    let area = Rect::new(0, 0, 31, 14);
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
    // Two studios + a 2-entry cast (`cast_entries = 2` in `draw`)
    // light both nav hints (wide-01).
    let mut value = summary_json();
    let mut second = value["studios"][0].clone();
    second["id"] = serde_json::json!("second-studio");
    value["studios"].as_array_mut().unwrap().push(second);
    let two_studios: Summary = serde_json::from_value(value).unwrap();
    let unfocused = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 30, 20);
    let (buf, _) = draw(area, Some(&two_studios), &unfocused, None);
    let last = &lines(&buf, area)[area.height as usize - 1];
    assert!(last.contains("alt"), "{last}");
    assert!(last.contains("studio"), "{last}");
    assert!(last.contains("role"), "{last}");
    // One studio alone drops the `studio` hint — the key has
    // nowhere to go.
    let one = summary();
    let (buf, _) = draw(area, Some(&one), &unfocused, None);
    let last = &lines(&buf, area)[area.height as usize - 1];
    assert!(!last.contains("studio"), "{last}");
    assert!(last.contains("role"), "{last}");
    // Focused panel shows its own keys.
    let mut focused = panel(PanelMode::Expanded);
    focused.focused = true;
    let (buf, _) = draw(area, Some(&two_studios), &focused, None);
    let last = &lines(&buf, area)[area.height as usize - 1];
    assert!(last.contains('m'), "{last}");
    assert!(last.contains('?'), "{last}");
    // Under 8 rows the line is dropped.
    let area = Rect::new(0, 0, 30, 7);
    let (buf, _) = draw(area, Some(&two_studios), &unfocused, None);
    let text = lines(&buf, area).join("\n");
    assert!(!text.contains("studio  ←"), "{text}");
    assert!(!text.contains("m menu"), "{text}");
}

#[test]
fn an_expanded_block_draws_its_input_line() {
    let summary = summary();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 30, 30);
    let (buf, hits) = draw(area, Some(&summary), &panel, Some("acme-shop"));
    let text = lines(&buf, area).join("\n");
    // The `›` line sits inside the block's frame and its empty
    // placeholder teaches both channels (tell · /verbs).
    assert!(text.contains('›'), "{text}");
    assert!(text.contains("tell producer"), "{text}");
    assert!(
        hits.rows
            .iter()
            .any(|(_, t)| matches!(t, Target::Input(id) if id.id == "acme-shop")),
        "the `›` line is a hit target"
    );
}

#[test]
fn collapsed_strip_draws_health_id_and_attention_without_single_machine_suffix() {
    let summary = summary();
    let panel = panel(PanelMode::Collapsed);
    let area = Rect::new(0, 0, 16, 12);
    let (buf, hits) = draw(area, Some(&summary), &panel, None);
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("lot"), "{text}");
    assert!(text.contains("● acme-shop"), "{text}");
    assert!(!text.contains("·loc"), "single-machine omits ·loc: {text}");
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
    // Single-machine omits ·loc so `acme-shop  !1` fits cleanly at width 16;
    // narrower strips keep `!1` and truncate `id` with `…`.
    let mut value = summary_json();
    value["studios"][0]["roles"][3]["attention"] = serde_json::json!("needs permission");
    value["studios"][0]["waiting"] = serde_json::json!(1);
    let summary: Summary = serde_json::from_value(value.clone()).unwrap();
    let (buf, _) = draw(area, Some(&summary), &panel, None);
    let strip = lines(&buf, area)
        .iter()
        .find(|l| l.contains("acme"))
        .cloned()
        .unwrap();
    assert!(strip.contains("!1"), "{strip}");
    assert!(
        !strip.contains("·loc"),
        "single-machine omits ·loc: {strip}"
    );

    let narrow = Rect::new(0, 0, 13, 12);
    let (nbuf, _) = draw(narrow, Some(&summary), &panel, None);
    let nstrip = lines(&nbuf, narrow)
        .iter()
        .find(|l| l.contains("acme"))
        .cloned()
        .unwrap();
    assert!(nstrip.contains("!1"), "{nstrip}");
    assert!(nstrip.contains('…'), "{nstrip}");
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
        0,
        None,
        &keys(),
        None,
        false,
        false,
        true,
        &Default::default(),
        &palette(),
        1,
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
        0,
        None,
        &keys(),
        None,
        false,
        false,
        true,
        &Default::default(),
        &palette(),
        1,
    );
    let text = lines(&buf, area).join("\n");
    // The stage-down word is the block's owed-step line (S2 §2).
    assert!(text.contains("stage stopped · s start"), "{text}");
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
        None,
        0,
        Some(crate::project::LOCAL_LABEL),
        &keys(),
        None,
        false,
        false,
        true,
        &Default::default(),
        &palette(),
        1,
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
        None,
        0,
        Some(crate::project::LOCAL_LABEL),
        &keys(),
        None,
        false,
        false,
        true,
        &Default::default(),
        &palette(),
        1,
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
    let summary: Summary = serde_json::from_value(value.clone()).unwrap();
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

#[test]
fn idle_collapsed_strip_renders_calm_counts_and_owed_cues() {
    let value = serde_json::json!({
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
            "tasks_done": 1,
            "tasks_total": 5,
            "roles": [
                { "role": "producer", "name": "producer", "terminal": "t1", "state": "live", "work": "waiting" },
                { "role": "director", "name": "director", "terminal": "t2", "state": "live", "work": "waiting" },
                { "role": "actor-a", "name": "alice", "state": "closed", "resumable": true }
            ]
        }]
    });
    let summary: Summary = serde_json::from_value(value.clone()).unwrap();
    let panel = panel(PanelMode::Collapsed);
    let area = Rect::new(0, 0, 16, 12);
    let (buf, _) = draw(area, Some(&summary), &panel, None);
    let text = lines(&buf, area).join("\n");
    assert!(
        text.contains("● acme-shop 1/5"),
        "1-line calm strip: {text}"
    );
    assert!(
        !text.contains("○2 ✓1"),
        "calm strip is strictly 1 line: {text}"
    );

    // Ready-to-finish renders `✓ acme-shop 1/5` on 1 line as well.
    let mut ready_val = value.clone();
    ready_val["studios"][0]["ready_to_finish"] = serde_json::json!(true);
    let ready_summary: Summary = serde_json::from_value(ready_val).unwrap();
    let (buf, _) = draw(area, Some(&ready_summary), &panel, None);
    let ready_text = lines(&buf, area).join("\n");
    assert!(ready_text.contains("✓ acme-shop 1/5"), "{ready_text}");

    // Stage down renders a concise stage cue instead of blank space.
    let mut down_buf = Buffer::empty(area);
    render(
        &mut down_buf,
        area,
        &machines(Some(&summary)),
        |_| false,
        |_| true,
        &panel,
        PanelMode::Collapsed,
        None,
        None,
        0,
        None,
        &keys(),
        None,
        false,
        false,
        true,
        &Default::default(),
        &palette(),
        1,
    );
    let down_text = lines(&down_buf, area).join("\n");
    assert!(down_text.contains("◌ stage down"), "{down_text}");

    // Ready to finish renders compact `✓ acme-shop 1/5` on 1 line as well.
    let mut ready_val = value;
    ready_val["studios"][0]["ready_to_finish"] = serde_json::json!(true);
    let ready_summary: Summary = serde_json::from_value(ready_val).unwrap();
    let (buf, _) = draw(area, Some(&ready_summary), &panel, None);
    let ready_text = lines(&buf, area).join("\n");
    assert!(ready_text.contains("✓ acme-shop 1/5"), "{ready_text}");
    assert!(!ready_text.contains("✓ ready"), "{ready_text}");
}

#[test]
fn inactive_dock_surface_suppresses_selection_bar_and_highlight() {
    let summary = summary();
    let mut panel = panel(PanelMode::Expanded);
    panel.focused = true;
    panel.selected = Some(Target::Person("acme-shop".into(), "actor-alice".into()));
    let area = Rect::new(0, 0, 36, 20);
    let palette = palette();

    let mut active_buf = Buffer::empty(area);
    let active_hits = render(
        &mut active_buf,
        area,
        &machines(Some(&summary)),
        |_| true,
        |_| true,
        &panel,
        PanelMode::Expanded,
        None,
        None,
        0,
        None,
        &keys(),
        None,
        false,
        false,
        true,
        &Default::default(),
        &palette,
        1,
    );
    let sel_y = active_hits
        .rows
        .iter()
        .find(|(_, t)| *t == Target::Person("acme-shop".into(), "actor-alice".into()))
        .unwrap()
        .0
        .y;
    assert_eq!(active_buf[(2, sel_y)].symbol(), "▌");
    assert_eq!(active_buf[(4, sel_y)].bg, palette.selection_bg);

    let mut suppressed_buf = Buffer::empty(area);
    render(
        &mut suppressed_buf,
        area,
        &machines(Some(&summary)),
        |_| true,
        |_| true,
        &panel,
        PanelMode::Expanded,
        None,
        None,
        0,
        None,
        &keys(),
        None,
        false,
        false,
        false,
        &Default::default(),
        &palette,
        1,
    );
    assert_ne!(suppressed_buf[(2, sel_y)].symbol(), "▌");
    assert_ne!(suppressed_buf[(4, sel_y)].bg, palette.selection_bg);
}

/// R3-M6: `▸` belongs to a studio workspace's studio only — the
/// renderer takes `mark` separately from `current` so a remembered
/// studio never paints as active over `backlot`/`stage` or a plain
/// workspace. The block itself is unaffected.
#[test]
fn the_mark_paints_only_where_a_studio_workspace_points() {
    let summary = summary();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 30, 20);
    let key = crate::studio::StudioKey::local("acme-shop");
    for (mark, want) in [(Some(&key), true), (None, false)] {
        let mut buf = Buffer::empty(area);
        render(
            &mut buf,
            area,
            &machines(Some(&summary)),
            |_| true,
            |_| true,
            &panel,
            panel.mode,
            Some(&key),
            mark,
            2,
            None,
            &keys(),
            None,
            false,
            false,
            true,
            &Default::default(),
            &palette(),
            1,
        );
        let text = lines(&buf, area).join("\n");
        assert_eq!(text.contains('▸'), want, "mark {mark:?}: {text}");
        assert!(text.contains("acme-shop"), "block stays: {text}");
    }
}

/// wide-01: `↑↓ role` only has somewhere to go while the focused
/// studio cycles ≥2 cast entries — a solo cast drops the hint.
#[test]
fn a_solo_cast_drops_the_role_hint() {
    let summary = summary();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 30, 20);
    for (cast_entries, want) in [(2usize, true), (0, false)] {
        let mut buf = Buffer::empty(area);
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
            cast_entries,
            None,
            &keys(),
            None,
            false,
            false,
            true,
            &Default::default(),
            &palette(),
            1,
        );
        let last = &lines(&buf, area)[area.height as usize - 1];
        assert_eq!(last.contains("↑↓ role"), want, "{cast_entries}: {last}");
    }
}

/// wide-01: the unfocused footer's `alt ←→ studio` names a navigation
/// that exists — a lone studio drops the half, two studios keep it.
#[test]
fn a_lone_studio_drops_the_studio_hint() {
    let summary = summary();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 30, 20);
    let two = || {
        vec![
            remote(&summary),
            crate::panel::data::MachineSummary {
                machine: None,
                summary: summary.clone(),
            },
        ]
    };
    for (list, want) in [(machines(Some(&summary)), false), (two(), true)] {
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
            None,
            0,
            None,
            &keys(),
            None,
            false,
            false,
            true,
            &Default::default(),
            &palette(),
            1,
        );
        let last = &lines(&buf, area)[area.height as usize - 1];
        assert_eq!(last.contains("alt ←→ studio"), want, "{last}");
    }
    let mut buf = Buffer::empty(area);
    render(
        &mut buf,
        area,
        &two(),
        |_| true,
        |_| true,
        &panel,
        panel.mode,
        None,
        None,
        2,
        None,
        &keys(),
        None,
        false,
        false,
        true,
        &Default::default(),
        &palette(),
        1,
    );
    let last = &lines(&buf, area)[area.height as usize - 1];
    assert_eq!(last, "│ alt ←→ studio  ↑↓ role");
}

#[test]
fn dock_compose_uses_raised_field_top_rule_and_two_tone_footer_hints() {
    let summary = summary();
    let area = Rect::new(0, 0, 36, 22);
    let pal = palette();
    let elev = pal.elevation();

    // Focused + composing with empty draft: `├──┤` divider directly above `›`,
    // `raised_field` interior, `▌` in accent, italic muted placeholder, and cursor on `'t'`.
    let mut p = panel(PanelMode::Expanded);
    p.focused = true;
    p.editing = true;
    p.selected = Some(Target::Input("acme-shop".into()));
    let (buf, hits) = draw(area, Some(&summary), &p, Some("acme-shop"));
    let input_rect = hits
        .rows
        .iter()
        .find(|(_, t)| *t == Target::Input("acme-shop".into()))
        .expect("input hit")
        .0;
    assert_eq!(buf[(input_rect.x, input_rect.y - 1)].symbol(), "├");
    assert_eq!(
        buf[(input_rect.x + 2, input_rect.y - 1)].fg,
        elev.border_muted
    );
    assert_eq!(buf[(input_rect.x + 1, input_rect.y)].symbol(), "▌");
    assert_eq!(buf[(input_rect.x + 1, input_rect.y)].fg, pal.accent);
    assert_eq!(buf[(input_rect.x + 4, input_rect.y)].bg, elev.raised_field);
    assert_eq!(buf[(input_rect.x + 4, input_rect.y)].fg, elev.muted_raised);
    assert!(
        buf[(input_rect.x + 4, input_rect.y)]
            .modifier
            .contains(ratatui::style::Modifier::ITALIC)
    );
    assert!(hits.cursor.is_some());

    // Footer hint row splits keys (`subtext0`) and labels (`muted_surface`) on `surface`.
    let fy = area.y + area.height - 1;
    let footer: String = (area.x..area.x + area.width)
        .map(|x| buf[(x, fy)].symbol())
        .collect();
    if let Some(pos) = footer.find("esc") {
        let cell_x = area.x + crate::chrome::text::display_width(&footer[..pos]) as u16;
        assert_eq!(buf[(cell_x, fy)].fg, pal.subtext0);
        assert_eq!(buf[(cell_x, fy)].bg, elev.surface);
    }
}

/// R3-N2: a director-blocked person is not just its task id in the
/// 16-column strip — the `dir` cue rides beside it.
#[test]
fn a_director_blocked_role_keeps_its_dir_cue_in_the_strip() {
    let summary = summary();
    let panel = panel(PanelMode::Collapsed);
    let area = Rect::new(0, 0, 16, 20);
    let (buf, _) = draw(area, Some(&summary), &panel, None);
    let text = lines(&buf, area).join("\n");
    // bob: blocked on `task-3` — `task-3 dir`, not bare `task-3`.
    assert!(text.contains("dir"), "{text}");
}

#[test]
fn stopped_ledger_renders_start_untruncated_at_38_cols() {
    let mut panel = panel(PanelMode::Expanded);
    panel.stopped = Some(crate::panel::StoppedLedger {
        count: 2,
        machine: None,
    });
    for width in [38u16, 36] {
        let area = Rect::new(0, 0, width, 20);
        let (buf, _) = draw(area, None, &panel, None);
        let text = lines(&buf, area).join("\n");
        let row = text
            .lines()
            .find(|line| line.contains("stopped stages"))
            .unwrap_or_else(|| panic!("missing stopped ledger row at {width}:\n{text}"));
        assert!(row.contains("start"), "at {width} cols: {row:?}");
        assert!(!row.contains('…'), "no ellipsis at {width} cols: {row:?}");
    }
    let wide_area = Rect::new(0, 0, 42, 20);
    let (wide_buf, _) = draw(wide_area, None, &panel, None);
    let wide = lines(&wide_buf, wide_area).join("\n");
    assert!(
        wide.contains("2 studios on stopped stages — s to start"),
        "full form when width fits: {wide}"
    );
}
