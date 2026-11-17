//! The bar's renderer (U7): the collapsed strip and the film-strip
//! band at 160/100/60 columns — the sprocket rows, the title row, one
//! frame per studio, the new-studio frame, the height ladder and the
//! no-agent form. Every colour is a palette entry, checked here.
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

use super::*;
use crate::chrome::palette::Palette;
use crate::hollywood::{CardView, Hollywood, LayerMode, LayerOpts, Target, render};

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

/// The layer with `selected` picked, the way a key press leaves it.
fn selecting(mode: LayerMode, target: Target) -> Hollywood {
    Hollywood {
        mode,
        selected: Some(target),
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

fn row_of(buf: &Buffer, area: Rect, y: u16) -> String {
    (area.x..area.x + area.width)
        .map(|x| buf[(x, y)].symbol())
        .collect::<String>()
        .trim_end()
        .to_string()
}

fn draw_with(
    area: Rect,
    summary: Option<&Summary>,
    stage_running: bool,
    layer: &Hollywood,
    opts: LayerOpts,
    palette: &Palette,
) -> (Buffer, crate::hollywood::HollywoodRender) {
    let mut buf = Buffer::empty(area);
    let hits = render(
        &mut buf,
        area,
        &machines(summary),
        None,
        stage_running,
        layer,
        opts,
        palette,
    );
    (buf, hits)
}

fn draw(
    area: Rect,
    summary: Option<&Summary>,
    stage_running: bool,
    layer: &Hollywood,
    mobile: bool,
    palette: &Palette,
) -> (Buffer, crate::hollywood::HollywoodRender) {
    draw_with(
        area,
        summary,
        stage_running,
        layer,
        LayerOpts {
            agent: true,
            mobile,
        },
        palette,
    )
}

fn draw_without_agent(
    area: Rect,
    summary: Option<&Summary>,
    stage_running: bool,
    layer: &Hollywood,
    palette: &Palette,
) -> (Buffer, crate::hollywood::HollywoodRender) {
    draw_with(
        area,
        summary,
        stage_running,
        layer,
        LayerOpts {
            agent: false,
            mobile: false,
        },
        palette,
    )
}

/// The frame hit for `target`, as the mouse would find it.
fn frame_hit(hits: &crate::hollywood::HollywoodRender, target: &Target) -> Option<Rect> {
    hits.rows
        .iter()
        .find(|(_, t)| t == target)
        .map(|(rect, _)| *rect)
}

/// The fg colour of the first cell on row `y` holding `symbol` after
/// column `from`.
fn fg_of(buf: &Buffer, area: Rect, y: u16, from: u16, symbol: &str) -> Option<Color> {
    (from..area.width)
        .find(|x| buf[(*x, y)].symbol() == symbol)
        .map(|x| buf[(x, y)].fg)
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
            assert!(hits.area.width <= cap.max(1), "w{cols}");
            assert_eq!(hits.area.x + hits.area.width, cols, "w{cols}: right edge");
            let row = row_of(&buf, area, 0);
            assert!(row.contains("✦ hollywood"), "w{cols}: {row}");
            if cols >= 80 {
                assert!(row.contains("3 studio"), "w{cols}: {row}");
            }
            // The strip sits on surface_dim, the waiting chip on peach.
            for x in hits.area.x..hits.area.x + hits.area.width {
                let bg = buf[(x, 0)].bg;
                assert!(
                    bg == palette.surface_dim || bg == palette.peach,
                    "w{cols} x{x}: {bg:?}"
                );
            }
            assert_eq!(buf[(hits.area.x, 0)].bg, palette.surface_dim);
            // A click on it expands — the whole strip is the target.
            assert!(hits.contains((cols - 1, 0)), "w{cols}");
        }
    }
}

#[test]
fn the_strip_shows_the_waiting_chip_only_when_something_waits() {
    let palette = palettes()[0].clone();
    let summary = summary();
    let area = Rect::new(0, 0, 120, 24);
    let (buf, hits) = draw(
        area,
        Some(&summary),
        true,
        &layer(LayerMode::Collapsed),
        false,
        &palette,
    );
    let row = row_of(&buf, area, 0);
    assert!(row.contains("!2"), "{row}");
    // The chip's cells are the peach ones on the row.
    let chip: Vec<u16> = (hits.area.x..hits.area.x + hits.area.width)
        .filter(|x| buf[(*x, 0)].bg == palette.peach)
        .collect();
    assert!(!chip.is_empty(), "the peach chip: {row}");
    assert!(
        chip.iter()
            .map(|x| buf[(*x, 0)].symbol())
            .collect::<String>()
            .contains('!'),
        "the chip carries the count: {row}"
    );
    // Nothing waiting: no chip.
    let mut value = summary_json();
    value["studios"][0]["waiting"] = serde_json::json!(0);
    let quiet: Summary = serde_json::from_value(value).unwrap();
    let (buf, _) = draw(
        area,
        Some(&quiet),
        true,
        &layer(LayerMode::Collapsed),
        false,
        &palette,
    );
    let row = row_of(&buf, area, 0);
    assert!(!row.contains('!'), "{row}");
}

// ---------- the band ----------

#[test]
fn the_band_draws_sprockets_title_and_counts() {
    let summary = summary();
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 160, 45);
    let (buf, hits) = draw(
        area,
        Some(&summary),
        true,
        &layer(LayerMode::Expanded),
        false,
        &palette,
    );
    let text = lines(&buf, area).join("\n");
    // The band is surface_dim; the sprocket rows sit on sidebar_bg.
    assert_eq!(buf[(0, 2)].bg, palette.surface_dim);
    // Sprockets: `▄▄` on the first row, `▀▀` on the last, on sidebar_bg.
    let top = row_of(&buf, area, 0);
    assert!(top.contains("▄▄"), "{top}");
    let bottom = row_of(&buf, area, hits.area.height - 1);
    assert!(bottom.contains("▀▀"), "{bottom}");
    assert_eq!(buf[(1, 0)].bg, palette.sidebar_bg);
    assert_eq!(buf[(1, hits.area.height - 1)].bg, palette.sidebar_bg);
    // The title row: the mark, the agent and the right-hand counts.
    let title = row_of(&buf, area, 1);
    assert!(title.contains("✦  H O L L Y W O O D"), "{title}");
    assert!(title.contains("live"), "{title}");
    assert!(title.contains("fake / model-h / high"), "{title}");
    assert!(title.contains("3 studios"), "{title}");
    assert!(title.contains("2 waiting on you"), "{title}");
    // The waiting part is peach.
    let peach: Vec<u16> = (area.x..area.x + area.width)
        .filter(|x| buf[(*x, 1)].fg == palette.peach)
        .collect();
    assert!(!peach.is_empty(), "{title}: the waiting count");
    // The headline, the cards and the hints follow the frames.
    assert!(text.contains("acme-shop needs a look"), "{text}");
    assert!(text.contains("acme-shop · pause t1"), "{text}");
    assert!(text.contains("apply"), "{text}");
    assert!(
        text.contains("← → move   ⏎ open   n new   a apply   c cancel   i tell   esc close"),
        "{text}"
    );
}

#[test]
fn an_error_studio_shows_it_in_red() {
    let summary = summary();
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 160, 45);
    let (buf, hits) = draw(
        area,
        Some(&summary),
        true,
        &layer(LayerMode::Expanded),
        false,
        &palette,
    );
    let frame = frame_hit(&hits, &Target::Studio("gamma-site".into())).expect("gamma-site");
    let what = row_of(&buf, area, frame.y + 3);
    assert!(what.contains("events.jsonl: unreadable"), "{what}");
    assert_eq!(
        fg_of(&buf, area, frame.y + 3, frame.x, "e"),
        Some(palette.red)
    );
}

#[test]
fn frames_that_do_not_fit_get_carets() {
    let summary = summary();
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 100, 30);
    let (buf, hits) = draw(
        area,
        Some(&summary),
        true,
        &layer(LayerMode::Expanded),
        false,
        &palette,
    );
    let top_row = hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Studio(_)))
        .map(|(rect, _)| rect.y)
        .expect("a frame");
    let top = row_of(&buf, area, top_row);
    assert!(!top.contains('‹'), "{top}");
    assert!(top.contains('›'), "{top}: more frames right");
    // Moving the window right shows the left caret.
    let mut scrolled = layer(LayerMode::Expanded);
    scrolled.frame_scroll = 2;
    let (buf, _) = draw(area, Some(&summary), true, &scrolled, false, &palette);
    let top = row_of(&buf, area, top_row);
    assert!(top.contains('‹'), "{top}");
}

#[test]
fn the_height_ladder_never_panics_and_keeps_the_frames() {
    let summary = summary();
    let palette = palettes()[0].clone();
    for width in [160u16, 100, 60, 30] {
        for height in 1..=30u16 {
            let area = Rect::new(0, 0, width, height);
            let (buf, hits) = draw(
                area,
                Some(&summary),
                true,
                &layer(LayerMode::Expanded),
                width <= 64,
                &palette,
            );
            assert!(!hits.area.is_empty(), "w{width} h{height}");
            assert_eq!(lines(&buf, area).len(), usize::from(height));
        }
    }
    // A short band drops the hints and the headline before the frames.
    let area = Rect::new(0, 0, 160, 20);
    let (buf, _) = draw(
        area,
        Some(&summary),
        true,
        &layer(LayerMode::Expanded),
        false,
        &palette,
    );
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("acme-shop"), "{text}");
    assert!(!text.contains("esc close"), "{text}");
    assert!(!text.contains("acme-shop needs a look"), "{text}");
}

#[test]
fn the_mobile_layout_puts_two_frames_per_row() {
    let summary = summary();
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 60, 20);
    let (buf, hits) = draw(
        area,
        Some(&summary),
        true,
        &layer(LayerMode::Expanded),
        true,
        &palette,
    );
    let frames: Vec<Rect> = hits
        .rows
        .iter()
        .filter(|(_, t)| matches!(t, Target::Studio(_)))
        .map(|(rect, _)| *rect)
        .collect();
    assert_eq!(frames.len(), 3, "{frames:?}");
    assert_eq!(frames[0].y, frames[1].y, "two per row");
    assert!(frames[2].y > frames[0].y, "the third wraps");
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("acme-shop"), "{text}");
    // Under 40 columns a frame takes the row alone.
    let area = Rect::new(0, 0, 36, 20);
    let (_, hits) = draw(
        area,
        Some(&summary),
        true,
        &layer(LayerMode::Expanded),
        true,
        &palette,
    );
    let frames: Vec<Rect> = hits
        .rows
        .iter()
        .filter(|(_, t)| matches!(t, Target::Studio(_)))
        .map(|(rect, _)| *rect)
        .collect();
    assert!(frames.len() >= 2, "{frames:?}");
    assert!(frames[0].y < frames[1].y, "one per row");
}

// ---------- the no-agent bar ----------

#[test]
fn without_an_agent_the_band_is_the_setup_line() {
    let summary = summary();
    let area = Rect::new(0, 0, 120, 24);
    let (buf, hits) = draw_without_agent(
        area,
        Some(&summary),
        true,
        &layer(LayerMode::Expanded),
        &palettes()[0],
    );
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("✦  H O L L Y W O O D"), "{text}");
    assert!(text.contains("no agent"), "{text}");
    assert!(text.contains("acme-shop"), "{text}");
    assert!(text.contains("backlot hollywood setup"), "{text}");
    assert!(!text.contains("tell hollywood"), "{text}");
    assert!(!text.contains("acme-shop needs a look"), "{text}");
    assert!(!text.contains("apply"), "{text}");
    assert!(hits.input.is_empty(), "no input line without an agent");
}

#[test]
fn the_strip_without_an_agent_names_the_studios() {
    let summary = summary();
    let area = Rect::new(0, 0, 120, 24);
    let (buf, _) = draw_without_agent(
        area,
        Some(&summary),
        true,
        &layer(LayerMode::Collapsed),
        &palettes()[0],
    );
    let row = row_of(&buf, area, 0);
    assert!(row.contains("✦ hollywood"), "{row}");
    assert!(row.contains("3 studios"), "{row}");
}

#[test]
fn every_studio_is_a_frame_and_the_selected_one_is_accent() {
    let summary = summary();
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 160, 45);
    let selected = Target::Studio("acme-shop".into());
    let (buf, hits) = draw(
        area,
        Some(&summary),
        true,
        &selecting(LayerMode::Expanded, selected.clone()),
        false,
        &palette,
    );
    let text = lines(&buf, area).join("\n");
    for studio in ["acme-shop", "beta-tools", "gamma-site"] {
        assert!(text.contains(studio), "{studio} missing:\n{text}");
    }
    let frame = frame_hit(&hits, &selected).expect("acme-shop's frame");
    assert_eq!(frame.height, 7);
    // The top edge: `╭─ acme-shop ─…╮` in accent, the id bold text.
    let top = row_of(&buf, area, frame.y);
    assert!(top.contains("╭─ acme-shop"), "{top}");
    assert_eq!(buf[(frame.x, frame.y)].fg, palette.accent);
    // The inside is selection_bg; another frame's is panel_bg.
    assert_eq!(buf[(frame.x + 1, frame.y + 1)].bg, palette.selection_bg);
    let other = frame_hit(&hits, &Target::Studio("beta-tools".into())).expect("beta-tools");
    assert_eq!(buf[(other.x + 1, other.y + 1)].bg, palette.panel_bg);
    assert_eq!(buf[(other.x, other.y)].fg, palette.surface1);
}

#[test]
fn the_frame_names_its_state_and_the_role_on_set() {
    let summary = summary();
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 160, 45);
    let (buf, hits) = draw(
        area,
        Some(&summary),
        true,
        &layer(LayerMode::Expanded),
        false,
        &palette,
    );
    let frame = frame_hit(&hits, &Target::Studio("acme-shop".into())).expect("acme-shop");
    let state = row_of(&buf, area, frame.y + 1);
    assert!(state.contains("■ live"), "{state}");
    assert_eq!(buf[(frame.x + 2, frame.y + 1)].fg, palette.green);
    // The waiting chip: ` !2 ` on peach, right-aligned on the row.
    assert!(state.contains("!2"), "{state}");
    let chip: Vec<u16> = (frame.x..frame.x + frame.width)
        .filter(|x| buf[(*x, frame.y + 1)].bg == palette.peach)
        .collect();
    assert_eq!(chip.len(), 4, "the ` !2 ` chip: {chip:?}");
    assert_eq!(
        chip.iter()
            .map(|x| buf[(*x, frame.y + 1)].symbol())
            .collect::<String>(),
        " !2 "
    );
    // The working role is on set, then what it is doing.
    let who = row_of(&buf, area, frame.y + 2);
    assert!(who.contains("producer"), "{who}");
    let what = row_of(&buf, area, frame.y + 3);
    assert!(what.contains("review delivery 1"), "{what}");
    // The cast line: one mark each, coloured by role state.
    let cast = row_of(&buf, area, frame.y + 5);
    assert!(cast.contains("◐ ! ! ○"), "{cast}");
    assert!(cast.contains("4 cast · r2"), "{cast}");
    let marks: Vec<Color> = (frame.x..frame.x + frame.width)
        .filter(|x| matches!(buf[(*x, frame.y + 5)].symbol(), "◐" | "!" | "○"))
        .map(|x| buf[(x, frame.y + 5)].fg)
        .collect();
    assert_eq!(
        marks,
        vec![
            palette.yellow,   // producer: working
            palette.peach,    // alice: attention
            palette.red,      // bob: lost
            palette.overlay1  // carol: idle
        ]
    );
}

// ---------- colours ----------

#[test]
fn every_colour_is_a_palette_entry() {
    let summary = summary();
    let area = Rect::new(0, 0, 160, 45);
    for palette in palettes() {
        let allowed = [
            palette.accent,
            palette.panel_bg,
            palette.sidebar_bg,
            palette.active_row_bg,
            palette.selection_bg,
            palette.surface0,
            palette.surface1,
            palette.surface_dim,
            palette.overlay0,
            palette.overlay1,
            palette.text,
            palette.subtext0,
            palette.mauve,
            palette.green,
            palette.yellow,
            palette.red,
            palette.blue,
            palette.teal,
            palette.peach,
        ];
        for mode in [LayerMode::Collapsed, LayerMode::Expanded] {
            let (buf, hits) = draw(area, Some(&summary), true, &layer(mode), false, &palette);
            for y in hits.area.y..hits.area.y + hits.area.height {
                for x in hits.area.x..hits.area.x + hits.area.width {
                    let cell = &buf[(x, y)];
                    for colour in [cell.fg, cell.bg] {
                        assert!(
                            colour == Color::Reset || allowed.contains(&colour),
                            "{mode:?} at ({x},{y}): {colour:?} is not a palette entry"
                        );
                    }
                }
            }
        }
    }
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
            &machines(Some(&summary)),
            None,
            true,
            &layer(mode),
            LayerOpts {
                agent: true,
                mobile: false,
            },
            &palettes()[0],
        );
        for y in hits.area.y..hits.area.y + hits.area.height {
            for x in hits.area.x..hits.area.x + hits.area.width {
                assert_ne!(buf[(x, y)].symbol(), "█", "leak at ({x},{y})");
            }
        }
    }
}

#[test]
fn the_band_is_opaque() {
    let summary = summary();
    let area = Rect::new(0, 0, 120, 24);
    // The backlot theme's surfaces are opaque; the reference themes
    // leave `sidebar_bg` as the terminal's own background by design.
    let palette = palettes()[0].clone();
    let (buf, hits) = draw(
        area,
        Some(&summary),
        true,
        &layer(LayerMode::Expanded),
        false,
        &palette,
    );
    for y in hits.area.y..hits.area.bottom() {
        for x in hits.area.x..hits.area.right() {
            assert_ne!(
                buf[(x, y)].bg,
                Color::Reset,
                "cell ({x},{y}) shows through; row {:?}",
                row_of(&buf, area, y)
            );
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

// ---------- the new-studio frame ----------

#[test]
fn the_new_studio_frame_is_always_last_and_dashed() {
    let summary = summary();
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 160, 45);
    let (buf, hits) = draw(
        area,
        Some(&summary),
        true,
        &layer(LayerMode::Expanded),
        false,
        &palette,
    );
    let new = frame_hit(&hits, &Target::NewStudio).expect("the new-studio frame");
    let studios: Vec<Rect> = hits
        .rows
        .iter()
        .filter(|(_, t)| matches!(t, Target::Studio(_)))
        .map(|(rect, _)| *rect)
        .collect();
    assert_eq!(studios.len(), 3);
    assert!(
        studios.iter().all(|frame| frame.x < new.x),
        "the entry is the last frame"
    );
    let top = row_of(&buf, area, new.y);
    assert!(top.contains('┄'), "a dashed edge: {top}");
    assert!(top.contains('╭') && top.contains('╮'), "{top}");
    assert_eq!(buf[(new.x, new.y)].fg, palette.overlay0);
    assert!(row_of(&buf, area, new.y + 2).contains("+  new studio"));
    assert!(row_of(&buf, area, new.y + 3).contains("repository + cast"));
    let chip = row_of(&buf, area, new.y + 5);
    assert!(chip.contains(" n "), "{chip}");
    // The key chip: ` n ` in accent on surface0.
    let cells: Vec<u16> = (new.x..new.x + new.width)
        .filter(|x| buf[(*x, new.y + 5)].bg == palette.surface0)
        .collect();
    assert_eq!(cells.len(), 3, "{chip}");
    assert_eq!(
        cells
            .iter()
            .map(|x| buf[(*x, new.y + 5)].symbol())
            .collect::<String>(),
        " n "
    );
}

// ---------- studios on several machines (U6a) ----------

#[test]
fn a_remote_studios_frame_titles_its_machine() {
    // Both machines serve `acme-shop`: the frame titles read
    // `acme-shop · Local` and `acme-shop · host-b`, local first.
    let summary = summary();
    let mut list = machines(Some(&summary));
    list.push(remote(&summary));
    let area = Rect::new(0, 0, 160, 45);
    let mut buf = Buffer::empty(area);
    let hits = render(
        &mut buf,
        area,
        &list,
        Some(crate::project::LOCAL_LABEL),
        true,
        &layer(LayerMode::Expanded),
        LayerOpts {
            agent: true,
            mobile: false,
        },
        &palettes()[0],
    );
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("acme-shop · Local"), "{text}");
    assert!(text.contains("acme-shop · host-b"), "{text}");
    // The remote studio is its own frame — a hit keyed by machine.
    let key = crate::studio::StudioKey {
        machine: Some("host-b".into()),
        id: "acme-shop".into(),
    };
    assert!(frame_hit(&hits, &Target::Studio(key)).is_some(), "{text}");
}

#[test]
fn the_narrow_frame_cuts_the_machine_before_the_id() {
    // Two-per-row frames at 56 columns are ~25 cells: a label too
    // long for them falls back to the bare id — the machine name is
    // what the cut takes.
    let summary = summary();
    let mut list = machines(Some(&summary));
    let mut theirs = summary.clone();
    theirs.studios[0].id = "acme-shop-remote-ops".into();
    list.push(remote(&theirs));
    let area = Rect::new(0, 0, 56, 45);
    let mut buf = Buffer::empty(area);
    render(
        &mut buf,
        area,
        &list,
        Some(crate::project::LOCAL_LABEL),
        true,
        &layer(LayerMode::Expanded),
        LayerOpts {
            agent: true,
            mobile: true,
        },
        &palettes()[0],
    );
    let text = lines(&buf, area).join("\n");
    // The short label still fits; the long one's id survives the cut
    // (truncated itself only past the machine's share of the cells).
    assert!(text.contains("beta-tools · host-b"), "{text}");
    assert!(text.contains("acme-shop-remote"), "{text}");
    assert!(!text.contains("acme-shop-remote-ops · host-b"), "{text}");
}
