//! The renderer: the five state marks and colors, the animating
//! working mark, the scrollbar on overflow and the hit map.
use super::*;
use crate::chrome::palette::Palette;
use crate::panel::rows::Mark;
use crate::panel::{Panel, PanelMode, mark_color, mark_symbol, render};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

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
