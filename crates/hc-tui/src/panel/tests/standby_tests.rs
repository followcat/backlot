//! Dock panel tests for standby fixed roles: dim rendering, blank
//! glyph, second hint line, and exclusion from mark counts.

use backlot_model::summary::Summary;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::*;
use crate::chrome::palette::Palette;
use crate::panel::{Panel, PanelMode, Target, mark_counts, render};

fn standby_summary(writer_state: &str, producer_state: &str) -> Summary {
    serde_json::from_value(serde_json::json!({
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
            "roles": [
                { "role": "producer", "name": "producer", "terminal": if producer_state == "live" { serde_json::json!("t1") } else { serde_json::Value::Null },
                  "state": producer_state, "runner": "claude", "model": "claude-opus-4-7" },
                { "role": "director", "name": "director", "terminal": "t2",
                  "state": "live", "work": "waiting", "runner": "claude", "model": "claude-opus-4-7" },
                { "role": "writer", "name": "writer",
                  "state": writer_state, "runner": "claude", "model": "claude-sonnet-4-5" },
                { "role": "actor-alice", "name": "alice", "terminal": "t4",
                  "runner": "agent-a", "model": "m", "state": "live",
                  "work": "working", "task": "task-1", "priority": "normal" }
            ]
        }]
    }))
    .unwrap()
}

fn draw_panel(summary: &Summary, width: u16, height: u16) -> (Buffer, crate::panel::PanelRender) {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    let panel = Panel {
        mode: PanelMode::Expanded,
        ..Panel::new()
    };
    let palette = Palette::from_name("catppuccin").unwrap();
    let hits = render(
        &mut buf,
        area,
        &machines(Some(summary)),
        |_| true,
        |_| true,
        &panel,
        panel.mode,
        Some(&crate::studio::StudioKey::local("acme-shop")),
        Some(&crate::studio::StudioKey::local("acme-shop")),
        2,
        None,
        &Default::default(),
        None,
        false,
        false,
        true,
        &Default::default(),
        &palette,
        1,
    );
    (buf, hits)
}

fn line_at(buf: &Buffer, area: Rect, y: u16) -> String {
    (area.x..area.x + area.width)
        .map(|x| buf[(x, y)].symbol())
        .collect()
}

#[test]
fn standby_row_in_dock_renders_dim_hint_line_and_no_counts() {
    let summary = standby_summary("standby", "live");
    let counts = mark_counts(&summary.studios[0]);
    let total_marked: usize = counts.iter().map(|(_, n)| *n).sum();
    // producer (ready) + director (ready) + alice (working) = 3; writer standby ignored.
    assert_eq!(total_marked, 3, "standby ignored in counts: {counts:?}");

    let palette = Palette::from_name("catppuccin").unwrap();
    let area = Rect::new(0, 0, 44, 30);
    let (buf, hits) = draw_panel(&summary, area.width, area.height);
    let writer_rect = hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Person(_, r) if r == "writer"))
        .map(|(r, _)| *r)
        .expect("writer row in dock");

    let row1 = line_at(&buf, area, writer_rect.y);
    assert!(row1.contains("writer"), "{row1}");
    assert!(row1.trim_end().ends_with("standby│"), "{row1}");
    // No activity glyph at mark column (index 2 inside `│ <mark> ...`).
    assert_eq!(buf[(area.x + 2, writer_rect.y)].symbol(), " ");
    let name_x = row1.find("writer").unwrap() as u16;
    assert_eq!(buf[(area.x + name_x, writer_rect.y)].fg, palette.overlay0);
    let state_x = row1.find("standby").unwrap() as u16;
    assert_eq!(buf[(area.x + state_x, writer_rect.y)].fg, palette.overlay0);

    let row2 = line_at(&buf, area, writer_rect.y + 1);
    assert!(
        row2.contains("starts when the director plans"),
        "expected writer standby hint line below row, got: {row2}"
    );
    let hint_x = row2.find("starts").unwrap() as u16;
    assert_eq!(
        buf[(area.x + hint_x, writer_rect.y + 1)].fg,
        palette.overlay0
    );
}

#[test]
fn standby_producer_in_dock_renders_studio_start_hint() {
    let summary = standby_summary("standby", "standby");
    let area = Rect::new(0, 0, 44, 30);
    let (buf, hits) = draw_panel(&summary, area.width, area.height);
    let prod_rect = hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Person(_, r) if r == "producer"))
        .map(|(r, _)| *r)
        .expect("producer row in dock");
    let row2 = line_at(&buf, area, prod_rect.y + 1);
    assert!(
        row2.contains("starts with the studio"),
        "expected studio start hint, got: {row2}"
    );
}
