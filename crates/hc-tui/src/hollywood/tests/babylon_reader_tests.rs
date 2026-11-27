//! S3 tests: Babylon intertitle Reader frame (`>= 64` stepped brass & `< 64`
//! collapse), letter-spaced vs plain Design title, Markdown `intertitle`
//! banners, `gilt` `║` margin bars, CJK right-rail alignment, hit rects, and
//! Plain-mode preservation.

use ratatui::buffer::{Buffer, CellWidth};
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

use super::draw_tests::lines;
use crate::chrome::palette::Palette;
use crate::hollywood::{
    CardView, ReaderKind, ReaderMeta, ReaderOrigin, ReaderRenderOpts, render_view_with_opts,
};
use crate::studio::StudioKey;
use backlot_model::design::{DesignPhase, SignMode};

fn babylon_design_lines() -> Vec<String> {
    [
        "# Design v2 · acme-shop",
        "",
        "## R1",
        "",
        "### §R1.W · Proposal",
        "",
        "Baseline proposal context spanning multiple lines before round two so scroll range footer metrics populate cleanly across medium and compact viewports.",
        "",
        "### §R1.P · Summary",
        "",
        "Round one aligned on the single-column chrome layout and section navigation.",
        "",
        "## R2",
        "",
        "### §R2.D · Layout Architecture",
        "",
        "需要仔细核对中文断行宽度与右边框对齐情况，不提前截断，保留 crates/hc-tui/src/hollywood/reader_wrap.rs。",
        "",
        "- **Q1**: 如何组织工作室分栏与导航宽度？",
        "> 摘要引用块保留原有文字内容宽度。",
        "Q2 · Open — Keep prose measure identical?",
        "  → Option A: keep exact body width and chrome rows",
        "",
        "## Sign-off",
        "",
        "Approved by user.",
        "",
        "### §R2.U · Final confirmation notes",
        "",
        "Keep footer scroll range populated on tall views so both stepped footer borders and margin bars appear together cleanly.",
        "Closing note one.",
        "Closing note two.",
        "Closing note three.",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn babylon_design_view(studio: &str, scroll: usize) -> CardView {
    let raw = babylon_design_lines();
    CardView {
        card: None,
        title: "Design".into(),
        raw_body: raw.join("\n"),
        lines: raw,
        scroll,
        meta: ReaderMeta {
            kind: ReaderKind::Design,
            studio: Some(studio.into()),
            file: Some("design/v2.md".into()),
            design_version: Some(2),
            design_round: Some(2),
            design_phase: Some(DesignPhase::WaitingUser),
            design_sign_mode: None,
            origin: ReaderOrigin::Design(StudioKey::local(studio)),
            ..ReaderMeta::default()
        },
        ..CardView::default()
    }
}

fn border_rect(buf: &Buffer, outer: Rect) -> Rect {
    if outer.width >= 4 && outer.height >= 4 && buf[(outer.x, outer.y)].symbol() == " " {
        Rect::new(outer.x + 1, outer.y + 1, outer.width - 2, outer.height - 2)
    } else {
        outer
    }
}

fn wide_row(buf: &Buffer, x0: u16, x1: u16, y: u16) -> String {
    let mut s = String::new();
    let mut x = x0;
    while x < x1 {
        let cell = &buf[(x, y)];
        let w = cell.cell_width().max(1);
        s.push_str(cell.symbol());
        x += w;
    }
    s
}

#[test]
fn babylon_reader_snapshots_preserve_measure_cjk_banners_q_bars_and_hits() {
    let palette = Palette::catppuccin();

    for (w, h) in [(94u16, 30u16), (78u16, 24u16), (52u16, 18u16)] {
        let view = babylon_design_view("acme-shop", if w == 52 { 18 } else { 14 });
        let area = Rect::new(0, 0, w, h);
        let mut bab_buf = Buffer::empty(area);
        let mut plain_buf = Buffer::empty(area);

        let bab = render_view_with_opts(
            &mut bab_buf,
            area,
            &view,
            None,
            &palette,
            ReaderRenderOpts {
                compose: None,
                now_ms: 0,
                shadow: false,
                plain: false,
            },
        );
        let plain = render_view_with_opts(
            &mut plain_buf,
            area,
            &view,
            None,
            &palette,
            ReaderRenderOpts {
                compose: None,
                now_ms: 0,
                shadow: false,
                plain: true,
            },
        );

        let b_bab = border_rect(&bab_buf, bab.area);
        let b_plain = border_rect(&plain_buf, plain.area);

        // 0 extra chrome rows (always 6: 3 top + 3 bottom) and 0 lost body columns.
        assert_eq!(bab.body.y - b_bab.y, 3, "{w}x{h} top chrome rows");
        assert_eq!(
            b_bab.bottom() - bab.body.bottom(),
            3,
            "{w}x{h} bottom chrome rows"
        );
        assert_eq!(bab.body.width, plain.body.width, "{w}x{h} body width");
        assert_eq!(b_bab.width, b_plain.width, "{w}x{h} frame width");

        // Top / context / divider / action / bottom rows.
        let top = wide_row(&bab_buf, b_bab.x, b_bab.right(), b_bab.y);
        let ctx = wide_row(&bab_buf, b_bab.x, b_bab.right(), b_bab.y + 1);
        let div1 = wide_row(&bab_buf, b_bab.x, b_bab.right(), b_bab.y + 2);
        let div2 = wide_row(&bab_buf, b_bab.x, b_bab.right(), b_bab.bottom() - 3);
        let action = wide_row(&bab_buf, b_bab.x, b_bab.right(), b_bab.bottom() - 2);
        let bottom = wide_row(&bab_buf, b_bab.x, b_bab.right(), b_bab.bottom() - 1);

        if b_bab.width >= 64 {
            let expected_head = if w >= 94 {
                "╔══╤═ ✦  D E S I G N   v 2  ·  R 2  ·  acme-shop"
            } else {
                "╔══╤═ ✦ DESIGN v2 · R2 · acme-shop"
            };
            assert!(
                top.starts_with(expected_head) && top.ends_with("═╤══╗"),
                "{w}x{h} stepped top: {top}"
            );
            assert!(
                ctx.starts_with("║  │ ") && ctx.ends_with(" │  ║"),
                "{w}x{h} stepped ctx: {ctx}"
            );
            assert!(
                div1.starts_with("╟──┴") && div1.ends_with("┴──╢"),
                "{w}x{h} div1: {div1}"
            );
            assert!(
                div2.starts_with("╟──┬") && div2.ends_with("┬──╢"),
                "{w}x{h} div2: {div2}"
            );
            assert!(
                action.starts_with("║  │ ") && action.ends_with(" │  ║"),
                "{w}x{h} action: {action}"
            );
            assert!(
                action.contains(" r reply ") && !action.contains("‹ r reply ›"),
                "{action}"
            );
            assert!(
                bottom.starts_with("╚══╧═ lines ") && bottom.ends_with("╧══╝"),
                "{w}x{h} bottom: {bottom}"
            );
        } else {
            assert!(
                top.starts_with("╔═ ") && top.ends_with("═╗"),
                "{w}x{h} collapsed top: {top}"
            );
            assert!(
                top.contains("✦ DESIGN v2"),
                "{w}x{h} plain title fallback: {top}"
            );
            assert!(div1.starts_with("╟─") && div1.ends_with("─╢"), "{div1}");
            assert!(div2.starts_with("╟─") && div2.ends_with("─╢"), "{div2}");
            assert!(
                bottom.starts_with("╚═") && bottom.ends_with("╝"),
                "{bottom}"
            );
            for ch in ['╤', '┴', '┬', '╧'] {
                assert!(!top.contains(ch) && !div1.contains(ch) && !bottom.contains(ch));
            }
        }

        // Hit rects: ◀ / ▶ and chip hit rects.
        let prev = bab.prev_hit.expect("prev_hit");
        let next = bab.next_hit.expect("next_hit");
        assert_eq!(bab_buf[(prev.x, prev.y)].symbol(), "◀");
        assert_eq!(bab_buf[(next.x, next.y)].symbol(), "▶");
        assert!(bab.chips.iter().any(|(_, c)| *c == 'r'));
        assert!(bab.chips.iter().any(|(_, c)| *c == 'a'));
        assert!(bab.chips.iter().any(|(_, c)| *c == 'y'));
        for (rect, key) in &bab.chips {
            assert_eq!(rect.y, b_bab.bottom() - 2);
            let txt = wide_row(&bab_buf, rect.x, rect.right(), rect.y);
            assert!(txt.contains(*key), "chip hit {key} mismatch: {txt:?}");
        }

        // CJK wrap alignment against right rail `║`.
        crate::chrome::wide::mark_wide_tail_slots(&mut bab_buf);
        for y in bab.body.y..bab.body.bottom() {
            assert_eq!(
                bab_buf[(b_bab.x, y)].symbol(),
                "║",
                "{w}x{h} left rail row {y}"
            );
            assert_eq!(
                bab_buf[(b_bab.right() - 1, y)].symbol(),
                "║",
                "{w}x{h} right rail row {y}"
            );
            assert_eq!(bab_buf[(b_bab.right() - 1, y)].cell_width(), 1);
            let inner = wide_row(&bab_buf, b_bab.x + 1, b_bab.right() - 1, y);
            assert_eq!(
                UnicodeWidthStr::width(inner.as_str()),
                usize::from(b_bab.width - 2),
                "{w}x{h} CJK inner width mismatch on row {y}: {inner:?}"
            );
        }

        // Section banners (`§R2.D · Layout Architecture`) + gilt `║` Q/quote bars visible when scrolled.
        let body_text: Vec<String> = (bab.body.y..bab.body.bottom())
            .map(|y| wide_row(&bab_buf, bab.body.x, bab.body.right(), y))
            .collect();
        let body_joined = body_text.join("\n");
        if w >= 78 {
            assert!(
                body_joined.contains("───══╡ ‹✦› §R2.D · Layout Architecture ‹✦› ╞══───"),
                "{w}x{h} full chevron banner:\n{body_joined}"
            );
        } else {
            assert!(
                body_joined.contains("══╡ ✦ §R2.D · Layout Architecture ╞══")
                    || body_joined.contains("╡ ✦ §R2.D · Layout Architecture ╞"),
                "{w}x{h} narrow ladder banner:\n{body_joined}"
            );
        }
        let q1_y = (bab.body.y..bab.body.bottom())
            .find(|&y| wide_row(&bab_buf, bab.body.x, bab.body.right(), y).contains("Q1"))
            .expect("Q1 row visible");
        let text_x = b_bab.x + if w >= 56 { 3 } else { 2 };
        assert_eq!(bab_buf[(text_x, q1_y)].symbol(), "║");
        assert_eq!(bab_buf[(text_x, q1_y)].fg, palette.elevation().gilt);
        assert!(
            body_joined.contains("║ 摘要引用块保留原有文字内容宽度。"),
            "{w}x{h} quote bar:\n{body_joined}"
        );

        if std::env::var_os("BABYLON_READER_DUMP").is_some() && (w == 78 || w == 52) {
            eprintln!("=== BABYLON READER {w}x{h} ===");
            for row in lines(&bab_buf, area) {
                eprintln!("{row}");
            }
        }
    }
}

#[test]
fn babylon_reader_64col_long_studio_falls_back_to_compact_title_in_stepped_frame() {
    let palette = Palette::catppuccin();
    let mut view = babylon_design_view("acme-shop-long-studio-name", 0);
    view.meta.design_sign_mode = Some(SignMode::Skipped);
    let area = Rect::new(0, 0, 66, 20);
    let mut buf = Buffer::empty(area);
    let vr = render_view_with_opts(
        &mut buf,
        area,
        &view,
        None,
        &palette,
        ReaderRenderOpts {
            compose: None,
            now_ms: 0,
            shadow: false,
            plain: false,
        },
    );
    let b = border_rect(&buf, vr.area);
    assert_eq!(b.width, 64);
    let top = wide_row(&buf, b.x, b.right(), b.y);
    assert!(
        top.starts_with("╔══╤═ ✦ DESIGN v2 · R2"),
        "compact title fallback: {top}"
    );
    assert!(top.ends_with("═╤══╗"), "still stepped at 64: {top}");
}

#[test]
fn babylon_reader_78x24_fits_letter_spaced_title_with_short_nav_and_scroll_14() {
    let palette = Palette::catppuccin();
    let mut view = babylon_design_view("acme-shop", 14);
    view.meta.design_section_idx = Some(3);
    let area = Rect::new(0, 0, 78, 24);
    let mut buf = Buffer::empty(area);
    let vr = render_view_with_opts(
        &mut buf,
        area,
        &view,
        None,
        &palette,
        ReaderRenderOpts {
            compose: None,
            now_ms: 0,
            shadow: false,
            plain: false,
        },
    );
    let b = border_rect(&buf, vr.area);
    let top = wide_row(&buf, b.x, b.right(), b.y);
    assert!(
        top.starts_with("╔══╤═ ✦  D E S I G N   v 2  ·  R 2  ·  acme-shop")
            && top.ends_with("═╤══╗"),
        "78x24 fits wide title when nav is short: {top}"
    );
}

#[test]
fn plain_mode_reader_is_unchanged() {
    let palette = Palette::catppuccin();
    let view = babylon_design_view("acme-shop", 14);
    let area = Rect::new(0, 0, 78, 24);
    let mut buf = Buffer::empty(area);
    let vr = render_view_with_opts(
        &mut buf,
        area,
        &view,
        None,
        &palette,
        ReaderRenderOpts {
            compose: None,
            now_ms: 0,
            shadow: false,
            plain: true,
        },
    );
    let b = border_rect(&buf, vr.area);
    let full = lines(&buf, area).join("\n");
    assert!(full.contains("╭"), "plain rounded corner:\n{full}");
    assert!(
        full.contains("■ DESIGN v2 · R2 · acme-shop"),
        "plain title:\n{full}"
    );
    assert!(full.contains("[r] reply"), "plain chips:\n{full}");
    assert!(
        !full.contains("╔") && !full.contains("‹✦›"),
        "no Babylon chrome:\n{full}"
    );
    let body_joined: String = (vr.body.y..vr.body.bottom())
        .map(|y| wide_row(&buf, vr.body.x, vr.body.right(), y))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        body_joined.contains("▪ Q1:"),
        "plain Q1 bullet:\n{body_joined}"
    );
    assert!(
        body_joined.contains("> 摘要引用块保留"),
        "plain blockquote:\n{body_joined}"
    );
    assert_eq!(buf[(b.right() - 1, b.y)].symbol(), "╮");
}
