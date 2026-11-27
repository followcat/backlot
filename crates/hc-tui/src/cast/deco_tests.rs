//! Babylon S4 Cast header rule, closed divider rule, scroll, and herdr bypass tests.

use backlot_model::summary::Summary;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use std::collections::BTreeMap;

use super::*;
use crate::chrome::palette::Palette;
use crate::chrome::render::ShellHitMap;

fn summary() -> Summary {
    serde_json::from_value(serde_json::json!({
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
    }))
    .unwrap()
}

fn config() -> ClientShellConfig {
    let mut cfg = ClientShellConfig::from_config(
        &crate::config::Config::default(),
        None,
        None,
        std::path::PathBuf::new(),
    );
    cfg.palette = Palette::backlot();
    cfg.studio_chrome = true;
    cfg
}

fn cast_view<'a>(summary: &'a Summary, focused: Option<&'a str>) -> CastView<'a> {
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
        titles: BTreeMap::from([
            ("t1".to_string(), Some("stage console".to_string())),
            ("t4".to_string(), Some("feature".to_string())),
        ]),
        shells: Vec::new(),
        others: Vec::new(),
        others_expanded: false,
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
        studio_chrome: true,
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
fn deco_cast_divider_closed_rule_scroll_and_herdr_bypass() {
    let summary = summary();
    let mut cfg = config();
    let pal = cfg.palette.clone();
    let elev = pal.elevation();
    let area = Rect::new(0, 0, 26, 20);

    // 1. Unfocused Deco Cast: `──╡ ✦ CAST ╞────` in `gilt` on row 0,
    // `── · 1 closed · dave     ▸` single line, 0 extra rows, click hits preserved.
    let unfocused = cast_view(&summary, None);
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
    let div_y = ulines.iter().position(|l| l.contains("1 closed")).unwrap() as u16;
    assert_eq!(ulines[div_y as usize], "── · 1 closed · dave     ▸");
    assert_eq!(ubuf[(0, div_y)].fg, elev.border_muted);
    assert_eq!(ubuf[(3, div_y)].fg, elev.gilt);
    assert_eq!(ubuf[(5, div_y)].fg, elev.muted_surface);
    assert!(uhits.cast_closed.is_some());
    assert!(!uhits.agents.is_empty());

    // 2. Focused Deco Cast (`region_focused = true`): `━━╡ ▸ CAST ╞━━━━` in `accent` + `▌` bar.
    let mut focused = cast_view(&summary, Some("t1"));
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
