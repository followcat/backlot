//! PR 7 fail-first & regression tests for Call Sheet `DECISION` items and Pill `?N` bucket.
//! Neutral fixtures only (`acme-shop`, `beta-api`, `producer`).

use super::draw_tests::{layer, lines, stage_up};
use super::*;
use crate::chrome::lot::LotCounts;
use crate::chrome::palette::Palette;
use crate::hollywood::items::any_user_urgent;
use crate::hollywood::{LayerMode, LayerOpts, WaitBucket, expanded_height, render, waiting_items};
use backlot_model::summary::Summary;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

fn design_waiting_json(suggest_skip: bool) -> serde_json::Value {
    let mut base = summary_json();
    base["hollywood"]["cards"] = serde_json::json!([]);
    base["studios"][0]["roles"] = serde_json::json!([
        { "role": "producer", "name": "producer", "terminal": "t1", "state": "live", "work": "waiting" }
    ]);
    base["studios"][0]["waiting"] = serde_json::json!(0);
    let design = if suggest_skip {
        serde_json::json!({
            "version": 1,
            "round": 0,
            "phase": "suggest_skip",
            "waiting_user": true,
            "suggest_skip": true,
            "skip_reason": "tiny bugfix in existing endpoint; no architecture choices needed",
            "doc_file": "design/v1.md",
            "questions": [],
            "waiting_since_ms": 940_000
        })
    } else {
        serde_json::json!({
            "version": 1,
            "round": 2,
            "phase": "waiting_user",
            "waiting_user": true,
            "suggest_skip": false,
            "doc_file": "design/v1.md",
            "summary": "Split worker or keep single binary with SQLite WAL",
            "questions": [
                {
                    "id": "Q1",
                    "prompt": "Separate job worker crate?",
                    "recommendation": "keep single crate for v1",
                    "section_ref": "§R2.W"
                },
                {
                    "id": "Q2",
                    "prompt": "Session storage backend?",
                    "recommendation": "SQLite WAL table",
                    "section_ref": "§R2.D"
                },
                {
                    "id": "Q3",
                    "prompt": "Retry backoff cap?",
                    "recommendation": "30s exponential cap"
                }
            ],
            "waiting_since_ms": 940_000
        })
    };
    base["studios"][0]["design"] = design;
    base
}

fn counts_from(waits: &[crate::hollywood::Wait<'_>], studio_count: usize) -> LotCounts {
    let mut counts = LotCounts {
        studios: studio_count,
        ..LotCounts::default()
    };
    for wait in waits {
        match WaitBucket::of(wait) {
            WaitBucket::Blocking => counts.blocking += 1,
            WaitBucket::Asks => counts.asks += 1,
            WaitBucket::Cards => counts.cards += 1,
            WaitBucket::Ready => counts.ready += 1,
        }
    }
    counts
}

#[test]
fn decision_waits_use_asks_bucket_peach_never_blocking_or_urgent() {
    let summary: Summary = serde_json::from_value(design_waiting_json(false)).unwrap();
    let list = machines(Some(&summary));
    let waits = waiting_items(&list, &stage_up);
    assert_eq!(waits.len(), 1, "expected 1 Decision wait item");
    assert_eq!(WaitBucket::of(&waits[0]), WaitBucket::Asks);
    let counts = counts_from(&waits, summary.studios.len());
    assert_eq!(counts.asks, 1, "warm ?1 pill must increment");
    assert_eq!(counts.blocking, 0, "blocking !N must remain zero");
    assert!(!any_user_urgent(&waits), "Decision is never user-urgent");

    // Non-waiting phases (`waiting_proposal`, `signed`) must NOT emit Call Sheet wait or ?N.
    for phase in [
        "waiting_proposal",
        "waiting_buildability",
        "waiting_summary",
        "signed",
    ] {
        let mut v = design_waiting_json(false);
        v["studios"][0]["design"]["phase"] = serde_json::json!(phase);
        v["studios"][0]["design"]["waiting_user"] = serde_json::json!(false);
        let non_waiting: Summary = serde_json::from_value(v).unwrap();
        let non_list = machines(Some(&non_waiting));
        let non_waits = waiting_items(&non_list, &stage_up);
        assert!(
            non_waits.is_empty(),
            "phase {phase} with waiting_user=false must emit no Call Sheet wait"
        );
    }
}

#[test]
fn callsheet_decision_200x44_and_100x40_render_summary_q1_q3_and_chips() {
    let summary: Summary = serde_json::from_value(design_waiting_json(false)).unwrap();
    let list = machines(Some(&summary));
    let palette = Palette::backlot();
    let mut hw = layer(LayerMode::Expanded);
    let waits = waiting_items(&list, &stage_up);
    assert!(
        !waits.is_empty(),
        "expected Wait::Decision item in Call Sheet"
    );
    hw.selected = Some(waits[0].target());
    hw.now_ms = Some(1_000_000);

    let opts = LayerOpts {
        agent: true,
        mobile: false,
        suppress_highlights: false,
        no_shadow: false,
    };

    for (cols, rows) in [(200u16, 44u16), (100u16, 40u16)] {
        let area = Rect::new(0, 0, cols, rows);
        let h = expanded_height(area, &list, &stage_up, &hw, opts);
        assert!(h <= rows / 2, "{cols}x{rows} exceeded cap: {h}");
        let mut buf = Buffer::empty(area);
        let hits = render(
            &mut buf,
            area,
            &list,
            Some("local"),
            &stage_up,
            &hw,
            opts,
            &palette,
        );
        let text = lines(&buf, area).join("\n");
        assert!(
            text.contains("DECISION"),
            "{cols}x{rows} missing DECISION:\n{text}"
        );
        assert!(
            text.contains("acme-shop"),
            "{cols}x{rows} missing studio:\n{text}"
        );
        assert!(text.contains("R2"), "{cols}x{rows} missing R2:\n{text}");
        assert!(
            text.contains("Q1") && text.contains("Q2") && text.contains("Q3"),
            "{cols}x{rows} missing Q1..Q3:\n{text}"
        );
        assert!(
            text.contains("§R2.W") && text.contains("keep single crate"),
            "{cols}x{rows} missing section_ref / recommendation:\n{text}"
        );
        // Selected row renders action chips (`read o`, `reply r`, `sign a`, `open ↵`).
        for tok in ["read o", "reply r", "sign a", "open ↵"] {
            assert!(
                text.contains(tok),
                "{cols}x{rows} missing chip `{tok}`:\n{text}"
            );
        }
        let row_y = hits
            .rows
            .iter()
            .find(|(_, t)| *t == waits[0].target())
            .map(|(r, _)| r.y)
            .expect("decision row hit rect");
        assert_eq!(
            buf[(2, row_y)].fg,
            palette.peach,
            "indicator bar must be peach"
        );
    }
}

#[test]
fn callsheet_suggest_skip_renders_skip_suggested_discuss_and_skip() {
    let summary: Summary = serde_json::from_value(design_waiting_json(true)).unwrap();
    let list = machines(Some(&summary));
    let palette = Palette::backlot();
    let mut hw = layer(LayerMode::Expanded);
    let waits = waiting_items(&list, &stage_up);
    assert_eq!(waits.len(), 1);
    hw.selected = Some(waits[0].target());
    hw.now_ms = Some(1_000_000);

    let area = Rect::new(0, 0, 200, 44);
    let mut buf = Buffer::empty(area);
    render(
        &mut buf,
        area,
        &list,
        Some("local"),
        &stage_up,
        &hw,
        LayerOpts::default(),
        &palette,
    );
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("DECISION"), "{text}");
    assert!(text.contains("skip suggested"), "{text}");
    assert!(text.contains("discuss r"), "{text}");
    assert!(text.contains("skip a"), "{text}");
    assert!(text.contains("read o"), "{text}");
}

#[test]
fn callsheet_zero_design_invariance() {
    let mut plain = design_waiting_json(false);
    plain["studios"][0]
        .as_object_mut()
        .unwrap()
        .remove("design");
    let baseline: Summary = serde_json::from_value(plain).unwrap();
    let with_idle_null = summary_json();
    let mut null_design = with_idle_null.clone();
    null_design["studios"][0]["design"] = serde_json::Value::Null;
    let s1: Summary = serde_json::from_value(with_idle_null).unwrap();
    let s2: Summary = serde_json::from_value(null_design).unwrap();

    let area = Rect::new(0, 0, 200, 44);
    let palette = Palette::backlot();
    let l1 = machines(Some(&s1));
    let l2 = machines(Some(&s2));
    let hw1 = layer(LayerMode::Expanded);
    let hw2 = layer(LayerMode::Expanded);
    let mut b1 = Buffer::empty(area);
    let mut b2 = Buffer::empty(area);
    render(
        &mut b1,
        area,
        &l1,
        Some("local"),
        &stage_up,
        &hw1,
        LayerOpts::default(),
        &palette,
    );
    render(
        &mut b2,
        area,
        &l2,
        Some("local"),
        &stage_up,
        &hw2,
        LayerOpts::default(),
        &palette,
    );
    assert_eq!(b1, b2);
    let b_list = machines(Some(&baseline));
    assert!(waiting_items(&b_list, &stage_up).is_empty());
}
