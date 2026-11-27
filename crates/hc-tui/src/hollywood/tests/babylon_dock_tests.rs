//! Cell-exact render snapshots and layout invariants for Option II
//! ("Kinoscope Premiere") across `(200×44, 174×19, 120×28, 100×17, 80×24, 60×16)`
//! and each Tab focus (`WAITING`, `HOLLYWOOD`, `STUDIOS`), plus short-terminal
//! `24`-row blocking prompt and CJK talk width invariance.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use unicode_width::UnicodeWidthStr;

use super::draw_tests::stage_up;
use super::*;
use crate::chrome::palette::Palette;
use crate::hollywood::talk::talk_text_width;
use crate::hollywood::{Hollywood, LayerMode, LayerOpts, Target, render};
use crate::studio::StudioKey;

/// Collapse Ratatui wide-char trailing spacer cells so rendered lines match
/// true monospace Unicode display text (e.g., CJK double-width glyphs).
fn visual_lines(buf: &Buffer, area: Rect) -> Vec<String> {
    (area.y..area.y + area.height)
        .map(|y| {
            let mut row = String::new();
            let mut skip = 0usize;
            for x in area.x..area.x + area.width {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                let sym = buf[(x, y)].symbol();
                row.push_str(sym);
                skip = sym.width().saturating_sub(1);
            }
            row.trim_end().to_string()
        })
        .collect()
}

fn snapshot_fixture() -> Summary {
    let mut value = serde_json::to_value(six_studios_ready_recast_summary()).unwrap();
    value["hollywood"]["exchange"] = serde_json::json!([
        {
            "seq": 1,
            "ts_ms": 600_000,
            "who": "user",
            "text": "check all six studios"
        },
        {
            "seq": 2,
            "ts_ms": 650_000,
            "who": "hollywood",
            "text": "all 6 studios running on local stage"
        },
        {
            "seq": 3,
            "ts_ms": 700_000,
            "who": "user",
            "text": "帮我盯一下 acme-shop 的同步与测试"
        },
        {
            "seq": 10,
            "ts_ms": 820_000,
            "who": "hollywood",
            "text": "acme-shop 全部 12 项任务已验收通过；已提三张换角卡 c1372、c1373、c1374 等你确认，如有疑问可按 o 查看。"
        }
    ]);
    serde_json::from_value(value).unwrap()
}

fn short_blocking_fixture() -> Summary {
    let mut value = serde_json::to_value(snapshot_fixture()).unwrap();
    value["studios"][2]["roles"][0]["attention_user"] = serde_json::json!(true);
    value["studios"][2]["roles"][0]["attention"] = serde_json::json!("permission prompt");
    value["studios"][2]["roles"][0]["attention_class"] = serde_json::json!("needs_you");
    value["studios"][2]["roles"][0]["attention_seq"] = serde_json::json!(9);
    value["studios"][2]["roles"][0]["attention_detail"] =
        serde_json::json!("./scripts/deploy-stage.sh --verify-gates");
    value["hollywood"]["cards"] = serde_json::json!([
        { "id": "c1", "studio": "acme-shop", "action": "recast producer → agy/gemini-3-pro/high", "why": "quota limit" },
        { "id": "c2", "studio": "acme-shop", "action": "recast director → agy/gemini-3-pro/high", "why": "quota limit" },
        { "id": "c3", "studio": "acme-shop", "action": "recast writer → agy/gemini-3-pro/high", "why": "quota limit" },
        { "id": "c4", "studio": "delta-ops", "action": "priority t4 → high", "why": "blocks wrap" },
        { "id": "c5", "studio": "eps-ui", "action": "pause t2", "why": "looping" }
    ]);
    serde_json::from_value(value).unwrap()
}

fn assert_sill_and_rails(buf: &Buffer, area: Rect, palette: &Palette, sill_y: u16, label: &str) {
    let elev = palette.elevation();
    let sill: Vec<String> = (area.x..area.x + area.width)
        .map(|x| buf[(x, sill_y)].symbol().to_string())
        .collect();
    assert_eq!(sill[0], "╚", "{label}: sill left corner");
    assert_eq!(sill[sill.len() - 1], "╝", "{label}: sill right corner");
    let joined = sill.concat();
    assert!(
        joined.contains("╡ ✦ ╞") && joined.contains('▪'),
        "{label}: sill must carry ╡ ✦ ╞ clasp and ▪ bulbs: {joined}"
    );
    for x in area.x..area.x + area.width {
        assert_eq!(
            buf[(x, sill_y)].bg,
            elev.stage_well,
            "{label}: sill x={x} bg"
        );
        assert_eq!(
            buf[(x, area.y)].bg,
            elev.stage_well,
            "{label}: title x={x} bg"
        );
    }
    let clasp_x = area.x + (area.width - 5) / 2;
    let clasp_right = clasp_x + 5;
    let clasp_mid = clasp_x + 2;
    assert_eq!(
        buf[(clasp_mid, sill_y)].symbol(),
        "✦",
        "{label}: centered ✦ in sill: {joined}"
    );
    assert_eq!(buf[(clasp_mid, sill_y)].fg, palette.accent);
    // Bulbs repeat mirror-symmetrically every 3 cells outward from `╡ ✦ ╞`.
    let max_sym = (clasp_x - (area.x + 1)).min((area.x + area.width - 1) - clasp_right);
    for d in 1..=max_sym {
        let lx = clasp_x - d;
        let rx = clasp_right - 1 + d;
        assert_eq!(
            buf[(lx, sill_y)].symbol(),
            buf[(rx, sill_y)].symbol(),
            "{label}: sill mirror symmetry at d={d}: {joined}"
        );
        if d % 3 == 0 {
            assert_eq!(buf[(lx, sill_y)].symbol(), "▪", "{label}: bulb at d={d}");
            assert_eq!(buf[(lx, sill_y)].fg, elev.bulb_warm);
        }
    }
}

#[test]
fn babylon_dock_renders_across_all_sizes_and_focuses_preserving_sill_rails_crossbars_and_cjk() {
    let summary = snapshot_fixture();
    let list = machines(Some(&summary));
    let palette = Palette::backlot();
    let status: &crate::hollywood::sheet::Status = &stage_up;

    // Talk text width must stay identical to pre-Babylon calculations.
    assert_eq!(talk_text_width(200, "hollywood"), 173);
    assert_eq!(talk_text_width(120, "hollywood"), 93);
    assert_eq!(talk_text_width(90, "hollywood"), 63);
    assert_eq!(talk_text_width(80, "hollywood"), 59);
    assert_eq!(talk_text_width(60, "hollywood"), 39);

    let sizes: [(u16, u16, bool); 6] = [
        (200, 44, false),
        (174, 19, false),
        (120, 28, false),
        (100, 17, false),
        (80, 24, false),
        (60, 16, true),
    ];

    for (w, h, mobile) in sizes {
        let area = Rect::new(0, 0, w, h);
        let opts = LayerOpts {
            agent: true,
            mobile,
            suppress_highlights: false,
            no_shadow: true,
        };
        for (focus_name, selected, editing) in [
            ("WAITING", Target::Card("c1372".into()), false),
            ("HOLLYWOOD", Target::Input, true),
            (
                "STUDIOS",
                Target::Studio(StudioKey::local("acme-shop")),
                false,
            ),
        ] {
            let mut hw = Hollywood::new();
            hw.mode = LayerMode::Expanded;
            hw.selected = Some(selected);
            hw.editing = editing;
            hw.active_studio = Some(StudioKey::local("acme-shop"));
            hw.now_ms = Some(1_000_000);

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
            let drawn = visual_lines(&buf, Rect::new(0, 0, w, hits.area.height));
            let text = drawn.join("\n");

            if std::env::var_os("BABYLON_DUMP").is_some() && matches!((w, h), (120, 28)) {
                eprintln!(
                    "=== BABYLON {w}x{h} {focus_name} (h={}) ===\n{text}\n=== END ===",
                    hits.area.height
                );
            }
            if (w, h) == (120, 28) {
                let expected: &[&str] = match focus_name {
                    "WAITING" => &[
                        "╔═╡ ‹✦› H O L L Y W O O D ‹✦› ╞═ ● live · claude / claude-opus-5-5 / high ═══════════════╡ 6 studios · 3c · ✓2 ready ╞═╗",
                        "▌▸▎ ◆ CARD c1372  acme-shop · recast Ⓟ → agy/gemini-3-pro/high                          a apply   x cancel  ‹ ↵ show › ║",
                        "▌ ▎   └─ why: producer hit quota limit                                                                                 ║",
                        "▌ ▎ ◆ CARD c1373  acme-shop · recast Ⓓ → agy/gemini-3-pro/high                                                         ║",
                        "▌ ▎ ◆ CARD c1374  acme-shop · recast Ⓦ → agy/gemini-3-pro/high                                                         ║",
                        "▌ ▎ ✓ READY acme-shop · 12/13 tasks accepted                                                                           ║",
                        "▌ ▎ ✓ READY beta-api · 13/13 tasks accepted                                                                            ║",
                        "╟── HOLLYWOOD ─────────────────────────────────────────────────────────────────────────────────────── 3m · 3 earlier ──╢",
                        "║ ✦ hollywood  acme-shop 全部 12 项任务已验收通过；已提三张换角卡 c1372、c1373、c1374 等你确认，如有疑问可按 o 查看。  ║",
                        "╟─┤ › tell hollywood…                                                                                                ├─╢",
                        "╟─ STUDIOS 6 ─ [▸acme-shop ✓ 3c] [beta-api ✓] [gamma-web ● 1/4] [delta-ops ● 2/5] [eps-ui ● 0/3] ────────── › [+ new] ─╢",
                        "║ a/x card  i tell  tab ←→ ↑↓ ⏎  m menu  ? help  esc                                                                   ║",
                        "╚══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══╡ ✦ ╞══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪╝",
                    ],
                    "HOLLYWOOD" => &[
                        "╔═╡ ‹✦› H O L L Y W O O D ‹✦› ╞═ ● live · claude / claude-opus-5-5 / high ═══════════════╡ 6 studios · 3c · ✓2 ready ╞═╗",
                        "║ ▎ ◆ CARD c1372  acme-shop · recast Ⓟ → agy/gemini-3-pro/high                                                         ║",
                        "║ ▎ ◆ CARD c1373  acme-shop · recast Ⓓ → agy/gemini-3-pro/high                                                         ║",
                        "║ ▎ ◆ CARD c1374  acme-shop · recast Ⓦ → agy/gemini-3-pro/high                                                         ║",
                        "║ ▎ ✓ READY acme-shop · 12/13 tasks accepted                                                                           ║",
                        "║ ▎ ✓ READY beta-api · 13/13 tasks accepted                                                                            ║",
                        "╠══╡ ‹✦› H O L L Y W O O D ‹✦› ╞═══════════════════════════════════════════════════════════════╡ 2 earlier · ↵ read ╞══╣",
                        "▌ › you        帮我盯一下 acme-shop 的同步与测试                                                                    5m ║",
                        "▌ ✦ hollywood  acme-shop 全部 12 项任务已验收通过；已提三张换角卡 c1372、c1373、c1374 等你确认，如有疑问可按        3m ║",
                        "▌              o 查看。                                                                                                ║",
                        "╠═╡ › tell hollywood…                                                                                                ╞═╣",
                        "╟─ STUDIOS 6 ─ [▸acme-shop ✓ 3c] [beta-api ✓] [gamma-web ● 1/4] [delta-ops ● 2/5] [eps-ui ● 0/3] ────────── › [+ new] ─╢",
                        "║ ⏎ send  ↑↓ history  tab next zone  esc rows  alt+y close                                                             ║",
                        "╚══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══╡ ✦ ╞══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪╝",
                    ],
                    "STUDIOS" => &[
                        "╔═╡ ‹✦› H O L L Y W O O D ‹✦› ╞═ ● live · claude / claude-opus-5-5 / high ═══════════════╡ 6 studios · 3c · ✓2 ready ╞═╗",
                        "║ ▎ ◆ CARD c1372  acme-shop · recast Ⓟ → agy/gemini-3-pro/high                                                         ║",
                        "║ ▎ ◆ CARD c1373  acme-shop · recast Ⓓ → agy/gemini-3-pro/high                                                         ║",
                        "║ ▎ ◆ CARD c1374  acme-shop · recast Ⓦ → agy/gemini-3-pro/high                                                         ║",
                        "║ ▎ ✓ READY acme-shop · 12/13 tasks accepted                                                                           ║",
                        "║ ▎ ✓ READY beta-api · 13/13 tasks accepted                                                                            ║",
                        "║ ✦ hollywood  acme-shop 全部 12 项任务已验收通过；已提三张换角卡 c1372、c1373、c1374 等你确认，如有…   3m · 3 earlier ║",
                        "╟─┤ › tell hollywood…                                                                                                ├─╢",
                        "╠═▄▄══▄▄══▄▄══▄▄══▄▄══▄▄══▄▄══▄▄══▄▄══▄▄══▄▄═╡ ‹✦› S T U D I O S · 6 ‹✦› ╞═▄▄══▄▄══▄▄══▄▄══▄▄══▄▄══▄▄══▄▄══▄▄══▄▄══▄▄══╣",
                        "▌ ╔═ ▸ acme-shop ═══════════╗  ╭─ beta-api ──────────────╮  ╭─ gamma-web ─────────────╮  ╭─ delta-ops ─────────────╮  ›║",
                        "▌ ║ ✓ ready      r2 · 12/13 ║  │ ✓ ready      r1 · 13/13 │  │ ● live         r1 · 1/4 │  │ ● live         r1 · 2/5 │   ║",
                        "▌ ╚═════════════════════════╝  ╰─────────────────────────╯  ╰─────────────────────────╯  ╰─────────────────────────╯   ║",
                        "║ f finish  p peek  i tell  tab ←→ ↑↓ ⏎  m menu  ? help  esc                                                           ║",
                        "╚══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══╡ ✦ ╞══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪╝",
                    ],
                    _ => unreachable!(),
                };
                assert_eq!(
                    drawn, expected,
                    "120x28 {focus_name} snapshot mismatch:\n{text}"
                );
            }

            // Title width ladder.
            let title = &drawn[0];
            if w >= 120 {
                assert!(
                    title.contains("╔═╡ ‹✦› H O L L Y W O O D ‹✦› ╞═"),
                    "{w}x{h} {focus_name}: {title}"
                );
                assert!(title.ends_with("╞═╗"), "{w}x{h} {focus_name}: {title}");
            } else if w >= 100 {
                assert!(
                    title.contains("╔═╡ ✦ H O L L Y W O O D ╞═"),
                    "{w}x{h} {focus_name}: {title}"
                );
                assert!(title.ends_with("╞═╗"), "{w}x{h} {focus_name}: {title}");
            } else {
                assert!(
                    title.contains("╔╡✦ HOLLYWOOD╞═"),
                    "{w}x{h} {focus_name}: {title}"
                );
                assert!(title.ends_with("╞╗"), "{w}x{h} {focus_name}: {title}");
            }

            // Bottom marquee sill on every size and focus.
            assert_sill_and_rails(
                &buf,
                area,
                &palette,
                hits.area.bottom() - 1,
                &format!("{w}x{h} {focus_name}"),
            );

            // Right rail stays `║` on body/hints rows (`╟`/`╠`/`╢`/`╣` on crossbars).
            for y in 1..hits.area.bottom() - 1 {
                let left = buf[(0, y)].symbol();
                let right = buf[(w - 1, y)].symbol();
                assert!(
                    matches!(left, "║" | "▌" | "╟" | "╠"),
                    "{w}x{h} {focus_name} y={y} left={left:?}\n{text}"
                );
                assert!(
                    matches!(right, "║" | "╢" | "╣"),
                    "{w}x{h} {focus_name} y={y} right={right:?}\n{text}"
                );
            }

            // Compose crossbar joins both rails.
            let cy = hits.input.y;
            let crow = &drawn[cy as usize];
            if editing {
                assert!(crow.starts_with("╠═╡ › "), "{w}x{h} {focus_name}: {crow}");
                assert!(crow.ends_with("╞═╣"), "{w}x{h} {focus_name}: {crow}");
            } else {
                assert!(crow.starts_with("╟─┤ › "), "{w}x{h} {focus_name}: {crow}");
                assert!(crow.ends_with("├─╢"), "{w}x{h} {focus_name}: {crow}");
            }

            // Focus-specific crossbars, rails & CJK wrapping.
            match focus_name {
                "WAITING" => {
                    assert!(hits.strip);
                    let wait_y = hits
                        .rows
                        .iter()
                        .find(|(_, t)| matches!(t, Target::Card(_)))
                        .unwrap()
                        .0
                        .y;
                    assert_eq!(buf[(0, wait_y)].symbol(), "▌");
                    assert_eq!(buf[(0, wait_y)].fg, palette.accent);
                    assert!(text.contains("└─ why: producer hit quota limit"), "{text}");
                    let strip_row = &drawn[hits.body.y as usize];
                    assert!(
                        strip_row.starts_with("╟─ STUDIOS 6 ─ ") && strip_row.ends_with(" ─╢"),
                        "{w}x{h}: {strip_row}"
                    );
                }
                "HOLLYWOOD" => {
                    assert!(hits.strip);
                    assert!(
                        text.contains("╠")
                            && (text.contains("H O L L Y W O O D") || text.contains("HOLLYWOOD")),
                        "{text}"
                    );
                    let t_y = hits
                        .rows
                        .iter()
                        .find(|(_, t)| *t == Target::Talk(10))
                        .unwrap()
                        .0
                        .y;
                    assert_eq!(buf[(0, t_y)].symbol(), "▌");
                    // CJK talk text renders cleanly without losing content width.
                    assert!(text.contains("acme-shop 全部"), "{text}");
                }
                "STUDIOS" => {
                    if !hits.strip {
                        assert!(text.contains("▄▄"), "{w}x{h} sprocket:\n{text}");
                        assert!(
                            text.contains("╔═ ▸ acme-shop") && text.contains("╭─ beta-api"),
                            "{w}x{h} frames:\n{text}"
                        );
                    } else {
                        let strip_row = &drawn[hits.body.y as usize];
                        assert!(strip_row.starts_with("▌─ STUDIOS 6 ─ "), "{strip_row}");
                    }
                }
                _ => unreachable!(),
            }
        }
    }
}

#[test]
fn short_terminal_24_rows_8_waits_blocking_prompt_keeps_sill_and_red_tally() {
    let summary = short_blocking_fixture();
    let list = machines(Some(&summary));
    let palette = Palette::backlot();
    let status: &crate::hollywood::sheet::Status = &stage_up;

    for width in [200u16, 80] {
        let area = Rect::new(0, 0, width, 24);
        let opts = LayerOpts {
            agent: true,
            mobile: false,
            suppress_highlights: false,
            no_shadow: true,
        };
        let prompt_target = Target::Prompt(StudioKey::local("gamma-web"), "producer".into());

        // 1. WAITING focused on blocking prompt: crimson `◉` tally + `!1` + `‹ y allow ›` (`‹y›`) + sill preserved at row 11.
        let mut hw_wait = Hollywood::new();
        hw_wait.mode = LayerMode::Expanded;
        hw_wait.selected = Some(prompt_target);
        hw_wait.now_ms = Some(1_000_000);
        let mut buf_w = Buffer::empty(area);
        let hits_w = render(
            &mut buf_w,
            area,
            &list,
            Some("Local"),
            &status,
            &hw_wait,
            opts,
            &palette,
        );
        assert_eq!(hits_w.area.height, 12);
        let lines_w = visual_lines(&buf_w, Rect::new(0, 0, width, hits_w.area.height));
        let text_w = lines_w.join("\n");

        if width == 80 {
            assert_eq!(
                lines_w,
                &[
                    "╔╡✦ HOLLYWOOD╞═ ◉ !1 · live ═════════════════╡ 6 studios · !1 · 5c · ✓2 ready ╞╗",
                    "▌▸▎ × PROMPT #9 gamma-web · Ⓟ           ‹ y allow ›  d deny   p peek   ↵ open  ║",
                    "▌ ▎ ◆ CARD c1 acme-shop · recast Ⓟ → agy/gemini-3-pro/high                     ║",
                    "▌ ▎ ◆ CARD c2 acme-shop · recast Ⓓ → agy/gemini-3-pro/high                     ║",
                    "▌ ▎ ◆ CARD c3 acme-shop · recast Ⓦ → agy/gemini-3-pro/high                     ║",
                    "▌ ▎ ◆ CARD c4 delta-ops · priority t4 → high                                   ║",
                    "▌ ▎ ◆ CARD c5 eps-ui · pause t2                                                ║",
                    "▌ ▎ ✓ READY acme-shop · 12/13 tasks accepted                                   ║",
                    "▌ ▎ ✓ READY beta-api · 13/13 tasks accepted                                    ║",
                    "╟─┤ › tell hollywood…                                                        ├─╢",
                    "╟─ STUDIOS 6 ─ [acme-shop ✓ 3c] [beta-api ✓] [gamma-web ● !1] ───── › [+ new] ─╢",
                    "╚▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══╡ ✦ ╞══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪═╝",
                ]
            );
        }

        assert!(lines_w[0].contains("◉ !1 · "), "{width}: {}", lines_w[0]);
        let tally_x = (0..width).find(|&x| buf_w[(x, 0)].symbol() == "◉").unwrap();
        assert_eq!(buf_w[(tally_x, 0)].fg, palette.red);
        assert!(buf_w[(tally_x, 0)].modifier.contains(Modifier::BOLD));

        assert!(text_w.contains("‹ y allow ›"), "{text_w}");
        assert_sill_and_rails(
            &buf_w,
            area,
            &palette,
            hits_w.area.bottom() - 1,
            &format!("{width}x24 WAITING"),
        );

        // 2. HOLLYWOOD focused (`Target::Input`): WAITING scrolls to `1–7/8` in Title while keeping sill & crimson tally.
        let mut hw_talk = Hollywood::new();
        hw_talk.mode = LayerMode::Expanded;
        hw_talk.selected = Some(Target::Input);
        hw_talk.now_ms = Some(1_000_000);
        let mut buf_h = Buffer::empty(area);
        let hits_h = render(
            &mut buf_h,
            area,
            &list,
            Some("Local"),
            &status,
            &hw_talk,
            opts,
            &palette,
        );
        assert_eq!(hits_h.area.height, 12);
        let lines_h = visual_lines(&buf_h, Rect::new(0, 0, width, hits_h.area.height));
        if width == 80 {
            assert_eq!(
                lines_h,
                &[
                    "╔╡✦ HOLLYWOOD╞═ ◉ !1 · live ═════════╡ 6 studios · !1 · 5c · ✓2 ready · 1–7/8 ╞╗",
                    "║ ▎ × PROMPT #9 gamma-web · Ⓟ                                                  ║",
                    "║ ▎ ◆ CARD c1 acme-shop · recast Ⓟ → agy/gemini-3-pro/high                     ║",
                    "║ ▎ ◆ CARD c2 acme-shop · recast Ⓓ → agy/gemini-3-pro/high                     ║",
                    "║ ▎ ◆ CARD c3 acme-shop · recast Ⓦ → agy/gemini-3-pro/high                     ║",
                    "║ ▎ ◆ CARD c4 delta-ops · priority t4 → high                                   ║",
                    "║ ▎ ◆ CARD c5 eps-ui · pause t2                                                ║",
                    "║ ▎ ✓ READY acme-shop · 12/13 tasks accepted                                   ║",
                    "▌ ✦ hw   acme-shop 全部 12 项任务已验收通过；已提三张换角卡 c…  3m · 3 earlier ║",
                    "╟─┤ › tell hollywood…                                                        ├─╢",
                    "╟─ STUDIOS 6 ─ [acme-shop ✓ 3c] [beta-api ✓] [gamma-web ● !1] ───── › [+ new] ─╢",
                    "╚▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══╡ ✦ ╞══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪══▪═╝",
                ]
            );
        }
        assert!(lines_h[0].contains("◉ !1 · "), "{width}: {}", lines_h[0]);
        assert!(lines_h[0].contains("1–7/8"), "{width}: {}", lines_h[0]);
        assert_sill_and_rails(
            &buf_h,
            area,
            &palette,
            hits_h.area.bottom() - 1,
            &format!("{width}x24 HOLLYWOOD"),
        );
    }
}
