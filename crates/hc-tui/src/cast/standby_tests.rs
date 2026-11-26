//! Cast sidebar tests for standby fixed roles.

use backlot_model::summary::Summary;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::rows::{CastRow, cast_rows};
use super::*;
use crate::chrome::palette::Palette;
use crate::chrome::render::ShellHitMap;

fn summary_with_standby(writer_state: &str, producer_state: &str) -> Summary {
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
                  "state": "live", "work": "working", "task": "t1", "runner": "agent-a", "model": "m1" }
            ]
        }]
    }))
    .unwrap()
}

fn draw_cast(summary: &Summary, width: u16, height: u16) -> Buffer {
    let view = CastView {
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
        focused_terminal: None,
        titles: Default::default(),
        shells: Vec::new(),
        others: Vec::new(),
        others_expanded: false,
        spin: 0,
        now_ms: 1,
        studio_focused: false,
        hint: false,
        stopped: false,
        server_stopped: false,
    };
    let cfg = ClientShellConfig::from_config(
        &crate::config::Config::default(),
        None,
        None,
        std::path::PathBuf::new(),
    );
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    let mut hits = ShellHitMap::default();
    let mut scroll = 0usize;
    let mut reveal = false;
    render_cast(
        &mut buf,
        area,
        &view,
        &cfg,
        &mut scroll,
        &mut reveal,
        &mut hits,
    );
    buf
}

fn row_text(buf: &Buffer, area: Rect, y: u16) -> String {
    (area.x..area.x + area.width)
        .map(|x| buf[(x, y)].symbol())
        .collect::<String>()
        .trim_end()
        .to_string()
}

#[test]
fn standby_writer_in_cast_renders_single_line_without_runner_or_plan_hint() {
    let summary = summary_with_standby("standby", "live");
    let palette = Palette::from_name("catppuccin").unwrap();
    let view = CastView {
        studio: Some(StudioRef {
            machine: None,
            studio: &summary.studios[0],
        }),
        label: None,
        index: 0,
        total: 1,
        focused_terminal: None,
        titles: Default::default(),
        shells: Vec::new(),
        others: Vec::new(),
        others_expanded: false,
        spin: 0,
        now_ms: 1,
        studio_focused: false,
        hint: false,
        stopped: false,
        server_stopped: false,
    };
    let rows_built = cast_rows(&view);
    assert!(rows_built.iter().all(|r| r.lines() == 1));
    let order: Vec<&str> = rows_built
        .into_iter()
        .filter_map(|r| match r {
            CastRow::Person { role, .. } => Some(role.role.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(order, vec!["producer", "director", "writer", "actor-alice"]);

    let area = Rect::new(0, 0, 50, 20);
    let buf = draw_cast(&summary, area.width, area.height);
    let rows: Vec<String> = (0..area.height).map(|y| row_text(&buf, area, y)).collect();
    let y = rows
        .iter()
        .position(|l| l.contains("Ⓦ"))
        .expect("writer row") as u16;
    assert_eq!(buf[(1, y)].symbol(), " ");
    let row = &rows[y as usize];
    let badge_x = row[..row.find("Ⓦ").unwrap()].chars().count() as u16;
    assert_eq!(buf[(badge_x, y)].fg, palette.teal);
    assert!(row.contains("standby"), "{:?}", row);
    let st_x = row[..row.find("standby").unwrap()].chars().count() as u16;
    assert_eq!(buf[(st_x, y)].fg, palette.overlay0);
    assert!(
        !rows
            .iter()
            .any(|l| l.contains("starts when the director plans")),
        "standby hint moved out of regular cast rows: {rows:?}"
    );
}

#[test]
fn standby_producer_in_cast_renders_single_line_standby() {
    let summary = summary_with_standby("standby", "standby");
    let area = Rect::new(0, 0, 50, 20);
    let buf = draw_cast(&summary, area.width, area.height);
    let rows: Vec<String> = (0..area.height).map(|y| row_text(&buf, area, y)).collect();
    let prod = rows.iter().find(|l| l.contains("Ⓟ")).expect("producer row");
    assert!(prod.contains("standby"), "{prod:?}");
    assert!(
        !rows.iter().any(|l| l.contains("starts with the studio")),
        "{rows:?}"
    );
}
