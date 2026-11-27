//! PR 7 fail-first & regression tests for Lot Dock expanded design rows and 16-col collapsed strip.
//! Neutral fixtures only (`acme-shop`, `beta-api`, `producer`).

use super::*;
use crate::chrome::palette::Palette;
use crate::panel::config::StudioKeys;
use crate::panel::rows::rows;
use crate::panel::rows_fit::block_height;
use crate::panel::rows_fold::folded_block_height;
use crate::panel::{Panel, PanelMode, render};
use crate::studio::StudioKey;
use backlot_model::summary::Summary;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

fn panel(mode: PanelMode) -> Panel {
    Panel {
        mode,
        ..Panel::new()
    }
}

fn lines(buf: &Buffer, area: Rect) -> Vec<String> {
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

fn draw_panel(area: Rect, summary: Option<&Summary>, p: &Panel, current: Option<&str>) -> Buffer {
    let mut buf = Buffer::empty(area);
    let cur = current.map(StudioKey::local);
    render(
        &mut buf,
        area,
        &machines(summary),
        |_| true,
        |_| true,
        p,
        p.mode,
        cur.as_ref(),
        cur.as_ref(),
        2,
        None,
        &StudioKeys::default(),
        None,
        false,
        false,
        true,
        &Default::default(),
        &Palette::backlot(),
        1,
    );
    buf
}

fn json_with_design(phase: &str, suggest_skip: bool) -> serde_json::Value {
    let mut v = summary_json();
    v["studios"][0]["roles"] = serde_json::json!([
        { "role": "producer", "name": "producer", "terminal": "t1", "state": "live", "work": "waiting" }
    ]);
    let waiting_user = matches!(phase, "waiting_user" | "suggest_skip");
    let round = if suggest_skip { 0 } else { 2 };
    let questions = if phase == "waiting_user" {
        serde_json::json!([
            {
                "id": "Q1",
                "prompt": "Separate job worker crate?",
                "recommendation": "single crate",
                "section_ref": "§R2.W"
            },
            {
                "id": "Q2",
                "prompt": "Storage engine?",
                "recommendation": "SQLite WAL",
                "section_ref": "§R2.D"
            }
        ])
    } else {
        serde_json::json!([])
    };
    v["studios"][0]["design"] = serde_json::json!({
        "version": 1,
        "round": round,
        "phase": phase,
        "waiting_user": waiting_user,
        "suggest_skip": suggest_skip,
        "skip_reason": if suggest_skip { serde_json::json!("small typo fix only") } else { serde_json::Value::Null },
        "doc_file": "design/v1.md",
        "summary": if phase == "waiting_user" { serde_json::json!("Single crate vs worker split") } else { serde_json::Value::Null },
        "questions": questions,
        "waiting_since_ms": 940_000
    });
    v
}

#[test]
fn expanded_block_displays_every_active_design_phase_and_agrees_with_heights() {
    let p = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 40, 28);

    let cases = [
        (
            "waiting_user",
            false,
            vec![
                "◆ design · R2 · you",
                "Q1",
                "Q2",
                "r reply   o read   a sign",
            ],
        ),
        (
            "suggest_skip",
            true,
            vec![
                "◆ design · skip suggested · you",
                "r discuss  o read  a skip",
            ],
        ),
        ("waiting_proposal", false, vec!["◆ design · R2 · proposal"]),
        (
            "waiting_buildability",
            false,
            vec!["◆ design · R2 · review"],
        ),
        ("waiting_summary", false, vec!["◆ design · R2 · summary"]),
    ];

    for (phase, skip, want_tokens) in cases {
        let s: Summary = serde_json::from_value(json_with_design(phase, skip)).unwrap();
        let ms = machines(Some(&s));
        let cur = StudioKey::local("acme-shop");
        let r = rows(&ms, |_| true, |_| true, &p, p.mode, Some(&cur), 40);
        // BlockTop..=BlockBottom count equals block_height for Full studio
        let studio_ref = crate::panel::data::StudioRef {
            machine: None,
            studio: &s.studios[0],
        };
        let block_rows_count = r.len() - 1; // subtract Title row
        assert_eq!(
            block_height(studio_ref, &p, false, false, false),
            block_rows_count,
            "phase {phase}: block_height must match emitted rows"
        );
        let mut two_v = json_with_design(phase, skip);
        let second = two_v["studios"][0].clone();
        two_v["studios"] = serde_json::json!([
            two_v["studios"][0],
            {
                "id": "beta-api",
                "title": "Beta API",
                "state": "live",
                "requirement_version": 1,
                "mode": "standard",
                "open_reviews": 0,
                "waiting": 0,
                "roles": second["roles"]
            }
        ]);
        let two_s: Summary = serde_json::from_value(two_v).unwrap();
        let two_ms = machines(Some(&two_s));
        let other = StudioKey::local("beta-api");
        let folded_r = rows(&two_ms, |_| true, |_| true, &p, p.mode, Some(&other), 60);
        let acme_rows = folded_r
            .iter()
            .filter(|row| {
                row.target().studio() == Some(&cur)
                    || matches!(row, crate::panel::rows::Row::BlockBottom(s) if s.key() == cur)
            })
            .count();
        assert_eq!(
            folded_block_height(&two_s.studios[0]),
            acme_rows,
            "phase {phase}: folded_block_height must match emitted folded rows"
        );

        let buf = draw_panel(area, Some(&s), &p, Some("acme-shop"));
        let text = lines(&buf, area).join("\n");
        for tok in want_tokens {
            assert!(
                text.contains(tok),
                "phase {phase}: missing `{tok}` in:\n{text}"
            );
        }
    }
}

#[test]
fn expanded_block_design_none_and_signed_match_baseline_cells() {
    let mut base_v = json_with_design("signed", false);
    base_v["studios"][0]
        .as_object_mut()
        .unwrap()
        .remove("design");
    let baseline: Summary = serde_json::from_value(base_v).unwrap();
    let signed: Summary = serde_json::from_value(json_with_design("signed", false)).unwrap();

    let p = panel(PanelMode::Expanded);
    let area = Rect::new(0, 0, 40, 24);
    let b1 = draw_panel(area, Some(&baseline), &p, Some("acme-shop"));
    let b2 = draw_panel(area, Some(&signed), &p, Some("acme-shop"));
    assert_eq!(b1, b2, "signed design must match no-design baseline");
}

#[test]
fn collapsed_16_col_strip_shows_single_line_peach_design_badge() {
    let mut v = json_with_design("waiting_user", false);
    v["studios"][0]["id"] = serde_json::json!("beta");
    v["studios"][0]["title"] = serde_json::json!("Beta");
    let s: Summary = serde_json::from_value(v).unwrap();

    let p = panel(PanelMode::Collapsed);
    let area = Rect::new(0, 0, 16, 12);
    let palette = Palette::backlot();
    let buf = draw_panel(area, Some(&s), &p, Some("beta"));
    let screen = lines(&buf, area);
    let strip_line = screen
        .iter()
        .enumerate()
        .find(|(_, l)| l.contains("beta"))
        .expect("collapsed strip studio row");
    assert!(
        strip_line.1.contains("◆R2 ?") || strip_line.1.contains("◆?"),
        "expected single-line `◆R2 ?` / `◆?` in 16-col strip, got:\n{}",
        screen.join("\n")
    );
    // Ensure the badge is on the exact same row (no wrap to next row) and uses peach.
    let y = strip_line.0 as u16;
    let next_row = screen
        .get(strip_line.0 + 1)
        .map(String::as_str)
        .unwrap_or("");
    assert!(
        !next_row.contains('◆') && !next_row.contains('R'),
        "16-col strip badge must never wrap onto next line:\n{}",
        screen.join("\n")
    );
    let diamond_x = (0..16u16)
        .find(|&x| buf[(x, y)].symbol() == "◆")
        .expect("peach diamond cell");
    assert_eq!(buf[(diamond_x, y)].fg, palette.peach);
}
