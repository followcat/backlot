//! Draw tests for the opening page: the full film-strip title card
//! cell for cell, one colour per part, the fallback ladder, the
//! action gates and the one-time animation's first and last frames.

use super::*;

/// A plain-text dump of the buffer's cells, row per line.
fn dump(buffer: &Buffer, area: Rect) -> String {
    let mut out = String::new();
    for y in area.y..area.bottom() {
        let mut line = String::new();
        for x in area.x..area.right() {
            line.push_str(buffer[(x, y)].symbol());
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn palette() -> Palette {
    Palette::backlot()
}

const THREE: OpeningOpts = OpeningOpts {
    hollywood: false,
    stage_running: false,
};

const FOUR: OpeningOpts = OpeningOpts {
    hollywood: true,
    stage_running: false,
};

fn draw(width: u16, height: u16, opts: OpeningOpts, frame: Option<u8>) -> String {
    draw_with(palette(), width, height, opts, frame)
}

fn draw_with(
    palette: Palette,
    width: u16,
    height: u16,
    opts: OpeningOpts,
    frame: Option<u8>,
) -> String {
    let area = Rect::new(0, 0, width, height);
    let mut buffer = Buffer::empty(area);
    render_opening(&mut buffer, area, &palette, opts, frame);
    dump(&buffer, area)
}

fn footer() -> String {
    format!(" v{} · {}", env!("CARGO_PKG_VERSION"), PLAIN_HINT)
}

/// The target page, 49 columns wide with stepped Deco corners and intertitle rule.
fn target() -> [String; 16] {
    [
        "  ▄▄ ▄▄ ▄▄ ▄▄ ▄▄ ▄▄ ▄▄ ▄▄ ▄▄ ▄▄ ▄▄ ▄▄ ▄▄ ▄▄ ▄▄".into(),
        "╔══╤═════════════════════════════════════════╤══╗".into(),
        "╟──┘                                         └──╢".into(),
        "║          █▀▄ ▄▀▄ ▄▀▀ █▄▀ █   ▄▀▄ ▀█▀          ║".into(),
        "║          █▀▄ █▀█ █   █▀▄ █   █ █  █           ║".into(),
        "║          ▀▀  ▀ ▀  ▀▀ ▀ ▀ ▀▀▀  ▀   ▀           ║".into(),
        "║                                               ║".into(),
        "║        a film studio for coding agents        ║".into(),
        "║   ──────────────═══╡ ‹✦› ╞═══──────────────   ║".into(),
        "║       n   New studio                          ║".into(),
        "║       s   Start the stage                     ║".into(),
        "║       q   detach                              ║".into(),
        "╟──┐                                         ┌──╢".into(),
        "╚══╧═════════════════════════════════════════╧══╝".into(),
        "  ▀▀ ▀▀ ▀▀ ▀▀ ▀▀ ▀▀ ▀▀ ▀▀ ▀▀ ▀▀ ▀▀ ▀▀ ▀▀ ▀▀ ▀▀".into(),
        footer(),
    ]
}

/// The block's top-left cell at 49×16 inside `area`.
fn origin(width: u16, height: u16) -> (u16, u16) {
    ((width - 49) / 2, (height - 16) / 2)
}

/// The card's top and bottom rows, found by its frame.
fn card_rows(text: &str) -> (usize, usize) {
    let top = text
        .lines()
        .position(|l| l.contains('╔'))
        .expect("a card top");
    let bottom = text
        .lines()
        .position(|l| l.contains('╚'))
        .expect("a card bottom");
    (top, bottom)
}

#[test]
fn the_full_card_matches_the_target_cell_for_cell() {
    let area = Rect::new(0, 0, 120, 40);
    let mut buffer = Buffer::empty(area);
    render_opening(&mut buffer, area, &palette(), THREE, None);
    let (x, y) = origin(120, 40);
    for (row, want) in target().iter().enumerate() {
        let got: String = (x..x + 49)
            .map(|x| buffer[(x, y + row as u16)].symbol())
            .collect();
        assert_eq!(
            got,
            format!("{want:<49}"),
            "row {row} at {x},{y}:\n{}",
            dump(&buffer, area)
        );
    }
}

#[test]
fn one_cell_per_part_has_its_palette_colour() {
    let p = palette();
    let elev = p.elevation();
    let area = Rect::new(0, 0, 120, 40);
    let mut buffer = Buffer::empty(area);
    render_opening(&mut buffer, area, &p, THREE, None);
    let (x, y) = origin(120, 40);
    let at = |dx: u16, dy: u16| &buffer[(x + dx, y + dy)];

    // Sprockets: surface1 on the panel background.
    let sprocket = at(2, 0);
    assert_eq!(sprocket.symbol(), "▄");
    assert_eq!(sprocket.fg, p.surface1);
    assert_eq!(sprocket.bg, p.panel_bg);
    // The double frame: accent, on the lit screen inside.
    let frame = at(0, 1);
    assert_eq!(frame.symbol(), "╔");
    assert_eq!(frame.fg, p.accent);
    // Stepped inner corner: gilt on surface_dim.
    let corner = at(3, 2);
    assert_eq!(corner.symbol(), "┘");
    assert_eq!(corner.fg, elev.gilt);
    assert_eq!(corner.bg, p.surface_dim);
    // The block title: accent, bold.
    let title = at(11, 3);
    assert_eq!(title.symbol(), "█");
    assert_eq!(title.fg, p.accent);
    assert!(title.modifier.contains(Modifier::BOLD));
    assert_eq!(title.bg, p.surface_dim);
    // The tagline: subtext0.
    let tagline = at(9, 7);
    assert_eq!(tagline.symbol(), "a");
    assert_eq!(tagline.fg, p.subtext0);
    // The rule: surface1 on the hairline, gilt on clasp, accent on star.
    let rule = at(4, 8);
    assert_eq!(rule.symbol(), "─");
    assert_eq!(rule.fg, p.surface1);
    assert_eq!(at(18, 8).symbol(), "═");
    assert_eq!(at(18, 8).fg, elev.gilt);
    assert_eq!(at(24, 8).symbol(), "✦");
    assert_eq!(at(24, 8).fg, p.accent);
    // Keys: accent, bold; labels: text.
    let key = at(8, 9);
    assert_eq!(key.symbol(), "n");
    assert_eq!(key.fg, p.accent);
    assert!(key.modifier.contains(Modifier::BOLD));
    let label = at(12, 9);
    assert_eq!(label.symbol(), "N");
    assert_eq!(label.fg, p.text);
    // The footer: overlay0, outside the card.
    let foot = at(1, 15);
    assert_eq!(foot.symbol(), "v");
    assert_eq!(foot.fg, p.overlay0);
    assert_eq!(foot.bg, p.panel_bg);
}

#[test]
fn the_footer_carries_the_sidebar_version() {
    // The sidebar's badge is `v<crate version>`; the footer is the
    // same string, so a release bump cannot split them.
    let out = draw(120, 40, THREE, None);
    assert!(out.contains(&footer()), "{out}");
}

#[test]
fn a_narrow_card_uses_the_plain_title() {
    // 46 columns: short of the 49-col art title, so the text title and
    // the sprockets (46 columns) still draw.
    let out = draw(46, 20, THREE, None);
    assert!(out.contains("B A C K L O T"), "{out}");
    assert!(out.contains('╔'), "{out}");
    assert!(out.contains('▄'), "{out}");
    assert!(out.contains("‹✦›"), "{out}");
    assert!(!out.contains("█▀▄"), "{out}");
}

#[test]
fn forty_four_by_14_drops_the_sprockets() {
    let out = draw(44, 14, THREE, None);
    assert!(out.contains("B A C K L O T"), "{out}");
    assert!(out.contains('╔'), "{out}");
    assert!(!out.contains('▄'), "{out}");
    assert!(out.contains("New studio"), "{out}");
}

#[test]
fn opening_card_ladder_80x24_49x16_46x14_42x12_36x10() {
    let full_80 = draw(80, 24, THREE, None);
    assert!(full_80.contains("╔══╤"), "{full_80}");
    assert!(full_80.contains("╟──┘"), "{full_80}");
    assert!(full_80.contains("‹✦›"), "{full_80}");
    assert!(full_80.contains("█▀▄"), "{full_80}");
    assert!(full_80.contains("▄ ▄"), "{full_80}");

    let exact_49 = draw(49, 16, THREE, None);
    assert!(exact_49.contains("╔══╤"), "{exact_49}");
    assert!(exact_49.contains("█▀▄"), "{exact_49}");
    assert!(exact_49.contains("▄ ▄"), "{exact_49}");

    let mid_46 = draw(46, 14, THREE, None);
    assert!(mid_46.contains("╔══╤"), "{mid_46}");
    assert!(mid_46.contains("B A C K L O T"), "{mid_46}");
    assert!(mid_46.contains("‹✦›"), "{mid_46}");
    assert!(mid_46.contains("▄ ▄"), "46x14 retains sprockets:\n{mid_46}");
    assert!(!mid_46.contains("█▀▄"), "{mid_46}");

    let min_42 = draw(42, 12, THREE, None);
    assert!(min_42.contains("╔══╤"), "{min_42}");
    assert!(min_42.contains("B A C K L O T"), "{min_42}");
    assert!(!min_42.contains("▄ ▄"), "{min_42}");

    let plain_36 = draw(36, 10, THREE, None);
    assert!(
        !plain_36.contains('╔'),
        "plain fallback below 42:\n{plain_36}"
    );
    assert!(plain_36.contains(TAGLINE), "{plain_36}");
    assert!(plain_36.contains("New studio"), "{plain_36}");
}

#[test]
fn forty_by_twelve_drops_the_frame_and_stays_readable() {
    let out = draw(40, 12, THREE, None);
    assert!(!out.contains('╔'), "{out}");
    assert!(out.contains(TAGLINE), "{out}");
    assert!(out.contains("New studio"), "{out}");
    assert!(out.contains("Start the stage"), "{out}");
    assert!(out.contains("backlot --plain"), "{out}");
}

#[test]
fn a_one_by_one_area_never_panics() {
    let area = Rect::new(0, 0, 1, 1);
    let mut buffer = Buffer::empty(area);
    render_opening(&mut buffer, area, &palette(), THREE, Some(0));
}

#[test]
fn empty_area_is_a_noop() {
    let area = Rect::new(0, 0, 0, 0);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 10));
    render_opening(&mut buffer, area, &palette(), THREE, None);
}

#[test]
fn hollywood_adds_a_row() {
    let three = draw(120, 40, THREE, None);
    let four = draw(120, 40, FOUR, None);
    let (top3, bottom3) = card_rows(&three);
    let (top4, bottom4) = card_rows(&four);
    assert_eq!(bottom3 - top3 + 1, 13, "{three}");
    assert_eq!(bottom4 - top4 + 1, 14, "{four}");
    assert!(four.contains("Tell Hollywood"), "{four}");
}

#[test]
fn a_live_stage_drops_the_s_row() {
    let out = draw(
        120,
        40,
        OpeningOpts {
            hollywood: false,
            stage_running: true,
        },
        None,
    );
    assert!(!out.contains("Start the stage"), "{out}");
    let (top, bottom) = card_rows(&out);
    assert_eq!(bottom - top + 1, 12, "{out}");
}

#[test]
fn the_first_frame_fades_the_title_and_shifts_the_sprockets() {
    let first = draw(120, 40, THREE, Some(0));
    let last = draw(120, 40, THREE, Some(OPENING_FRAMES - 1));
    assert_ne!(first, last);
    // The sprocket row moves a column per frame.
    let sprocket_row = origin(120, 40).1 as usize;
    let first_row = first.lines().nth(sprocket_row).unwrap_or_default();
    let last_row = last.lines().nth(sprocket_row).unwrap_or_default();
    assert_ne!(first_row, last_row, "the sprockets shifted");
    // Everything else is there from the first frame, and the title
    // starts dimmed in surface1.
    assert!(first.contains("a film studio for coding agents"), "{first}");
    assert!(first.contains("New studio"), "{first}");
    let area = Rect::new(0, 0, 120, 40);
    let mut buffer = Buffer::empty(area);
    render_opening(&mut buffer, area, &palette(), THREE, Some(0));
    let (x, y) = origin(120, 40);
    let title = &buffer[(x + 11, y + 3)];
    assert_eq!(title.symbol(), "█");
    assert_eq!(title.fg, palette().surface1);
    assert!(!title.modifier.contains(Modifier::BOLD));
}

#[test]
fn the_last_frame_is_the_static_page() {
    let area = Rect::new(0, 0, 120, 40);
    let mut animated = Buffer::empty(area);
    let mut still = Buffer::empty(area);
    render_opening(
        &mut animated,
        area,
        &palette(),
        THREE,
        Some(OPENING_FRAMES - 1),
    );
    render_opening(&mut still, area, &palette(), THREE, None);
    assert_eq!(dump(&animated, area), dump(&still, area));
    assert_eq!(animated.content, still.content);
}

#[test]
fn reference_theme_uses_its_palette() {
    // Catppuccin — one reference theme: same rows, its own colours.
    let p = Palette::catppuccin();
    let out = draw_with(p.clone(), 120, 40, THREE, None);
    assert!(out.contains("New studio"));
    let area = Rect::new(0, 0, 120, 40);
    let mut buffer = Buffer::empty(area);
    render_opening(&mut buffer, area, &p, THREE, None);
    assert_eq!(buffer[(0, 0)].bg, p.panel_bg);
    let (x, y) = origin(120, 40);
    assert_eq!(buffer[(x, y + 1)].fg, p.accent);
}

#[test]
fn the_hint_is_legible_in_dark_and_light() {
    for (name, p) in [
        ("backlot", Palette::backlot()),
        ("catppuccin", Palette::catppuccin()),
        ("catppuccin-latte", Palette::catppuccin_latte()),
    ] {
        // The plain fallback is the only layout that still draws the
        // hint; 40×12 lands on it for every theme.
        let area = Rect::new(0, 0, 40, 12);
        let mut buffer = Buffer::empty(area);
        render_opening(&mut buffer, area, &p, THREE, None);
        let mut fg = None;
        'rows: for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                if buffer[(x, y)].symbol() == "b"
                    && buffer[(x, y)].bg == p.panel_bg
                    && buffer[(x, y)].fg != p.accent
                {
                    fg = Some(buffer[(x, y)].fg);
                    break 'rows;
                }
            }
        }
        let fg = fg.unwrap_or_else(|| panic!("a hint cell on {name}"));
        assert_eq!(fg, p.overlay0, "hint colour on {name}");
        assert_ne!(
            fg, p.surface_dim,
            "hint on {name} must not be the separator colour"
        );
    }
}

/// The report's screens, rendered through the same helper the tests
/// use — run with `--ignored --nocapture` to paste into the report.
#[test]
#[ignore = "prints the report's screens"]
fn report_screens() {
    for (w, h, frame) in [
        (120u16, 40u16, None),
        (120, 40, Some(0u8)),
        (60, 20, None),
        (44, 14, None),
        (40, 12, None),
    ] {
        println!(
            "--- {w}×{h} frame {frame:?} ---\n{}",
            draw(w, h, THREE, frame)
        );
    }
}
