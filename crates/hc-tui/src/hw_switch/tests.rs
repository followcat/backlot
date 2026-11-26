//! Draw & behavior tests for [`HwSwitch`].

use backlot_stage::config::HollywoodCfg;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::{HwSwitch, render};
use crate::chrome::palette::Palette;
use crate::new_studio::{Probe, RunnerRow};

fn frozen_screen(buf: &mut Buffer) {
    let style = Style::default()
        .fg(Color::Red)
        .add_modifier(Modifier::REVERSED | Modifier::UNDERLINED);
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            buf[(x, y)].set_symbol("#").set_style(style);
        }
    }
}

fn assert_no_bleed(buf: &Buffer, area: Rect) {
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let cell = &buf[(x, y)];
            assert!(
                !cell
                    .modifier
                    .intersects(Modifier::REVERSED | Modifier::UNDERLINED),
                "foreign modifier at ({x}, {y}): {:?}",
                cell.modifier
            );
            assert_ne!(cell.fg, Color::Red, "foreign colour at ({x}, {y})");
            assert_ne!(cell.symbol(), "#", "frozen glyph at ({x}, {y})");
        }
    }
}

#[test]
fn the_dialog_clears_the_frozen_screen_behind_it() {
    let band = Rect::new(0, 0, 80, 24);
    let mut buf = Buffer::empty(band);
    frozen_screen(&mut buf);
    let palette = Palette::catppuccin();
    let hits = render(&mut buf, &HwSwitch::new(None, false), &palette, band);
    assert!(hits.area.width > 0, "the frame must fit the band");
    assert_no_bleed(&buf, hits.area);
}

#[test]
fn the_footer_names_whole_hint_tiers_never_a_mid_word_cut() {
    let palette = Palette::catppuccin();
    let tiers = [
        "↑↓ field  ←→ value  ⏎ list · apply  e text  esc cancel",
        "↑↓ field  ←→ value  ⏎ apply  esc",
        "↑↓ · ←→ · ⏎ · esc",
    ];
    for width in [80u16, 64, 48, 36] {
        let band = Rect::new(0, 0, width, 24);
        let mut buf = Buffer::empty(band);
        let hits = render(&mut buf, &HwSwitch::new(None, false), &palette, band);
        let text = (hits.area.y..hits.area.bottom())
            .map(|y| {
                (hits.area.x..hits.area.right())
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        let footers: Vec<&str> = text.lines().filter(|line| line.contains("↑↓")).collect();
        assert!(
            footers.len() <= 1,
            "{width}: at most one hint line: {footers:?}"
        );
        if let Some(footer) = footers.first() {
            assert!(
                tiers.iter().any(|t| footer.contains(t)),
                "{width}: {footer:?}"
            );
            assert!(!footer.contains('…'), "{width}: clipped: {footer:?}");
        }
    }
}

#[test]
fn switch_picker_prefills_current_highlights_alternative_and_detects_unchanged() {
    let cfg = HollywoodCfg {
        runner: "claude".into(),
        model: "claude-sonnet-5-5".into(),
        effort: Some("high".into()),
        key: None,
        default_cast: None,
    };
    let rows = vec![
        RunnerRow {
            runner: "claude".into(),
            status: "found".into(),
            version: Some("1".into()),
            path: Some("/usr/bin/claude".into()),
            needs_path: false,
            models: vec!["claude-sonnet-5-5".into(), "claude-opus-5-5".into()],
            efforts: vec!["low".into(), "high".into()],
        },
        RunnerRow {
            runner: "codex".into(),
            status: "found".into(),
            version: Some("1".into()),
            path: Some("/usr/bin/codex".into()),
            needs_path: false,
            models: vec!["gpt-6-sol".into()],
            efforts: vec!["high".into()],
        },
    ];
    let mut normal = HwSwitch::new(Some(cfg.clone()), false);
    normal.runners = Probe::Done(rows.clone());
    normal.prefill();
    assert_eq!(normal.runner_row().unwrap().runner, "claude");
    assert_eq!(normal.model, "claude-sonnet-5-5");
    assert!(normal.unchanged());

    let mut quota = HwSwitch::new(Some(cfg), true);
    quota.runners = Probe::Done(rows);
    quota.prefill();
    assert_eq!(quota.runner_row().unwrap().runner, "codex");
    assert_eq!(quota.model, "auto");
    assert!(!quota.unchanged());
}

#[test]
fn popup_chrome_standard_renders_gutter_rounded_corners_and_drop_shadow() {
    let band = Rect::new(0, 0, 80, 24);
    let mut buf = Buffer::empty(band);
    frozen_screen(&mut buf);
    let palette = Palette::catppuccin();
    let elev = palette.elevation();
    let shadow_bg = elev.shadow.expect("catppuccin has drop-shadow color");
    let frame_bg = crate::chrome::popup_frame::overlay_bg(&palette);

    let hits =
        super::render_with_shadow(&mut buf, &HwSwitch::new(None, false), &palette, band, true);
    let outer = hits.area;
    assert!(outer.width >= 28 && outer.height >= 10);
    assert!(outer.right() < band.right() && outer.bottom() < band.bottom());

    // Outer 1-cell gutter uses overlay_bg, with rounded frame immediately inside it.
    assert_eq!(buf[(outer.x, outer.y)].symbol(), " ");
    assert_eq!(buf[(outer.x, outer.y)].bg, frame_bg);
    assert_eq!(buf[(outer.x + 1, outer.y + 1)].symbol(), "╭");
    assert_eq!(buf[(outer.right() - 2, outer.y + 1)].symbol(), "╮");
    assert_eq!(buf[(outer.x + 1, outer.bottom() - 2)].symbol(), "╰");
    assert_eq!(buf[(outer.right() - 2, outer.bottom() - 2)].symbol(), "╯");

    // 1-cell drop shadow sits outside geom.outer (bottom and right, excluding top-right/bottom-left corners).
    for y in (outer.y + 1)..=outer.bottom() {
        assert_eq!(
            buf[(outer.right(), y)].bg,
            shadow_bg,
            "right shadow at y={y}"
        );
    }
    for x in (outer.x + 1)..=outer.right() {
        assert_eq!(
            buf[(x, outer.bottom())].bg,
            shadow_bg,
            "bottom shadow at x={x}"
        );
    }
    assert_ne!(buf[(outer.right(), outer.y)].bg, shadow_bg);
    assert_ne!(buf[(outer.x, outer.bottom())].bg, shadow_bg);
}
