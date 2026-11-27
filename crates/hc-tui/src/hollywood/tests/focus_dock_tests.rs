//! Size × focus layout matrix (`200×19`, `200×12`, `120×24`, `100×17`, `60`),
//! monotonic two-phase height planner invariants, and 3-row micro frames.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::draw_tests::{layer, lines, stage_up};
use super::*;
use crate::chrome::palette::Palette;
use crate::hollywood::expanded_extent;
use crate::hollywood::{Hollywood, LayerMode, LayerOpts, Target, render};

fn focus_fixture() -> Summary {
    let mut value = serde_json::to_value(six_studios_ready_recast_summary()).unwrap();
    value["hollywood"]["exchange"] = serde_json::json!([
        { "seq": 1, "ts_ms": 600_000, "who": "user", "text": "check all six studios" },
        { "seq": 2, "ts_ms": 650_000, "who": "hollywood", "text": "all 6 studios running on local stage" },
        { "seq": 3, "ts_ms": 700_000, "who": "user", "text": "recast acme-shop roles onto gemini" },
        { "seq": 4, "ts_ms": 750_000, "who": "hollywood", "text": "queued recast analysis across producer director and writer" },
        { "seq": 10, "ts_ms": 700_000, "who": "hollywood", "text": "acme-shop recast cards c1372, c1373, c1374 proposed; ready to apply or finish" }
    ]);
    serde_json::from_value(value).unwrap()
}

fn hw_with(selected: Target) -> Hollywood {
    let mut hw = layer(LayerMode::Expanded);
    hw.selected = Some(selected);
    hw.active_studio = Some(crate::studio::StudioKey::local("acme-shop"));
    hw.now_ms = Some(1_000_000);
    hw
}

struct MatrixCase {
    label: &'static str,
    width: u16,
    frame_h: u16,
    cap: u16,
    mobile: bool,
    expect_studio_h: Option<u16>,
}

#[test]
fn focus_dock_size_and_focus_matrix_on_six_studios() {
    let summary = focus_fixture();
    let list = machines(Some(&summary));
    let palette = Palette::backlot();
    let status: &crate::hollywood::sheet::Status = &stage_up;

    let cases = [
        MatrixCase {
            label: "200x19",
            width: 200,
            frame_h: 38,
            cap: 19,
            mobile: false,
            expect_studio_h: Some(7),
        },
        MatrixCase {
            label: "174x19",
            width: 174,
            frame_h: 38,
            cap: 19,
            mobile: false,
            expect_studio_h: Some(7),
        },
        MatrixCase {
            label: "200x12",
            width: 200,
            frame_h: 24,
            cap: 12,
            mobile: false,
            expect_studio_h: Some(1),
        },
        MatrixCase {
            label: "120x24",
            width: 120,
            frame_h: 48,
            cap: 24,
            mobile: false,
            expect_studio_h: Some(7),
        },
        MatrixCase {
            label: "100x17",
            width: 100,
            frame_h: 34,
            cap: 17,
            mobile: false,
            expect_studio_h: Some(5),
        },
        MatrixCase {
            label: "74x17",
            width: 74,
            frame_h: 34,
            cap: 17,
            mobile: false,
            expect_studio_h: Some(5),
        },
        MatrixCase {
            label: "60x16_mobile",
            width: 60,
            frame_h: 32,
            cap: 16,
            mobile: true,
            expect_studio_h: None,
        },
    ];

    for case in cases {
        let MatrixCase {
            label,
            width,
            frame_h,
            cap,
            mobile,
            expect_studio_h,
        } = case;
        let area = Rect::new(0, 0, width, frame_h);
        let opts = LayerOpts {
            agent: true,
            mobile,
            suppress_highlights: false,
            no_shadow: true,
        };
        for (focus_name, target) in [
            ("WAITING", Target::Card("c1372".into())),
            ("HOLLYWOOD", Target::Input),
            (
                "STUDIOS",
                Target::Studio(crate::studio::StudioKey::local("acme-shop")),
            ),
        ] {
            let mut hw = hw_with(target);
            if focus_name == "HOLLYWOOD" {
                hw.editing = true;
            }
            let (ext_h, shadow_h) = expanded_extent(area, &list, &status, &hw, opts);
            assert_eq!(shadow_h, 0, "{label} {focus_name}: external shadow removed");
            assert!(
                ext_h <= cap,
                "{label} {focus_name}: height {ext_h} exceeds cap {cap}"
            );

            let mut buf = Buffer::empty(area);
            let hits = render(
                &mut buf,
                area,
                &list,
                Some("Local"),
                &status,
                &hw,
                opts,
                &palette,
            );
            assert_eq!(hits.area.height, ext_h, "{label} {focus_name}");
            assert!(!hits.shadow_drawn, "{label} {focus_name}: no shadow");
            let drawn_area = Rect::new(0, 0, width, hits.area.height);
            let text = lines(&buf, drawn_area).join("\n");

            if std::env::var_os("CALLSHEET_FOCUS_DUMP").is_some()
                && matches!(label, "200x19" | "100x17")
            {
                eprintln!(
                    "=== DUMP {label} focus={focus_name} h={} ===\n{text}\n=== END ===",
                    hits.area.height
                );
            }

            // Common invariants across every size and focus.
            assert!(
                !hits.input.is_empty(),
                "{label} {focus_name}: compose must draw"
            );
            assert!(
                text.contains("tell hollywood"),
                "{label} {focus_name}:\n{text}"
            );
            assert!(text.contains("CARD c1372"), "{label} {focus_name}:\n{text}");
            assert!(text.contains("READY"), "{label} {focus_name}:\n{text}");
            if width < 100 {
                assert!(
                    text.contains("✦ HOLLYWOOD"),
                    "{label} {focus_name} narrow mark:\n{text}"
                );
            } else if width < 120 {
                assert!(
                    text.contains("✦ H O L L Y W O O D"),
                    "{label} {focus_name} medium mark:\n{text}"
                );
            } else {
                assert!(
                    text.contains("‹✦› H O L L Y W O O D ‹✦›"),
                    "{label} {focus_name} wide mark:\n{text}"
                );
            }

            // Unfocused zones never expand (`HOLLYWOOD` unfocused rule appears only from spare).
            match focus_name {
                "WAITING" => {
                    assert!(hits.strip, "{label}: STUDIOS stays 1-row strip");
                    assert!(
                        !text.contains("╠══╡") && !text.contains("╠═╡"),
                        "{label}: HOLLYWOOD must stay unfocused"
                    );
                    assert!(
                        text.contains("why: producer hit quota limit"),
                        "{label}: selected wait detail"
                    );
                    let wait_y = hits
                        .rows
                        .iter()
                        .find(|(_, t)| matches!(t, Target::Card(_)))
                        .expect("wait row hit")
                        .0
                        .y;
                    assert_eq!(buf[(0, wait_y)].symbol(), "▌", "{label}: col-0 gutter");
                }
                "HOLLYWOOD" => {
                    assert!(hits.strip, "{label}: STUDIOS stays 1-row strip");
                    assert!(!text.contains("why:"), "{label}: WAITING unfocused");
                    assert!(
                        !text.contains("a apply"),
                        "{label}: WAITING chips hidden when unfocused"
                    );
                    assert!(
                        text.contains("╠")
                            && (text.contains("H O L L Y W O O D") || text.contains("HOLLYWOOD")),
                        "{label}: HOLLYWOOD focused rule:\n{text}"
                    );
                }
                "STUDIOS" => {
                    assert!(!text.contains("why:"), "{label}: WAITING unfocused");
                    assert!(
                        !text.contains("╠══╡") && !text.contains("╠═╡✦"),
                        "{label}: HOLLYWOOD must stay unfocused"
                    );
                    if let Some(exp_h) = expect_studio_h {
                        if exp_h >= 3 {
                            assert!(!hits.strip, "{label}: STUDIOS expanded frames h={exp_h}");
                            let frect = hits
                                .rows
                                .iter()
                                .find(|(_, t)| matches!(t, Target::Studio(_)))
                                .unwrap()
                                .0;
                            assert_eq!(frect.height, exp_h, "{label}: frame height");
                        } else {
                            assert!(hits.strip, "{label}: tight STUDIOS uses 1-row strip");
                        }
                    }
                }
                _ => unreachable!(),
            }
        }
    }
}

#[test]
fn height_planner_is_monotonic_and_never_drops_compose_or_selected_wait() {
    let base_summary = focus_fixture();
    let palette = Palette::backlot();
    let status: &crate::hollywood::sheet::Status = &stage_up;

    for wait_count in 1..=12usize {
        let mut val = serde_json::to_value(&base_summary).unwrap();
        val["studios"][0]["ready_to_finish"] = serde_json::json!(false);
        val["studios"][1]["ready_to_finish"] = serde_json::json!(false);
        val["hollywood"]["cards"] = serde_json::json!(
            (1..=wait_count)
                .map(|i| serde_json::json!({
                    "id": format!("c{i}"),
                    "studio": "acme-shop",
                    "action": format!("task {i}"),
                    "why": format!("reason {i}"),
                }))
                .collect::<Vec<_>>()
        );
        let sum: Summary = serde_json::from_value(val).unwrap();
        let list = machines(Some(&sum));
        let (waits, rows) = crate::hollywood::band_rows(&list, &status, true);

        let last_wait = Target::Card(format!("c{wait_count}"));
        for mobile in [true, false] {
            let heights: Vec<u16> = if mobile {
                (6..=16).collect()
            } else {
                (12..=30).collect()
            };
            for cap in heights {
                let frame_h = if mobile { cap } else { cap * 2 };
                let area = Rect::new(0, 0, if mobile { 60 } else { 120 }, frame_h);
                let opts = LayerOpts {
                    agent: true,
                    mobile,
                    suppress_highlights: false,
                    no_shadow: true,
                };
                let base_hw = hw_with(Target::Title);
                let base_plan = crate::hollywood::plan::plan(area, &waits, &rows, &base_hw, opts);
                assert_eq!(
                    base_plan.tail_rows, 1,
                    "compose present w={wait_count} cap={cap}"
                );
                assert!(
                    base_plan.shown >= 1,
                    "selected wait present w={wait_count} cap={cap}"
                );
                assert!(base_plan.height() <= cap, "height <= cap");

                for target in [
                    last_wait.clone(),
                    Target::Input,
                    Target::Studio(crate::studio::StudioKey::local("acme-shop")),
                ] {
                    // Phase 2 expansion must never shrink any zone kept by Phase 1 for the same focus,
                    // and Phase 1 + Phase 2 must stay monotone across increasing caps (`cap` vs `cap + 1`).
                    let hw = hw_with(target.clone());
                    let mut p1 = crate::hollywood::plan::Plan {
                        title: true,
                        shown: waits.len(),
                        details: vec![false; waits.len()],
                        detail_extra: vec![0; waits.len()],
                        hollywood_rule: false,
                        headline: false,
                        talk: 1,
                        talk_wraps: vec![0; 5],
                        live: false,
                        tail_rows: 1,
                        studios_perf: false,
                        frames_h: 1,
                        hints: true,
                        bottom_edge: true,
                    };
                    crate::hollywood::plan::fit_base(
                        &mut p1,
                        cap,
                        crate::hollywood::Section::focus_of(&target),
                        waits.len(),
                    );
                    let p = crate::hollywood::plan::plan(area, &waits, &rows, &hw, opts);
                    let next_area = Rect::new(
                        0,
                        0,
                        area.width,
                        if mobile { cap + 1 } else { (cap + 1) * 2 },
                    );
                    let p_next = crate::hollywood::plan::plan(next_area, &waits, &rows, &hw, opts);

                    assert!(p.height() <= cap, "{target:?} cap={cap}");
                    assert_eq!(p.tail_rows, 1);
                    assert!(p.shown >= 1);
                    assert_eq!(p.shown, p1.shown, "{target:?} phase 2 never shrinks waits");
                    assert!(u16::from(p.title) >= u16::from(p1.title));
                    assert!(p.talk >= p1.talk);
                    assert!(p.frames_h >= p1.frames_h);
                    assert!(u16::from(p.hints) >= u16::from(p1.hints));
                    assert!(
                        p_next.shown >= p.shown,
                        "{target:?} shown monotone cap={cap}"
                    );
                    assert!(p_next.talk >= p.talk, "{target:?} talk monotone cap={cap}");
                    assert!(
                        p_next.frames_h >= p.frames_h,
                        "{target:?} frames monotone cap={cap}"
                    );

                    // Selected last wait + compose always physically draw in the buffer.
                    if matches!(target, Target::Card(_)) {
                        let mut buf = Buffer::empty(area);
                        let hits = render(
                            &mut buf,
                            area,
                            &list,
                            Some("Local"),
                            &status,
                            &hw,
                            opts,
                            &palette,
                        );
                        let text =
                            lines(&buf, Rect::new(0, 0, area.width, hits.area.height)).join("\n");
                        assert!(
                            text.contains(&format!("CARD c{wait_count} ")),
                            "selected wait missing w={wait_count} cap={cap}:\n{text}"
                        );
                        assert!(!hits.input.is_empty());
                    }
                }
            }
        }
    }
}

#[test]
fn three_row_micro_frame_renders_when_only_four_rows_remain_for_studios() {
    let summary = focus_fixture();
    let list = machines(Some(&summary));
    let palette = Palette::backlot();
    let status: &crate::hollywood::sheet::Status = &stage_up;
    // Unfocused base with 5 waits = 11 rows; at cap=14, studio zone gets 1 + 3 = 4 rows
    // (1 perforation + 3-row micro frames).
    let area = Rect::new(0, 0, 120, 28);
    let hw = hw_with(Target::Studio(crate::studio::StudioKey::local("acme-shop")));
    let mut buf = Buffer::empty(area);
    let hits = render(
        &mut buf,
        area,
        &list,
        Some("Local"),
        &status,
        &hw,
        LayerOpts {
            agent: true,
            mobile: false,
            suppress_highlights: false,
            no_shadow: true,
        },
        &palette,
    );
    let frame = hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Studio(_)))
        .expect("micro frame rendered")
        .0;
    assert_eq!(frame.height, 3);
    let text = lines(&buf, Rect::new(0, 0, 120, hits.area.height)).join("\n");
    assert!(text.contains("✓ ready"), "{text}");
    assert!(text.contains("r2 · 12/13"), "{text}");
}

#[test]
fn fit_base_sheds_in_babylon_order() {
    use crate::hollywood::Section;
    use crate::hollywood::plan::{Plan, fit_base};

    let base = || Plan {
        title: true,
        shown: 5,
        details: vec![false; 5],
        detail_extra: vec![0; 5],
        hollywood_rule: true,
        headline: false,
        talk: 1,
        talk_wraps: vec![0],
        live: false,
        tail_rows: 1,
        studios_perf: false,
        frames_h: 1,
        hints: true,
        bottom_edge: true,
    };

    // Total base = 12 (`hollywood_rule` + `title` + `5` + `talk` + `tail` + `strip` + `hints` + `sill`).
    // 1. Shed unfocused hollywood_rule (cap 11).
    let mut p = base();
    fit_base(&mut p, 11, Some(Section::Waiting), 5);
    assert!(!p.hollywood_rule && p.hints && p.talk == 1 && p.shown == 5 && p.bottom_edge);

    // 2. Shed hints (cap 10).
    let mut p = base();
    fit_base(&mut p, 10, Some(Section::Waiting), 5);
    assert!(!p.hints && p.talk == 1 && p.shown == 5 && p.bottom_edge);

    // 3. Shed unfocused talk (cap 9).
    let mut p = base();
    fit_base(&mut p, 9, Some(Section::Waiting), 5);
    assert!(p.talk == 0 && p.shown == 5 && p.frames_h == 1 && p.title && p.bottom_edge);

    // 4. Scroll WAITING down to min(3, 5) = 3 (cap 7).
    let mut p = base();
    fit_base(&mut p, 7, Some(Section::Waiting), 5);
    assert_eq!(p.shown, 3);
    assert_eq!(p.frames_h, 1);
    assert!(p.title && p.bottom_edge);

    // 5. Shed unfocused strip (cap 6).
    let mut p = base();
    fit_base(&mut p, 6, Some(Section::Waiting), 5);
    assert_eq!(p.shown, 3);
    assert_eq!(p.frames_h, 0);
    assert!(p.title && p.bottom_edge);

    // 6. Shed title (cap 5).
    let mut p = base();
    fit_base(&mut p, 5, Some(Section::Waiting), 5);
    assert!(!p.title && p.shown == 3 && p.bottom_edge);

    // 7. Scroll WAITING down to 1 while keeping marquee sill down to cap = 3.
    let mut p = base();
    fit_base(&mut p, 3, Some(Section::Waiting), 5);
    assert_eq!(p.shown, 1);
    assert_eq!(p.tail_rows, 1);
    assert!(p.bottom_edge, "marquee sill survives down to cap = 3");

    // 8. Sill sheds only when cap < 3.
    let mut p = base();
    fit_base(&mut p, 2, Some(Section::Waiting), 5);
    assert!(!p.bottom_edge && p.shown == 1 && p.tail_rows == 1);
}
