//! The layer's renderer at 200/120/80/40 columns: the hidden state
//! draws nothing, the strip caps at a third of the width, the panel
//! drops at most half the height floating and fills the frame on a
//! narrow screen. The draw tests run in the `backlot` theme and in
//! one reference theme — every colour is the palette's.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::*;
use crate::chrome::palette::Palette;
use crate::hollywood::{CardView, Hollywood, LayerMode, Target, render};

fn palettes() -> [Palette; 2] {
    [
        Palette::from_name("backlot").unwrap(),
        Palette::from_name("catppuccin").unwrap(),
    ]
}

fn layer(mode: LayerMode) -> Hollywood {
    Hollywood {
        mode,
        ..Hollywood::new()
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

fn draw(
    area: Rect,
    summary: Option<&Summary>,
    stage_running: bool,
    layer: &Hollywood,
    mobile: bool,
    palette: &Palette,
) -> (Buffer, crate::hollywood::HollywoodRender) {
    let mut buf = Buffer::empty(area);
    let hits = render(
        &mut buf,
        area,
        summary,
        stage_running,
        layer,
        mobile,
        palette,
    );
    (buf, hits)
}

// ---------- hidden ----------

#[test]
fn hidden_draws_nothing_and_maps_no_hits() {
    let summary = summary();
    let area = Rect::new(0, 0, 120, 24);
    let (buf, hits) = draw(
        area,
        Some(&summary),
        true,
        &layer(LayerMode::Hidden),
        false,
        &palettes()[0],
    );
    for cell in buf.content() {
        assert_eq!(cell.symbol(), " ");
    }
    assert!(!hits.contains((0, 0)));
    assert!(!hits.contains((119, 0)));
}

// ---------- collapsed ----------

#[test]
fn the_strip_sits_at_the_top_right_and_caps_at_a_third() {
    let summary = summary();
    for palette in &palettes() {
        for cols in [200u16, 120, 80, 40] {
            let area = Rect::new(0, 0, cols, 24);
            let (buf, hits) = draw(
                area,
                Some(&summary),
                true,
                &layer(LayerMode::Collapsed),
                cols <= 64,
                palette,
            );
            let cap = cols / 3;
            assert_eq!(hits.area.height, 1, "w{cols}");
            assert_eq!(hits.area.y, 0, "w{cols}");
            assert!(
                hits.area.width <= cap.max(1),
                "w{cols}: {}",
                hits.area.width
            );
            assert_eq!(hits.area.x + hits.area.width, cols, "w{cols}: right edge");
            let row = lines(&buf, area)[0].clone();
            assert!(row.contains("✦ hollywood"), "w{cols}: {row}");
            // The strip's own cells carry the panel background.
            for x in hits.area.x..hits.area.x + hits.area.width {
                assert_eq!(buf[(x, 0)].bg, palette.panel_bg, "w{cols} x{x}");
            }
            // A click on it expands — the whole strip is the target.
            assert!(hits.contains((cols - 1, 0)), "w{cols}");
        }
    }
}

#[test]
fn the_strip_truncates_a_long_headline_to_the_cap() {
    let mut value = summary_json();
    value["hollywood"]["headline"] = serde_json::json!("x".repeat(300));
    let summary: Summary = serde_json::from_value(value).unwrap();
    let area = Rect::new(0, 0, 120, 24);
    let (buf, hits) = draw(
        area,
        Some(&summary),
        true,
        &layer(LayerMode::Collapsed),
        false,
        &palettes()[0],
    );
    assert!(hits.area.width <= 40, "width {}", hits.area.width);
    let row = lines(&buf, area)[0].clone();
    assert!(row.contains("hollywood"), "{row}");
    assert!(!row.contains("xxxxx"), "{row}: the headline was cut");
}

// ---------- expanded ----------

#[test]
fn the_panel_drops_at_most_half_the_height_with_all_its_rows() {
    let summary = summary();
    for palette in &palettes() {
        for cols in [200u16, 120] {
            let area = Rect::new(0, 0, cols, 24);
            let (buf, hits) = draw(
                area,
                Some(&summary),
                true,
                &layer(LayerMode::Expanded),
                false,
                palette,
            );
            assert!(hits.area.height <= 12, "w{cols}: {}", hits.area.height);
            assert_eq!(hits.area.x, 0);
            assert_eq!(hits.area.width, cols);
            let text = lines(&buf, area).join("\n");
            assert!(text.contains("✦ hollywood live"), "w{cols}: {text}");
            assert!(text.contains("fake/model-h/high"), "w{cols}: {text}");
            assert!(text.contains("acme-shop"), "w{cols}");
            assert!(text.contains("beta-tools"), "w{cols}");
            assert!(text.contains("!2"), "w{cols}: the waiting count");
            assert!(text.contains("acme-shop needs a look"), "w{cols}");
            assert!(text.contains("pause t1"), "w{cols}");
            assert!(text.contains("apply"), "w{cols}: card buttons");
            assert!(text.contains("cancel"), "w{cols}");
            assert!(text.contains("show"), "w{cols}");
            assert!(text.contains("›"), "w{cols}: the input line");
            // Hit rows exist for each body row and the buttons.
            for target in [
                Target::Title,
                Target::Studio("acme-shop".into()),
                Target::Studio("beta-tools".into()),
                Target::Card("c3".into()),
                Target::Card("c4".into()),
                Target::Input,
            ] {
                assert!(
                    hits.rows.iter().any(|(_, t)| *t == target),
                    "w{cols}: hit {target:?}"
                );
            }
            assert!(hits.buttons.len() >= 6, "w{cols}: two cards × 3");
            assert!(hits.cursor.is_none(), "not editing");
            // The bottom edge closes the float.
            let edge = hits.area.y + hits.area.height - 1;
            assert_eq!(buf[(0, edge)].symbol(), "─", "w{cols}");
        }
    }
}

#[test]
fn the_panel_is_full_screen_at_narrow_widths() {
    let summary = summary();
    for cols in [80u16, 40] {
        let area = Rect::new(0, 0, cols, 24);
        let (buf, hits) = draw(
            area,
            Some(&summary),
            true,
            &layer(LayerMode::Expanded),
            true,
            &palettes()[0],
        );
        assert_eq!(hits.area, area, "w{cols}: the whole frame");
        let text = lines(&buf, area).join("\n");
        assert!(text.contains("hollywood"), "w{cols}: {text}");
        // No floating edge row on the full-screen form: the last row
        // is the input line's own.
        let last = &lines(&buf, area)[area.height as usize - 1];
        assert!(!last.chars().all(|c| c == '─'), "w{cols}: {last}");
    }
}

#[test]
fn the_panel_truncates_and_scrolls_its_body() {
    let summary = summary();
    // The float caps at half the frame: 12 rows → a 6-row panel whose
    // 3-row body cannot show the 5 body rows → scroll.
    let area = Rect::new(0, 0, 60, 12);
    let (_, hits) = draw(
        area,
        Some(&summary),
        true,
        &layer(LayerMode::Expanded),
        false,
        &palettes()[0],
    );
    assert_eq!(hits.area.height, 6);
    assert!(hits.viewport_rows > 0);
    assert!(
        hits.metrics.map(|m| m.max_offset_from_bottom).unwrap_or(0) > 0,
        "short body scrolls"
    );
    assert!(!hits.scrollbar.is_empty(), "the scrollbar track is mapped");
}

#[test]
fn a_dead_stage_says_so_and_a_down_layer_reads_down() {
    let summary = summary();
    let area = Rect::new(0, 0, 80, 24);
    let (buf, _) = draw(
        area,
        Some(&summary),
        false,
        &layer(LayerMode::Expanded),
        false,
        &palettes()[0],
    );
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("stage not running"), "{text}");
    // No hollywood object → the title reads `down`.
    let mut value = summary_json();
    value.as_object_mut().unwrap().remove("hollywood");
    let summary: Summary = serde_json::from_value(value).unwrap();
    let (buf, _) = draw(
        area,
        Some(&summary),
        true,
        &layer(LayerMode::Expanded),
        false,
        &palettes()[0],
    );
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("hollywood down"), "{text}");
    assert!(text.contains("acme-shop"), "{text}: studios stay");
}

#[test]
fn a_closed_or_lost_role_is_named_in_title_and_strip() {
    for state in ["closed", "lost"] {
        let mut value = summary_json();
        value["hollywood"]["state"] = serde_json::json!(state);
        let summary: Summary = serde_json::from_value(value).unwrap();
        let area = Rect::new(0, 0, 80, 24);
        let (buf, _) = draw(
            area,
            Some(&summary),
            true,
            &layer(LayerMode::Expanded),
            false,
            &palettes()[0],
        );
        assert!(
            lines(&buf, area)
                .join("\n")
                .contains(&format!("hollywood {state}")),
            "{state}"
        );
    }
}

#[test]
fn the_input_line_edits_and_hint_shows() {
    let summary = summary();
    let area = Rect::new(0, 0, 80, 24);
    // The dim hint while empty and not editing.
    let (buf, hits) = draw(
        area,
        Some(&summary),
        true,
        &layer(LayerMode::Expanded),
        false,
        &palettes()[0],
    );
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("i — tell hollywood"), "{text}");
    assert!(hits.cursor.is_none());
    // Editing maps the cursor.
    let mut editing = layer(LayerMode::Expanded);
    editing.editing = true;
    let (_, hits) = draw(area, Some(&summary), true, &editing, false, &palettes()[0]);
    assert!(hits.cursor.is_some(), "editing hands the cursor over");
}

#[test]
fn a_render_over_drawn_cells_leaves_no_symbol_behind() {
    let summary = summary();
    let area = Rect::new(0, 0, 120, 24);
    for mode in [LayerMode::Collapsed, LayerMode::Expanded] {
        let mut buf = Buffer::empty(area);
        for y in area.y..area.y + area.height {
            buf.set_string(
                area.x,
                y,
                "█".repeat(usize::from(area.width)),
                ratatui::style::Style::default(),
            );
        }
        let hits = render(
            &mut buf,
            area,
            Some(&summary),
            true,
            &layer(mode),
            false,
            &palettes()[0],
        );
        // Under the layer's own rect nothing bleeds through.
        for y in hits.area.y..hits.area.y + hits.area.height {
            for x in hits.area.x..hits.area.x + hits.area.width {
                assert_ne!(buf[(x, y)].symbol(), "█", "leak at ({x},{y})");
            }
        }
    }
}

#[test]
fn the_card_view_draws_over_the_layer() {
    let summary = summary();
    let mut open = layer(LayerMode::Expanded);
    open.view = Some(CardView {
        title: "c4".into(),
        lines: (0..30).map(|i| format!("draft line {i}")).collect(),
        scroll: 0,
    });
    let area = Rect::new(0, 0, 80, 24);
    let (buf, hits) = draw(area, Some(&summary), true, &open, false, &palettes()[0]);
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("draft line 0"), "{text}");
    assert!(text.contains("╭"), "{text}");
    assert!(text.contains("c4"), "{text}");
    let view = hits.view.expect("the view's hit map");
    assert!(view.metrics.unwrap().max_offset_from_bottom > 0);
}
