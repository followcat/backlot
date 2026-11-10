//! The renderer: the five state marks and colors, the animating
//! working mark, the scrollbar on overflow and the hit map.
use super::*;
use crate::chrome::palette::Palette;
use crate::panel::rows::Mark;
use crate::panel::{Panel, PanelMode, mark_color, mark_symbol, render};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

fn palette() -> Palette {
    Palette::from_name("catppuccin").unwrap()
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
    // The other marks never move.
    for mark in [Mark::Waiting, Mark::Blocked, Mark::Lost, Mark::Closed] {
        assert_eq!(mark_symbol(mark, 0), mark_symbol(mark, 7));
    }
}

#[test]
fn expanded_draw_shows_groups_people_and_their_lines() {
    let summary = summary();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 30, 30);
    let mut buf = Buffer::empty(area);
    let hits = render(&mut buf, area, Some(&summary), true, &panel, &palette());
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("working: producer, actors 2"), "{text}");
    assert!(text.contains("acme-shop"), "{text}");
    assert!(text.contains("▾ actors 5"), "{text}");
    assert!(text.contains("alice"), "{text}");
    assert!(text.contains("writes code"), "{text}");
    assert!(text.contains("agent-a/m"), "{text}");
    assert!(hits.working_drawn, "a working mark was drawn");
    assert!(hits.scrollbar.is_empty(), "no overflow → no scrollbar");
    assert!(hits.title.height == 1);
}

/// The summary builds `task t1 · behind v2` for an actor whose task
/// trails the current requirement — the panel draws the `doing` line
/// verbatim, so the marker needs no panel code of its own.
#[test]
fn a_behind_actor_draws_its_doing_line() {
    let mut value = summary_json();
    value["studios"][0]["roles"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "role": "actor-t1-c1", "name": "actor-t1-c1", "terminal": "t9",
            "state": "live", "task": "t1", "doing": "task t1 · behind v2",
        }));
    let summary: Summary = serde_json::from_value(value).unwrap();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 40, 40);
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, Some(&summary), true, &panel, &palette());
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("task t1 · behind v2"), "{text}");
}

#[test]
fn collapsed_draw_lists_working_groups() {
    let summary = summary();
    let panel = panel(PanelMode::Collapsed);
    let area = Rect::new(0, 0, 16, 12);
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, Some(&summary), true, &panel, &palette());
    let text = lines(&buf, area).join("\n");
    // The title is cut to the 16-column strip.
    assert!(text.contains("working:"), "{text}");
    assert!(text.contains("producer"), "{text}");
    assert!(text.contains("actors 2"), "{text}");
    assert!(!text.contains("alice"), "{text}");
}

#[test]
fn an_overflowing_body_draws_a_scrollbar() {
    // Many actors → more rows than the body fits.
    let mut value = summary_json();
    let roles = value["studios"][0]["roles"].as_array_mut().unwrap();
    for i in 0..12 {
        roles.push(serde_json::json!({
            "role": format!("actor-extra-{i}"), "name": format!("extra-{i}"),
            "state": "live", "work": "waiting",
        }));
    }
    let summary: Summary = serde_json::from_value(value).unwrap();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 30, 10);
    let mut buf = Buffer::empty(area);
    let hits = render(&mut buf, area, Some(&summary), true, &panel, &palette());
    assert!(!hits.scrollbar.is_empty(), "overflow draws the track");
    let text = lines(&buf, area).join("\n");
    assert!(text.contains('▕'), "scrollbar cells: {text}");
}

#[test]
fn a_render_over_drawn_cells_leaves_no_symbol_behind() {
    // The frame underneath wrote text into the same rect — full-screen
    // at 40 columns is exactly this shape.
    let summary = summary();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 40, 20);
    let mut buf = Buffer::empty(area);
    for y in area.y..area.y + area.height {
        // Only glyphs the panel itself can never draw, so a leak is
        // unambiguous.
        buf.set_string(area.x, y, "│9│8│6│4", Style::default());
    }
    render(&mut buf, area, Some(&summary), true, &panel, &palette());
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            let symbol = buf[(x, y)].symbol();
            assert!(
                !"│9864".contains(symbol),
                "frame symbol {symbol:?} leaked at ({x},{y})"
            );
        }
    }
}

#[test]
fn a_wide_name_pushes_the_runner_past_its_display_width() {
    // "\u{738B}\u{4E94}" is two characters but four cells: the runner
    // text must start after the display width, not the char count.
    let mut value = summary_json();
    value["studios"][0]["roles"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "role": "actor-we", "name": "\u{738B}\u{4E94}", "state": "live",
            "work": "waiting", "runner": "agent-a",
        }));
    let summary: Summary = serde_json::from_value(value).unwrap();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 40, 30);
    let mut buf = Buffer::empty(area);
    let hits = render(&mut buf, area, Some(&summary), true, &panel, &palette());
    let y = hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, crate::panel::Target::Person(_, r) if r == "actor-we"))
        .map(|(rect, _)| rect.y)
        .expect("the actor row drew");
    // mark at 0, name at 2 covering four cells, runner's 'a' at 7 —
    // a char-count advance would put it at 5, over the name's tail.
    assert_eq!(buf[(7, y)].symbol(), "a");
}

/// A studio the stage flags with `error` draws it on its own line
/// under the studio row, the way a person's attention draws.
#[test]
fn a_studio_error_draws_a_bang_line_under_the_studio() {
    let mut value = summary_json();
    value["studios"][0]["error"] = serde_json::json!("ledger unreadable");
    let summary: Summary = serde_json::from_value(value).unwrap();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 40, 30);
    let mut buf = Buffer::empty(area);
    let hits = render(&mut buf, area, Some(&summary), true, &panel, &palette());
    let y = hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, crate::panel::Target::Studio(id) if id == "acme-shop"))
        .map(|(rect, _)| rect.y)
        .expect("the studio row drew");
    let line = (area.x..area.x + area.width)
        .map(|x| buf[(x, y + 1)].symbol())
        .collect::<String>();
    assert_eq!(line.trim(), "! ledger unreadable", "{line:?}");
}

/// A studio with an owed step draws it on its own line under the
/// studio row — after the `error` line when both stand.
#[test]
fn a_studio_next_draws_an_arrow_line_under_the_studio() {
    let mut value = summary_json();
    value["studios"][0]["next"] = serde_json::json!("dispatch — producer");
    let summary: Summary = serde_json::from_value(value).unwrap();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 40, 30);
    let mut buf = Buffer::empty(area);
    let hits = render(&mut buf, area, Some(&summary), true, &panel, &palette());
    let y = hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, crate::panel::Target::Studio(id) if id == "acme-shop"))
        .map(|(rect, _)| rect.y)
        .expect("the studio row drew");
    let line = (area.x..area.x + area.width)
        .map(|x| buf[(x, y + 1)].symbol())
        .collect::<String>();
    assert_eq!(line.trim(), "→ dispatch — producer", "{line:?}");

    // With an error too the `next` line sits one row lower.
    let mut value = summary_json();
    value["studios"][0]["error"] = serde_json::json!("ledger unreadable");
    value["studios"][0]["next"] = serde_json::json!("dispatch — producer");
    let summary: Summary = serde_json::from_value(value).unwrap();
    let mut buf = Buffer::empty(area);
    let hits = render(&mut buf, area, Some(&summary), true, &panel, &palette());
    let y = hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, crate::panel::Target::Studio(id) if id == "acme-shop"))
        .map(|(rect, _)| rect.y)
        .expect("the studio row drew");
    let lines: Vec<String> = (1..=2)
        .map(|dy| {
            (area.x..area.x + area.width)
                .map(|x| buf[(x, y + dy)].symbol())
                .collect::<String>()
        })
        .collect();
    assert_eq!(lines[0].trim(), "! ledger unreadable", "{lines:?}");
    assert_eq!(lines[1].trim(), "→ dispatch — producer", "{lines:?}");
}

#[test]
fn a_dead_stage_says_so_on_one_line_and_keeps_the_document() {
    let summary = summary();
    let panel = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 30, 10);
    let mut buf = Buffer::empty(area);
    render(&mut buf, area, Some(&summary), false, &panel, &palette());
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("stage not running"), "{text}");
    assert!(text.contains("acme-shop"), "{text}");
}

#[test]
fn a_selected_row_is_highlighted() {
    let summary = summary();
    let mut panel = panel(PanelMode::Expanded);
    panel.selected = Some(crate::panel::Target::Person(
        "acme-shop".into(),
        "actor-alice".into(),
    ));
    let area = Rect::new(0, 0, 30, 20);
    let mut buf = Buffer::empty(area);
    let hits = render(&mut buf, area, Some(&summary), true, &panel, &palette());
    let alice = hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, crate::panel::Target::Person(_, r) if r == "actor-alice"))
        .map(|(rect, _)| *rect)
        .expect("alice row hit");
    let cell = &buf[(alice.x, alice.y)];
    assert_eq!(cell.bg, palette().active_row_bg);
}
