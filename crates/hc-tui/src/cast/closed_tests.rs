//! Collapsed closed-roles Cast line tests: 26/34-column rendering for 0, 1,
//! and 16 closed roles, CJK truncation, focus highlight, and stale terminal hit
//! exclusion.

use std::collections::BTreeMap;

use backlot_model::summary::Summary;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

use super::rows::{CastRow, cast_rows_with};
use super::*;
use crate::chrome::palette::Palette;
use crate::chrome::render::ShellHitMap;
use crate::focus::CastTarget;
use crate::studio::StudioKey;

fn closed_summary(closed_roles: &[(String, String, Option<String>)], with_open: bool) -> Summary {
    let mut roles = Vec::new();
    if with_open {
        roles.push(serde_json::json!({
            "role": "producer",
            "name": "producer",
            "terminal": "t1",
            "state": "live",
            "work": "working",
            "runner": "claude",
            "model": "claude-opus-4-7",
        }));
    }
    for (idx, (name, role_id, terminal)) in closed_roles.iter().enumerate() {
        roles.push(serde_json::json!({
            "role": role_id,
            "name": name,
            "terminal": terminal,
            "state": "closed",
            "resumable": true,
            "task": format!("t{}", idx + 1),
        }));
    }
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
            "roles": roles,
        }]
    }))
    .unwrap()
}

fn sixteen_closed() -> Vec<(String, String, Option<String>)> {
    (1..=6)
        .map(|i| {
            (
                "actor-a".to_owned(),
                format!("actor-t{i}-a"),
                Some(format!("t{}", i + 10)),
            )
        })
        .chain((7..=16).map(|i| {
            (
                "actor-b".to_owned(),
                format!("actor-t{i}-b"),
                Some(format!("t{}", i + 10)),
            )
        }))
        .collect()
}

fn studio_config(studio_chrome: bool) -> ClientShellConfig {
    let mut cfg = ClientShellConfig::from_config(
        &crate::config::Config::default(),
        None,
        None,
        std::path::PathBuf::new(),
    );
    cfg.palette = Palette::backlot();
    cfg.studio_chrome = studio_chrome;
    cfg
}

fn cast_view(summary: &Summary, studio_chrome: bool) -> CastView<'_> {
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
        focused_terminal: summary.studios[0]
            .roles
            .iter()
            .find(|r| r.state == "live")
            .and_then(|r| r.terminal.as_deref()),
        titles: BTreeMap::new(),
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
        studio_chrome,
    }
}

fn draw(
    view: &CastView<'_>,
    cfg: &ClientShellConfig,
    width: u16,
    height: u16,
) -> (Buffer, ShellHitMap, Vec<String>) {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    let mut hits = ShellHitMap::default();
    let mut scroll = 0usize;
    let mut reveal = false;
    render_cast(
        &mut buf,
        area,
        view,
        cfg,
        &mut scroll,
        &mut reveal,
        &mut hits,
    );
    let lines = (0..height)
        .map(|y| {
            let mut line = String::new();
            let mut x = 0u16;
            while x < width {
                let sym = buf[(x, y)].symbol();
                line.push_str(sym);
                x = x.saturating_add(sym.width().max(1) as u16);
            }
            line.trim_end().to_owned()
        })
        .collect();
    (buf, hits, lines)
}

#[test]
fn zero_one_and_sixteen_closed_roles_at_26_and_34_cols() {
    let cfg = studio_config(true);

    // 0 closed roles -> no closed line, no cast_closed hit.
    let zero = closed_summary(&[], true);
    let zero_view = cast_view(&zero, true);
    for w in [26u16, 34] {
        let (_, hits, lines) = draw(&zero_view, &cfg, w, 14);
        let text = lines.join("\n");
        assert!(!text.contains("closed"), "width {w}:\n{text}");
        assert!(hits.cast_closed.is_none());
    }

    // 1 closed role (even with zero open roles) -> still uses the single line.
    let one = closed_summary(
        &[("actor-a".into(), "actor-t1-a".into(), Some("t9".into()))],
        false,
    );
    let one_view = cast_view(&one, true);
    let (buf26, hits26, lines26) = draw(&one_view, &cfg, 26, 10);
    let row26 = lines26
        .iter()
        .find(|l| l.contains("1 closed"))
        .expect("1 closed line at 26");
    assert_eq!(row26, "── · 1 closed · actor-a  ▸");
    assert_eq!(
        buf26[(25, hits26.cast_closed.as_ref().unwrap().0.y)].symbol(),
        "▸"
    );

    let (_, hits34, lines34) = draw(&one_view, &cfg, 34, 10);
    let row34 = lines34
        .iter()
        .find(|l| l.contains("1 closed"))
        .expect("1 closed line at 34");
    assert_eq!(row34, "── · 1 closed · actor-a          ▸");
    assert!(hits34.cast_closed.is_some());
    assert!(
        hits34.agents.is_empty(),
        "closed role never registers agents hit"
    );

    // 16 closed roles -> collapses to one line (no closed role rows below).
    let sixteen = closed_summary(&sixteen_closed(), true);
    let mut sixteen_view = cast_view(&sixteen, true);
    assert_eq!(
        cast_rows_with(&sixteen_view, true)
            .iter()
            .filter(|r| matches!(r, CastRow::ClosedFold { count: 16, .. }))
            .count(),
        1
    );
    assert!(
        !cast_rows_with(&sixteen_view, true)
            .iter()
            .any(|r| matches!(r, CastRow::Person { role, .. } if role.state == "closed")),
        "no closed role rows are emitted"
    );

    let (_, _, lines26) = draw(&sixteen_view, &cfg, 26, 12);
    let closed_lines_26: Vec<&String> = lines26.iter().filter(|l| l.contains("closed")).collect();
    assert_eq!(
        closed_lines_26.len(),
        1,
        "exactly one closed line:\n{}",
        lines26.join("\n")
    );
    assert_eq!(closed_lines_26[0], "── · 16 closed · actor-… ▸");
    assert!(
        !lines26.iter().any(|l| l.contains("t10")),
        "no closed role rows drawn:\n{}",
        lines26.join("\n")
    );

    let (_, _, lines34) = draw(&sixteen_view, &cfg, 34, 12);
    let closed_line_34 = lines34.iter().find(|l| l.contains("16 closed")).unwrap();
    assert_eq!(closed_line_34, "── · 16 closed · actor-a×6 acto… ▸");

    let (_, _, lines42) = draw(&sixteen_view, &cfg, 42, 12);
    let closed_line_42 = lines42.iter().find(|l| l.contains("16 closed")).unwrap();
    assert_eq!(closed_line_42, "── · 16 closed · actor-a×6 actor-b×10    ▸");

    // Focused closed-line cursor paints `▌` + selection_bg cleanly.
    sixteen_view.region_focused = true;
    sixteen_view.cursor_target = Some(CastTarget::ClosedRoles {
        studio: StudioKey::local("acme-shop"),
    });
    let (fbuf, fhits, flines) = draw(&sixteen_view, &cfg, 26, 12);
    let frect = fhits.cast_closed.unwrap().0;
    assert_eq!(fbuf[(frect.x, frect.y)].symbol(), "▌");
    assert_eq!(fbuf[(frect.x, frect.y)].fg, cfg.palette.accent);
    assert_eq!(fbuf[(frect.x, frect.y)].bg, cfg.palette.selection_bg);
    assert!(flines[frect.y as usize].starts_with("▌─ · 16 closed"));

    if std::env::var_os("CAST_CLOSED_SHOT").is_some() {
        eprintln!("=== CAST 16 CLOSED @ 26 COLS ===\n{}", lines26.join("\n"));
    }
}

#[test]
fn closed_role_with_stale_terminal_never_produces_agents_hit_in_either_chrome() {
    let summary = closed_summary(
        &[("actor-a".into(), "actor-t1-a".into(), Some("t99".into()))],
        true,
    );
    for studio_chrome in [true, false] {
        let cfg = studio_config(studio_chrome);
        let view = cast_view(&summary, studio_chrome);
        let (_, hits, _) = draw(&view, &cfg, 26, 14);
        assert!(
            hits.agents.iter().all(|(_, id)| id != "t99"),
            "closed role stale terminal leaked into hits.agents (studio_chrome={studio_chrome}): {:?}",
            hits.agents
        );
    }
}

#[test]
fn cjk_closed_token_truncates_on_char_boundary_before_arrow() {
    let cfg = studio_config(true);
    let summary = closed_summary(
        &[
            ("张伟工程".into(), "actor-t1-c".into(), None),
            ("张伟工程".into(), "actor-t2-c".into(), None),
        ],
        false,
    );
    let view = cast_view(&summary, true);
    let (buf, hits, lines) = draw(&view, &cfg, 26, 10);
    let rect = hits.cast_closed.expect("closed fold hit").0;
    let row = &lines[rect.y as usize];
    assert!(row.contains("2 closed"), "{row:?}");
    assert!(row.ends_with('▸'), "{row:?}");
    assert!(row.width() <= 26, "width exceeded 26: {row:?}");
    assert_eq!(buf[(25, rect.y)].symbol(), "▸");
}
