//! Render tests for selected-studio accent frames, title chips,
//! collapsed strip / stacked bold accent labels, narrow chip fit,
//! multi-theme chip foreground contrast, and the 36x30 dump.

use super::fold_tests::two_studio_summary;
use super::*;
use crate::chrome::palette::Palette;
use crate::panel::config::StudioKeys;
use crate::panel::{Panel, PanelMode, Target, render};
use crate::studio::StudioKey;
use crate::studio::glyphs;
use backlot_model::summary::Summary;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

fn draw_cur(
    area: Rect,
    summary: &Summary,
    panel: &Panel,
    cur: Option<&str>,
    palette: &Palette,
) -> (Buffer, crate::panel::PanelRender) {
    let mut buf = Buffer::empty(area);
    let key = cur.map(StudioKey::local);
    let hits = render(
        &mut buf,
        area,
        &machines(Some(summary)),
        |_| true,
        |_| true,
        panel,
        panel.mode,
        key.as_ref(),
        key.as_ref(),
        2,
        None,
        &StudioKeys::default(),
        None,
        false,
        false,
        true,
        &Default::default(),
        palette,
        60_000,
    );
    (buf, hits)
}

fn block_bounds(buf: &Buffer, area: Rect, target_id: &str) -> (u16, u16) {
    let bx = area.x + 1;
    let top_y = (area.y..area.y + area.height)
        .find(|&y| {
            let row: String = (area.x..area.x + area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect();
            (row.contains('╭') || row.contains('▌')) && row.contains(target_id)
        })
        .unwrap_or_else(|| panic!("missing block top for {target_id}"));
    let bot_y = (top_y + 1..area.y + area.height)
        .find(|&y| buf[(bx, y)].symbol() == "╰")
        .unwrap_or_else(|| panic!("missing block bottom for {target_id}"));
    (top_y, bot_y)
}

fn assert_border_fg(
    buf: &Buffer,
    area: Rect,
    top_y: u16,
    bot_y: u16,
    expected_fg: ratatui::style::Color,
    label: &str,
) {
    let bx = area.x + 1;
    let rx = area.x + area.width - 1;
    for y in (top_y + 1)..bot_y {
        // A role-group or input divider joins the border with `├ … ┤`.
        let lsym = buf[(bx, y)].symbol();
        let rsym = buf[(rx, y)].symbol();
        assert!(
            matches!((lsym, rsym), ("│", "│") | ("├", "┤")),
            "{label} side y={y}: {lsym:?} / {rsym:?}"
        );
        assert_eq!(buf[(bx, y)].fg, expected_fg, "{label} left fg y={y}");
        assert_eq!(buf[(rx, y)].fg, expected_fg, "{label} right fg y={y}");
    }
    for x in bx..area.x + area.width {
        assert_eq!(buf[(x, bot_y)].fg, expected_fg, "{label} bottom x={x}");
        if buf[(x, top_y)].symbol() == "─" {
            assert_eq!(buf[(x, top_y)].fg, expected_fg, "{label} top dash x={x}");
        }
    }
    if buf[(bx, top_y)].symbol() == "╭" {
        assert_eq!(buf[(bx, top_y)].fg, expected_fg, "{label} top-left");
    }
    assert_eq!(
        buf[(area.x + area.width - 1, top_y)].fg,
        expected_fg,
        "{label} top-right"
    );
}

fn hit_y(hits: &crate::panel::PanelRender, target: &Target) -> u16 {
    hits.rows
        .iter()
        .find(|(_, t)| t == target)
        .unwrap_or_else(|| panic!("missing hit {target:?}"))
        .0
        .y
}

fn col_of(buf: &Buffer, area: Rect, y: u16, needle: &str) -> u16 {
    let row: String = (area.x..area.x + area.width)
        .map(|x| buf[(x, y)].symbol())
        .collect();
    let byte = row
        .find(needle)
        .unwrap_or_else(|| panic!("missing {needle:?} in {row:?}"));
    area.x + crate::chrome::text::display_width(&row[..byte]) as u16
}

#[test]
fn unfocused_lot_demotes_frame_and_title_while_focused_lot_accents_them() {
    let summary = two_studio_summary();
    let palette = Palette::from_name("catppuccin").unwrap();
    let elev = palette.elevation();
    let mut panel = Panel {
        mode: PanelMode::Expanded,
        focused: false,
        ..Panel::new()
    };
    let area = Rect::new(0, 0, 37, 30);

    // Unfocused lot, `current = acme-shop`: single focus ring keeps both borders `border_muted`,
    // no inverted `accent` chip, no `▌`; active terminal studio uses bold `text` `▸ acme-shop`.
    let (buf, _) = draw_cur(area, &summary, &panel, Some("acme-shop"), &palette);
    for y in area.y..area.bottom() {
        assert_eq!(buf[(area.x, y)].symbol(), "│", "dock left seam y={y}");
        assert_eq!(buf[(area.x, y)].fg, elev.border_muted, "dock seam fg y={y}");
    }
    let (acme_top, acme_bot) = block_bounds(&buf, area, "acme-shop");
    let (blog_top, blog_bot) = block_bounds(&buf, area, "blog");

    assert_border_fg(
        &buf,
        area,
        acme_top,
        acme_bot,
        elev.border_muted,
        "acme-shop",
    );
    assert_border_fg(&buf, area, blog_top, blog_bot, elev.border_muted, "blog");

    let top_row: String = (area.x..area.x + area.width)
        .map(|x| buf[(x, acme_top)].symbol())
        .collect();
    assert!(top_row.starts_with("│╭ ▸ acme-shop "), "{top_row:?}");
    let acme_start = col_of(&buf, area, acme_top, "acme-shop");
    assert_eq!(buf[(acme_start, acme_top)].fg, palette.text);
    assert!(
        buf[(acme_start, acme_top)]
            .modifier
            .contains(Modifier::BOLD)
    );
    assert_ne!(buf[(acme_start, acme_top)].bg, palette.accent);

    // Unselected blog keeps plain subtext0 title, no chip background, no `▌` anywhere.
    let blog_row: String = (area.x..area.x + area.width)
        .map(|x| buf[(x, blog_top)].symbol())
        .collect();
    let blog_start = col_of(&buf, area, blog_top, "blog");
    assert_eq!(buf[(blog_start, blog_top)].fg, palette.subtext0);
    assert_ne!(buf[(blog_start, blog_top)].bg, palette.accent);
    assert!(!blog_row.contains('▌') && !top_row.contains('▌'));

    // Focusing the lot paints the selected `acme-shop` frame accent + chip and `▌` on selected row.
    panel.focused = true;
    panel.selected = Some(Target::Person("acme-shop".into(), "producer".into()));
    let (focused_buf, hits) = draw_cur(area, &summary, &panel, Some("acme-shop"), &palette);
    let prod_y = hit_y(
        &hits,
        &Target::Person("acme-shop".into(), "producer".into()),
    );
    assert_eq!(focused_buf[(area.x + 2, prod_y)].symbol(), "▌");
    assert_border_fg(
        &focused_buf,
        area,
        acme_top,
        acme_bot,
        palette.accent,
        "focused acme",
    );
    let ftop: String = (area.x..area.x + area.width)
        .map(|x| focused_buf[(x, acme_top)].symbol())
        .collect();
    assert!(ftop.starts_with("│╭ ▸ acme-shop "), "{ftop:?}");
    let chip_fg = glyphs::contrast_fg(&palette);
    for x in 2..=14 {
        let cell = &focused_buf[(x, acme_top)];
        assert_eq!(cell.bg, palette.accent, "chip bg x={x}: {ftop:?}");
        assert_eq!(cell.fg, chip_fg, "chip fg x={x}: {ftop:?}");
        assert!(cell.modifier.contains(Modifier::BOLD), "chip bold x={x}");
    }
    let dot_x = col_of(&focused_buf, area, acme_top, "●");
    assert_eq!(focused_buf[(dot_x, acme_top)].fg, palette.green);
    assert_ne!(focused_buf[(dot_x, acme_top)].bg, palette.accent);
}

#[test]
fn moving_cursor_moves_accent_frame_and_title_chip_and_all_themes_contrast() {
    let summary = two_studio_summary();
    let area = Rect::new(0, 0, 37, 30);

    for theme in ["catppuccin", "catppuccin-latte", "backlot", "terminal"] {
        let palette = Palette::from_name(theme).unwrap();
        let elev = palette.elevation();
        let panel = Panel {
            mode: PanelMode::Expanded,
            focused: true,
            selected: Some(Target::Studio("blog".into())),
            ..Panel::new()
        };
        let (buf, _) = draw_cur(area, &summary, &panel, Some("acme-shop"), &palette);
        let (acme_top, acme_bot) = block_bounds(&buf, area, "acme-shop");
        let (blog_top, blog_bot) = block_bounds(&buf, area, "blog");

        // `acme-shop` still holds the focused terminal `▸` (`palette.text`), but its border is `border_muted`.
        assert_border_fg(&buf, area, acme_top, acme_bot, elev.border_muted, theme);
        let acme_x = (area.x..area.x + area.width)
            .find(|&x| buf[(x, acme_top)].symbol() == "a")
            .unwrap();
        assert_eq!(buf[(acme_x, acme_top)].fg, palette.text);
        assert_ne!(buf[(acme_x, acme_top)].bg, palette.accent);

        // `blog` is selected: border is `accent`, chip ` blog ` has `bg == accent` and `contrast_fg`, bold.
        assert_border_fg(&buf, area, blog_top, blog_bot, palette.accent, theme);
        let expected_fg = glyphs::contrast_fg(&palette);
        for x in 2..=7 {
            let cell = &buf[(x, blog_top)];
            assert_eq!(cell.bg, palette.accent, "{theme} blog chip bg x={x}");
            assert_eq!(cell.fg, expected_fg, "{theme} blog chip fg x={x}");
            assert!(cell.modifier.contains(Modifier::BOLD), "{theme} bold x={x}");
        }
    }

    // No cursor and no current studio while focused -> first studio (`acme-shop`) is accented.
    let palette = Palette::from_name("catppuccin").unwrap();
    let elev = palette.elevation();
    let panel = Panel {
        mode: PanelMode::Expanded,
        focused: true,
        ..Panel::new()
    };
    let (buf, _) = draw_cur(area, &summary, &panel, None, &palette);
    let (acme_top, acme_bot) = block_bounds(&buf, area, "acme-shop");
    let (blog_top, blog_bot) = block_bounds(&buf, area, "blog");
    assert_border_fg(
        &buf,
        area,
        acme_top,
        acme_bot,
        palette.accent,
        "fallback first",
    );
    assert_border_fg(
        &buf,
        area,
        blog_top,
        blog_bot,
        elev.border_muted,
        "fallback second",
    );
}

#[test]
fn strip_and_stacked_selected_studio_uses_accent_when_focused_and_text_when_unfocused() {
    let summary = two_studio_summary();
    let palette = Palette::from_name("catppuccin").unwrap();

    // Collapsed strip, unfocused vs focused, cursor in `blog`.
    for (focused, want_fg) in [(false, palette.text), (true, palette.accent)] {
        let strip_panel = Panel {
            mode: PanelMode::Collapsed,
            focused,
            selected: Some(Target::Strip("blog".into())),
            ..Panel::new()
        };
        let (strip_buf, strip_hits) = draw_cur(
            Rect::new(0, 0, 17, 20),
            &summary,
            &strip_panel,
            Some("acme-shop"),
            &palette,
        );
        let acme_y = hit_y(&strip_hits, &Target::Strip("acme-shop".into()));
        let blog_y = hit_y(&strip_hits, &Target::Strip("blog".into()));
        assert_eq!(strip_buf[(3, acme_y)].fg, palette.text);
        assert!(!strip_buf[(3, acme_y)].modifier.contains(Modifier::BOLD));
        assert_eq!(strip_buf[(3, blog_y)].fg, want_fg);
        assert!(strip_buf[(3, blog_y)].modifier.contains(Modifier::BOLD));
    }

    // Stacked rows, unfocused vs focused: selected `acme-shop` uses `text` BOLD when unfocused,
    // `accent` BOLD when focused.
    for (focused, want_fg) in [(false, palette.text), (true, palette.accent)] {
        let mut stacked_panel = Panel {
            mode: PanelMode::Expanded,
            focused,
            selected: Some(Target::Block("acme-shop".into())),
            ..Panel::new()
        };
        stacked_panel.collapsed_blocks.insert("acme-shop".into());
        stacked_panel.collapsed_blocks.insert("blog".into());
        let (stack_buf, stack_hits) = draw_cur(
            Rect::new(0, 0, 37, 20),
            &summary,
            &stacked_panel,
            None,
            &palette,
        );
        let acme_sy = hit_y(&stack_hits, &Target::Block("acme-shop".into()));
        let blog_sy = hit_y(&stack_hits, &Target::Block("blog".into()));
        assert_eq!(stack_buf[(3, acme_sy)].fg, want_fg);
        assert!(stack_buf[(3, acme_sy)].modifier.contains(Modifier::BOLD));
        assert_eq!(stack_buf[(3, blog_sy)].fg, palette.subtext0);
        assert!(!stack_buf[(3, blog_sy)].modifier.contains(Modifier::BOLD));
    }
}

#[test]
fn narrow_dock_fits_title_chip_padding_and_truncates_label_before_dropping_tail() {
    let mut summary = two_studio_summary();
    summary.studios[1].id = "very-long-second-studio-title-01".into();
    let palette = Palette::from_name("catppuccin").unwrap();
    let panel = Panel {
        mode: PanelMode::Expanded,
        focused: true,
        selected: Some(Target::Studio("very-long-second-studio-title-01".into())),
        ..Panel::new()
    };

    let studio_target = Target::Studio("very-long-second-studio-title-01".into());
    for width in [37u16, 29u16] {
        let area = Rect::new(0, 0, width, 30);
        let (buf, hits) = draw_cur(area, &summary, &panel, None, &palette);
        let top_y = hit_y(&hits, &studio_target);
        let row: String = (area.x..area.x + area.width)
            .map(|x| buf[(x, top_y)].symbol())
            .collect();
        // Tail (`!1`) and `╮` keep their place, label truncates with `…`, chip has 1-cell padding each side.
        assert!(row.contains("…"), "w{width} label truncated: {row:?}");
        assert!(row.contains("!1╮"), "w{width} tail preserved: {row:?}");
        let chip_cells: Vec<u16> = (0..area.width)
            .filter(|&x| buf[(x, top_y)].bg == palette.accent)
            .collect();
        assert!(!chip_cells.is_empty(), "w{width} chip present: {row:?}");
        assert_eq!(buf[(chip_cells[0], top_y)].symbol(), " ");
        assert_eq!(buf[(*chip_cells.last().unwrap(), top_y)].symbol(), " ");
        // Gap before `!` / `●` is un-accented.
        let bang_x = col_of(&buf, area, top_y, "!");
        assert_eq!(buf[(bang_x, top_y)].fg, palette.peach);
        assert_ne!(buf[(bang_x - 1, top_y)].bg, palette.accent);
    }

    // With waiting == 0 the full progress tail (`● r1·1/2`) stays while a long label truncates.
    summary.studios[1].waiting = 0;
    let area = Rect::new(0, 0, 37, 30);
    let (buf, hits) = draw_cur(area, &summary, &panel, None, &palette);
    let top_y = hit_y(&hits, &studio_target);
    let row: String = (area.x..area.x + area.width)
        .map(|x| buf[(x, top_y)].symbol())
        .collect();
    assert!(row.contains("…"), "progress tail label truncated: {row:?}");
    assert!(
        row.contains("● r1·1/2╮"),
        "progress tail preserved: {row:?}"
    );
}

#[test]
fn screenshot_36x30_renders_selected_chip_and_accent_border_when_focused() {
    let summary = two_studio_summary();
    let palette = Palette::from_name("catppuccin").unwrap();
    let elev = palette.elevation();
    let panel = Panel {
        mode: PanelMode::Expanded,
        focused: true,
        ..Panel::new()
    };
    let area = Rect::new(0, 0, 37, 30);
    let (buf, _) = draw_cur(area, &summary, &panel, Some("acme-shop"), &palette);
    let (acme_top, acme_bot) = block_bounds(&buf, area, "acme-shop");
    let (blog_top, blog_bot) = block_bounds(&buf, area, "blog");
    assert_border_fg(&buf, area, acme_top, acme_bot, palette.accent, "shot acme");
    assert_border_fg(
        &buf,
        area,
        blog_top,
        blog_bot,
        elev.border_muted,
        "shot blog",
    );

    if let Some(out_path) = std::env::var_os("LOT_ACCENT_SHOT_OUT") {
        let lines: Vec<String> = (area.y..area.y + area.height)
            .map(|y| {
                (area.x..area.x + area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect();
        let chip_cols: Vec<u16> = (0..area.width)
            .filter(|&x| buf[(x, acme_top)].bg == palette.accent)
            .collect();
        let summary_note = format!(
            "\n---\naccent border rows: y={acme_top}..={acme_bot} (`╭─╮│╰╯` fg=accent)\n\
             surface1 border rows: y={blog_top}..={blog_bot} (`╭─╮│╰╯` fg=surface1)\n\
             title chip cells: y={acme_top}, x={}..={} (`bg=accent`, `fg=panel_bg`, BOLD)\n",
            chip_cols.first().copied().unwrap_or(0),
            chip_cols.last().copied().unwrap_or(0),
        );
        if let Some(parent) = std::path::Path::new(&out_path).parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&out_path, format!("{}{}", lines.join("\n"), summary_note)).unwrap();
    }
}
