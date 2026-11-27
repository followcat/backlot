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

pub(super) fn layer(mode: LayerMode) -> Hollywood {
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

pub(super) fn lines(buf: &Buffer, area: Rect) -> Vec<String> {
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

pub(super) fn stage_up(_: Option<&str>) -> (crate::hollywood::sheet::Conn, bool) {
    (crate::hollywood::sheet::Conn::Online, true)
}

fn stage_down(_: Option<&str>) -> (crate::hollywood::sheet::Conn, bool) {
    (crate::hollywood::sheet::Conn::Online, false)
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
    let status: &crate::hollywood::sheet::Status = if stage_running {
        &stage_up
    } else {
        &stage_down
    };
    let hits = render(
        &mut buf,
        area,
        &machines(summary),
        None,
        status,
        layer,
        opts,
        palette,
    );
    (buf, hits)
}

pub(super) fn draw(
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
            ..LayerOpts::default()
        },
        palette,
    )
}

pub(super) fn draw_without_agent(
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
            ..LayerOpts::default()
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
fn a_collapsed_bar_draws_nothing_its_face_is_the_lot_pill() {
    // The collapsed state is the chrome's lot pill now — the layer
    // itself paints nothing and reports no hits.
    let summary = summary();
    for palette in &palettes() {
        let area = Rect::new(0, 0, 120, 24);
        let (buf, hits) = draw(
            area,
            Some(&summary),
            true,
            &layer(LayerMode::Collapsed),
            false,
            palette,
        );
        assert!(hits.area.is_empty());
        assert!(hits.rows.is_empty());
        for x in 0..area.width {
            for y in 0..area.height {
                assert_eq!(buf[(x, y)].symbol(), " ", "x{x} y{y}");
            }
        }
    }
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
    let elev = palette.elevation();
    // Title sits directly on row 0 (no top perforation), bottom `▀▀` edge on last row.
    assert_eq!(buf[(0, 0)].bg, elev.surface);
    let title = row_of(&buf, area, 0);
    assert!(title.contains("✦  H O L L Y W O O D"), "{title}");
    assert!(title.contains("live"), "{title}");
    assert!(title.contains("fake / model-h / high"), "{title}");
    assert!(title.contains("3 studios"), "{title}");
    assert!(title.contains("2c"), "{title}");
    // Bucketed wait count (`2c`) is peach.
    let peach: Vec<u16> = (area.x..area.x + area.width)
        .filter(|x| buf[(*x, 0)].fg == palette.peach)
        .collect();
    assert!(!peach.is_empty(), "{title}: the waiting bucket count");
    let bottom = row_of(&buf, area, hits.area.height - 1);
    assert!(bottom.contains("▀▀"), "{bottom}");
    assert_eq!(buf[(1, hits.area.height - 1)].bg, palette.panel_bg);
    // Unfocused layout shows headline/wait rows, 1-row studio strip, and hints.
    assert!(text.contains("acme-shop needs a look"), "{text}");
    assert!(text.contains("CARD c3  acme-shop · pause t1"), "{text}");
    assert!(text.contains("STUDIOS 3"), "{text}");
    assert!(!hits.shadow_drawn);
    assert!(
        !text.contains("apply a"),
        "chips belong to the selected row only: {text}"
    );
    assert!(
        text.contains("i tell  tab ←→ ↑↓ ⏎  m menu  ? help  esc"),
        "{text}"
    );
    assert!(!text.contains("y/d a/x"), "{text}");
    assert!(!text.contains("↑↓ move"), "{text}");
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
        &selecting(LayerMode::Expanded, Target::Studio("acme-shop".into())),
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
    let mut hw = selecting(LayerMode::Expanded, Target::Studio("acme-shop".into()));
    let (buf, hits) = draw(area, Some(&summary), true, &hw, false, &palette);
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
    hw.frame_scroll = 2;
    let (buf, _) = draw(area, Some(&summary), true, &hw, false, &palette);
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
    // At 20 rows (cap 12) unfocused layout keeps the 1-row studio chip strip and `tell`.
    let area = Rect::new(0, 0, 160, 20);
    let (buf, hits) = draw(
        area,
        Some(&summary),
        true,
        &layer(LayerMode::Expanded),
        false,
        &palette,
    );
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("[acme-shop"), "{text}");
    assert!(text.contains("tell hollywood"), "{text}");
    assert!(hits.area.height <= 12, "{text}");
}

#[test]
fn the_frames_fold_into_the_chip_strip_under_pressure() {
    // Unfocused 80×24 (cap 12): STUDIOS folds into the 1-row chip strip
    // with `STUDIOS 3`, per-studio bucket counts, and pinned `[+ new]`.
    let summary = summary();
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 80, 24);
    let (buf, hits) = draw(
        area,
        Some(&summary),
        true,
        &layer(LayerMode::Expanded),
        false,
        &palette,
    );
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("STUDIOS 3"), "{text}");
    assert!(text.contains("[acme-shop ● 1c]"), "{text}");
    assert!(text.contains("[beta-tools ✓ 1c]"), "{text}");
    assert!(text.contains("[+ new]"), "{text}");
    assert!(hits.strip);
    let chips: Vec<Rect> = hits
        .rows
        .iter()
        .filter(|(_, t)| matches!(t, Target::Studio(_)))
        .map(|(rect, _)| *rect)
        .collect();
    assert!(!chips.is_empty(), "the chips carry the studio hits");
    assert!(chips.iter().all(|r| r.height == 1), "{chips:?}");
    assert!(text.contains("tell hollywood"), "{text}");
    assert!(text.contains("CARD c3"), "{text}");
}

/// R3-M10: a `ready_to_finish` or `finished` chip draws the shared
/// health read's `✓` — never a hardcoded live `●`.
#[test]
fn a_ready_studios_chip_draws_the_ready_mark() {
    let mut summary = summary();
    summary.studios[0].ready_to_finish = true;
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 80, 24);
    let (buf, _) = draw(
        area,
        Some(&summary),
        true,
        &layer(LayerMode::Expanded),
        false,
        &palette,
    );
    let text = lines(&buf, area).join("\n");
    // acme-shop ready (`✓ 1c`) and beta-tools finished (`✓ 1c`) both read `✓`;
    // gamma-site's ledger error reads `×`.
    assert!(text.contains("[acme-shop ✓"), "{text}");
    assert!(text.contains("[beta-tools ✓"), "{text}");
    assert!(!text.contains("[gamma-site ✓"), "{text}");
}

// ---------- the footer ----------

#[test]
fn a_prompt_row_footer_names_y_d_answer() {
    // D9: alice's user-owned prompt is a sheet row — its footer
    // names the answers it takes, the peek and the tell line, then
    // the fixed right block.
    let summary = with_user_prompt("acme-shop", "actor-alice");
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 160, 45);
    let selected = Target::Prompt(
        crate::studio::StudioKey::local("acme-shop"),
        "actor-alice".into(),
    );
    let (buf, _) = draw(
        area,
        Some(&summary),
        true,
        &selecting(LayerMode::Expanded, selected),
        false,
        &palette,
    );
    let text = lines(&buf, area).join("\n");
    assert!(
        text.contains("y/d answer  p peek  i tell  tab ←→ ↑↓ ⏎  m menu  ? help  esc"),
        "{text}"
    );
}

/// A prompt row on a runner that cannot answer: the footer names no
/// `y/d answer`, and the row carries no `allow` chip — Enter and `p`
/// stay.
#[test]
fn a_prompt_the_runner_cannot_answer_names_neither_y_nor_d() {
    let mut value = summary_json();
    for s in value["studios"].as_array_mut().unwrap() {
        if s["id"] == "acme-shop" {
            for r in s["roles"].as_array_mut().unwrap() {
                if r["role"] == "actor-alice" {
                    r["attention_user"] = true.into();
                    r["attention_seq"] = 7.into();
                    r["attention_detail"] = "may I run the tests?".into();
                    r["can_answer_prompt"] = false.into();
                }
            }
        }
    }
    let summary: Summary = serde_json::from_value(value).unwrap();
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 160, 45);
    let selected = Target::Prompt(
        crate::studio::StudioKey::local("acme-shop"),
        "actor-alice".into(),
    );
    let (buf, _) = draw(
        area,
        Some(&summary),
        true,
        &selecting(LayerMode::Expanded, selected),
        false,
        &palette,
    );
    let text = lines(&buf, area).join("\n");
    assert!(!text.contains("y/d answer"), "{text}");
    assert!(!text.contains("allow y"), "{text}");
    assert!(!text.contains("deny d"), "{text}");
    assert!(text.contains("answer in its terminal (Enter)"), "{text}");
    assert!(text.contains("p peek"), "{text}");
}

/// The live row draws right under the newest `user ›` line: the
/// spinner mark and Hollywood's own `doing`.
#[test]
fn the_live_row_draws_under_the_user_line() {
    let mut value = summary_json();
    value["hollywood"]["exchange"] =
        serde_json::json!([{ "seq": 8, "who": "user", "text": "recast the producer" }]);
    value["hollywood"]["work"] = "working".into();
    value["hollywood"]["doing"] = "reading the ledgers".into();
    let summary: Summary = serde_json::from_value(value).unwrap();
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 160, 45);
    let (buf, _) = draw(
        area,
        Some(&summary),
        true,
        &selecting(LayerMode::Expanded, Target::Input),
        false,
        &palette,
    );
    let rows = lines(&buf, area);
    let at = rows
        .iter()
        .position(|l| l.contains("recast the producer"))
        .expect("the user line");
    assert!(
        rows[at].contains("› you") && rows[at].contains("recast the producer"),
        "{rows:?}"
    );
    assert!(
        rows[at + 1].contains("⠶ hollywood: reading the ledgers"),
        "{rows:?}"
    );
}

/// A card Hollywood proposed after the person's line draws as a talk
/// line under it, and the idle live row under that line names the
/// missing answer.
#[test]
fn a_card_line_draws_and_the_idle_row_names_the_missing_answer() {
    let mut value = summary_json();
    value["hollywood"]["exchange"] = serde_json::json!([
        { "seq": 8, "who": "user", "text": "recast the producer and the director" },
        { "seq": 9, "who": "hollywood", "kind": "card",
          "text": "proposed card c3 · acme-shop · pause t1" },
    ]);
    value["hollywood"]["work"] = "waiting".into();
    let summary: Summary = serde_json::from_value(value).unwrap();
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 160, 45);
    let (buf, _) = draw(
        area,
        Some(&summary),
        true,
        &selecting(LayerMode::Expanded, Target::Input),
        false,
        &palette,
    );
    let rows = lines(&buf, area);
    let card = rows
        .iter()
        .position(|l| l.contains("proposed card c3 · acme-shop · pause t1"))
        .expect("the card line");
    assert!(rows[card].trim_start().starts_with('✦'), "{rows:?}");
    assert!(
        rows[card - 1].contains("› you")
            && rows[card - 1].contains("recast the producer and the director"),
        "the card sits under the person's line: {rows:?}"
    );
    // The card ends the live row, and the idle-Hollywood line shows on
    // a later unanswered tell only; here the card answered it.
    assert!(
        !rows
            .iter()
            .any(|l| l.contains("hollywood is idle and has not answered")),
        "no idle row after a card: {rows:?}"
    );
}

/// An unanswered `user ›` line under an idle Hollywood draws the idle
/// live row word.
#[test]
fn an_idle_unanswered_line_draws_the_idle_row() {
    let mut value = summary_json();
    value["hollywood"]["exchange"] =
        serde_json::json!([{ "seq": 8, "who": "user", "text": "recast the producer" }]);
    value["hollywood"]["work"] = "waiting".into();
    let summary: Summary = serde_json::from_value(value).unwrap();
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 160, 45);
    let (buf, _) = draw(
        area,
        Some(&summary),
        true,
        &selecting(LayerMode::Expanded, Target::Input),
        false,
        &palette,
    );
    let text = lines(&buf, area).join("\n");
    assert!(
        text.contains("hollywood is idle and has not answered · open ↵"),
        "{text}"
    );
}

/// A tweet-sized long report from Hollywood over the person's line:
/// its point sits at the end, past a single band row.
fn long_report_summary() -> Summary {
    let long = "the producer of acme-shop has stopped at its prompt and will not deliver until you \
                answer; there is only one of you and the lot is busy, so choose 1 to keep waiting \
                or 2 to recast the producer onto a fresh identity now";
    let mut value = summary_json();
    value["hollywood"]["exchange"] = serde_json::json!([
        { "seq": 1, "who": "user", "text": "status?" },
        { "seq": 2, "who": "hollywood", "text": long },
    ]);
    serde_json::from_value(value).unwrap()
}

/// Any exchange line wraps up to 2 lines and clamps with `…`; short ones keep 1 row.
#[test]
fn the_newest_report_wraps_and_clamps_at_two_lines() {
    let summary = long_report_summary();
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 80, 45);
    let (buf, _) = draw(
        area,
        Some(&summary),
        true,
        &selecting(LayerMode::Expanded, Target::Input),
        false,
        &palette,
    );
    let rows = lines(&buf, area);
    let first = rows
        .iter()
        .position(|l| l.contains("✦ hw") && l.contains("the producer of acme-shop"))
        .expect("the wrap's first row");
    // Wraps to 2 lines with continuation indented under body, clamped with `…`.
    assert!(
        !rows[first + 1].contains("✦ hw") && rows[first + 1].trim_end().ends_with('…'),
        "2nd line continues body with ellipsis: {:?}",
        &rows[first..=(first + 2).min(rows.len() - 1)]
    );
    // The short user line stays on one row with fixed narrow speaker col.
    let user = rows
        .iter()
        .position(|l| l.contains("› you") && l.contains("status?"))
        .expect("the user line");
    assert!(!rows[user + 1].contains("status?"));
}

/// A focused HOLLYWOOD band gives wraps back oldest-first, then drops
/// older talk before ever dropping a waiting row.
#[test]
fn a_short_band_gives_the_wrap_back_before_dropping_a_wait() {
    let mut value = summary_json();
    let long = "the producer of acme-shop has stopped at its prompt and will not deliver until you \
                answer; there is only one of you and the lot is busy, so choose 1 to keep waiting";
    value["hollywood"]["headline"] = serde_json::Value::Null;
    value["hollywood"]["exchange"] = serde_json::json!([
        { "seq": 1, "who": "hollywood", "text": long },
        { "seq": 2, "who": "user", "text": "status?" },
        { "seq": 3, "who": "hollywood", "text": long },
    ]);
    let summary: Summary = serde_json::from_value(value).unwrap();
    let list = machines(Some(&summary));
    let status: &crate::hollywood::sheet::Status = &stage_up;
    let (waits, rows) = crate::hollywood::band_rows(&list, status, true);
    let mut hollywood = layer(LayerMode::Expanded);
    hollywood.selected = Some(Target::Input);
    let opts = LayerOpts {
        agent: true,
        mobile: true,
        ..LayerOpts::default()
    };

    let full =
        crate::hollywood::plan::plan(Rect::new(0, 0, 60, 14), &waits, &rows, &hollywood, opts);
    assert_eq!(full.talk, 3);
    assert_eq!(full.wrap_rows(), 2);
    assert_eq!(full.shown, waits.len());

    // One row short: oldest wrap shrinks 2->1 first while newest wrap & waits stay.
    let cap_full = full.height();
    let short = crate::hollywood::plan::plan(
        Rect::new(0, 0, 60, cap_full - 1),
        &waits,
        &rows,
        &hollywood,
        opts,
    );
    assert_eq!(short.wrap_rows(), 1);
    assert_eq!(short.talk_wraps.last().copied(), Some(1));
    assert_eq!(short.shown, waits.len());

    // Tighter heights shed older talk and wraps before any waiting row drops.
    let shorter = crate::hollywood::plan::plan(
        Rect::new(0, 0, 60, cap_full - 2),
        &waits,
        &rows,
        &hollywood,
        opts,
    );
    assert!(shorter.talk <= 3 && shorter.wrap_rows() <= 1);
    assert_eq!(shorter.shown, waits.len());

    let tight = crate::hollywood::plan::plan(
        Rect::new(0, 0, 60, cap_full - 4),
        &waits,
        &rows,
        &hollywood,
        opts,
    );
    assert_eq!(tight.wrap_rows(), 0);
    assert!(tight.talk < 3);
    assert_eq!(tight.shown, waits.len());
}

/// At a narrow 60-col width, a long message wraps up to two lines when
/// the band has room ahead of frames-to-chips folding.
#[test]
fn a_mobile_band_wraps_the_newest_line_at_two_rows() {
    let summary = long_report_summary();
    let area = Rect::new(0, 0, 60, 48);
    let list = machines(Some(&summary));
    let status: &crate::hollywood::sheet::Status = &stage_up;
    let (waits, rows) = crate::hollywood::band_rows(&list, status, true);
    let hollywood = selecting(LayerMode::Expanded, Target::Input);
    let plan = crate::hollywood::plan::plan(
        area,
        &waits,
        &rows,
        &hollywood,
        LayerOpts {
            agent: true,
            mobile: false,
            ..LayerOpts::default()
        },
    );
    assert_eq!(
        plan.wrap_rows(),
        1,
        "exchange wrap caps at two rows (one extra row)"
    );
}

/// A selected Hollywood prompt row names the answers it takes — the
/// same `y/d answer` and `p peek` a studio prompt rows carries.
#[test]
fn a_selected_hollywood_prompt_names_its_answers() {
    let mut value = summary_json();
    value["hollywood"]["exchange"] =
        serde_json::json!([{ "seq": 8, "who": "user", "text": "recast the producer" }]);
    value["hollywood"]["work"] = "blocked".into();
    value["hollywood"]["attention_user"] = true.into();
    value["hollywood"]["attention_detail"] = "backlot --help".into();
    let summary: Summary = serde_json::from_value(value).unwrap();
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 160, 45);
    let (buf, _) = draw(
        area,
        Some(&summary),
        true,
        &selecting(LayerMode::Expanded, Target::Hollywood),
        false,
        &palette,
    );
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("⚠ HOLLYWOOD asks: backlot --help"), "{text}");
    assert!(
        text.contains("y/d answer  p peek  i tell  tab ←→ ↑↓ ⏎  m menu  ? help  esc"),
        "{text}"
    );
    // Focusing HOLLYWOOD (`Target::HollywoodLive`) shows the live prompt line too.
    let (lbuf, _) = draw(
        area,
        Some(&summary),
        true,
        &selecting(LayerMode::Expanded, Target::HollywoodLive),
        false,
        &palette,
    );
    let ltext = lines(&lbuf, area).join("\n");
    assert!(
        ltext.contains("⚠ hollywood asks: backlot --help"),
        "{ltext}"
    );
}

#[test]
fn a_row_without_answers_never_names_their_keys() {
    // The calm row and the tell line run no row keys — their footers
    // name only `i tell` and the fixed right block; Headline/Talk
    // spell out `⏎ read` for their CardView open.
    let summary = summary();
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 160, 45);
    for selected in [Target::Calm, Target::Input] {
        let (buf, _) = draw(
            area,
            Some(&summary),
            true,
            &selecting(LayerMode::Expanded, selected.clone()),
            false,
            &palette,
        );
        let text = lines(&buf, area).join("\n");
        assert!(
            text.contains("i tell  tab ←→ ↑↓ ⏎  m menu  ? help  esc"),
            "{selected:?}: {text}"
        );
        assert!(!text.contains("y/d answer"), "{selected:?}: {text}");
        assert!(!text.contains("a/x card"), "{selected:?}: {text}");
        assert!(!text.contains("f finish"), "{selected:?}: {text}");
        assert!(!text.contains("s start"), "{selected:?}: {text}");
    }
    for selected in [Target::Headline, Target::Talk(1)] {
        let (buf, _) = draw(
            area,
            Some(&summary),
            true,
            &selecting(LayerMode::Expanded, selected.clone()),
            false,
            &palette,
        );
        let text = lines(&buf, area).join("\n");
        assert!(
            text.contains("i tell  tab ↑↓ ⏎ read  m menu  ? help  esc"),
            "{selected:?}: {text}"
        );
        assert!(!text.contains("y/d answer"), "{selected:?}: {text}");
        assert!(!text.contains("a/x card"), "{selected:?}: {text}");
    }
}

#[test]
fn a_card_row_footer_names_a_x_card() {
    let summary = summary();
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 160, 45);
    let selected = Target::Card("c3".into());
    let (buf, _) = draw(
        area,
        Some(&summary),
        true,
        &selecting(LayerMode::Expanded, selected),
        false,
        &palette,
    );
    let text = lines(&buf, area).join("\n");
    assert!(
        text.contains("a/x card  i tell  tab ←→ ↑↓ ⏎  m menu  ? help  esc"),
        "{text}"
    );
}

#[test]
fn a_ready_studio_footer_names_f_finish_from_row_and_frame() {
    // The READY row and the studio's own frame are one studio: both
    // arm `f`, so both footers name it. The row is `f`/`↵` only —
    // peek stays on the frame (R3-N5).
    let mut summary = summary();
    summary.studios[0].ready_to_finish = true;
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 160, 45);
    let key = crate::studio::StudioKey::local("acme-shop");
    let cases = [
        (
            Target::Ready(key.clone()),
            "f finish  i tell  tab ←→ ↑↓ ⏎  m menu  ? help  esc",
        ),
        (
            Target::Studio(key),
            "f finish  p peek  i tell  tab ←→ ↑↓ ⏎  m menu  ? help  esc",
        ),
    ];
    for (selected, footer) in cases {
        let (buf, _) = draw(
            area,
            Some(&summary),
            true,
            &selecting(LayerMode::Expanded, selected.clone()),
            false,
            &palette,
        );
        let text = lines(&buf, area).join("\n");
        assert!(text.contains(footer), "{selected:?}: {text}");
    }
}

/// R3-3: while the wrap report is still owed the READY row carries
/// a dim `report pending` under it — gone once `next` is a normal
/// step again.
#[test]
fn a_ready_row_names_the_pending_report_in_a_dim_line() {
    let mut pending = summary();
    pending.studios[0].ready_to_finish = true;
    pending.studios[0].next = Some("node report — director".into());
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 160, 45);
    let layer = selecting(
        LayerMode::Expanded,
        Target::Ready(crate::studio::StudioKey::local("acme-shop")),
    );
    let (buf, _) = draw(area, Some(&pending), true, &layer, false, &palette);
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("report pending"), "{text}");

    let mut landed = summary();
    landed.studios[0].ready_to_finish = true;
    let (buf, _) = draw(area, Some(&landed), true, &layer, false, &palette);
    let text = lines(&buf, area).join("\n");
    assert!(!text.contains("report pending"), "{text}");
}

/// R3-N5/R3-3: an armed `finish <id>?` owns every key but `y`/`n`/
/// `Esc` — the footer names just those, and the ask itself warns
/// while the wrap report is still pending. The frame's `f` arms the
/// row's ask too.
#[test]
fn an_armed_finish_confirm_names_yes_no_and_warns_on_a_pending_report() {
    let mut pending = summary();
    pending.studios[0].ready_to_finish = true;
    pending.studios[0].next = Some("node report — director".into());
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 160, 45);
    let key = crate::studio::StudioKey::local("acme-shop");
    // The selection sits on the studio frame — the confirm still
    // draws on the READY row it targets.
    let mut layer = selecting(LayerMode::Expanded, Target::Studio(key.clone()));
    layer.confirm = Some(crate::hollywood::Confirm::Finish(key));
    let (buf, _) = draw(area, Some(&pending), true, &layer, false, &palette);
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("finish studio (report pending)?"), "{text}");
    assert!(text.contains("y confirm  n / esc cancel"), "{text}");
    assert!(!text.contains("p peek"), "{text}");

    // Report landed: the ask drops the warning, the footer stays.
    let mut landed = summary();
    landed.studios[0].ready_to_finish = true;
    let (buf, _) = draw(area, Some(&landed), true, &layer, false, &palette);
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("finish studio?"), "{text}");
    assert!(!text.contains("(report pending)"), "{text}");
    assert!(text.contains("y confirm  n / esc cancel"), "{text}");
}

#[test]
fn a_narrow_prompt_row_still_names_y_d() {
    // At the 40-column floor the compact tier keeps the row's keys
    // and the menu hint beside the bare block.
    let summary = with_user_prompt("acme-shop", "actor-alice");
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 40, 24);
    let selected = Target::Prompt(
        crate::studio::StudioKey::local("acme-shop"),
        "actor-alice".into(),
    );
    let (buf, _) = draw(
        area,
        Some(&summary),
        true,
        &selecting(LayerMode::Expanded, selected),
        true,
        &palette,
    );
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("y/d p i  tab ←→ ↑↓ ⏎  m menu  esc"), "{text}");
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
    // R22: the line is the dialog's button — `⏎ hollywood setup`.
    assert!(text.contains("hollywood setup"), "{text}");
    assert!(!text.contains("tell hollywood what"), "{text}");
    assert!(!text.contains("acme-shop needs a look"), "{text}");
    assert!(hits.input.is_empty(), "no input line without an agent");
}

/// R3-M13: the setup row's lead shortens through clean tiers before
/// the `⏎ hollywood setup` button ever clips — full sentence, then
/// `✦ watch every studio:`, then the bare button.
#[test]
fn the_setup_row_shortens_its_lead_before_the_button_clips() {
    let summary = summary();
    for (width, want, not) in [
        (120u16, "an agent can watch every studio", ""),
        (60, "✦ watch every studio:", "an agent can watch"),
        (36, "", "watch every studio"),
    ] {
        let area = Rect::new(0, 0, width, 24);
        let (buf, _) = draw_without_agent(
            area,
            Some(&summary),
            true,
            &layer(LayerMode::Expanded),
            &palettes()[0],
        );
        let text = lines(&buf, area).join("\n");
        assert!(text.contains("⏎ hollywood setup"), "{width}: {text}");
        if !want.is_empty() {
            assert!(text.contains(want), "{width}: {text}");
        }
        if !not.is_empty() {
            assert!(!text.contains(not), "{width}: {text}");
        }
    }
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
    let elev = palette.elevation();
    assert_eq!(buf[(frame.x + 1, frame.y + 1)].bg, elev.raised_row);
    let other = frame_hit(&hits, &Target::Studio("beta-tools".into())).expect("beta-tools");
    assert_eq!(buf[(other.x + 1, other.y + 1)].bg, palette.panel_bg);
    assert_eq!(buf[(other.x, other.y)].fg, elev.border_muted);
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
        &selecting(LayerMode::Expanded, Target::Studio("acme-shop".into())),
        false,
        &palette,
    );
    let frame = frame_hit(&hits, &Target::Studio("acme-shop".into())).expect("acme-shop");
    let state = row_of(&buf, area, frame.y + 1);
    assert!(state.contains("● live"), "{state}");
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
    // One unchipped space separates the counts and the badge —
    // `0/0  !2 ` reads apart (round 2).
    assert_eq!(buf[(chip[0] - 1, frame.y + 1)].symbol(), " ");
    assert_ne!(buf[(chip[0] - 1, frame.y + 1)].bg, palette.peach);
    // The working role is on set, then what it is doing.
    let who = row_of(&buf, area, frame.y + 2);
    assert!(who.contains("Ⓟ"), "{who}");
    let what = row_of(&buf, area, frame.y + 3);
    assert!(what.contains("review delivery 1"), "{what}");
    // The cast line: one mark each, coloured by role state.
    let cast = row_of(&buf, area, frame.y + 5);
    assert!(cast.contains("◐ ! ! ○"), "{cast}");
    assert!(cast.contains("4 cast"), "{cast}");
    let marks: Vec<Color> = (frame.x..frame.x + frame.width)
        .filter(|x| matches!(buf[(*x, frame.y + 5)].symbol(), "◐" | "!" | "○" | "◦"))
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
        let elev = palette.elevation();
        let mut allowed = vec![
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
            elev.surface,
            elev.raised_row,
            elev.raised_field,
            elev.border_muted,
            elev.muted_surface,
            elev.muted_raised,
            crate::studio::glyphs::contrast_fg(&palette),
        ];
        if let Some(d) = elev.dim_bg {
            allowed.push(d);
        }
        if let Some(s) = elev.shadow {
            allowed.push(s);
        }
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
        let status: &crate::hollywood::sheet::Status = &stage_up;
        let hits = render(
            &mut buf,
            area,
            &machines(Some(&summary)),
            None,
            status,
            &layer(mode),
            LayerOpts {
                agent: true,
                mobile: false,
                ..LayerOpts::default()
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
    let view = CardView {
        card: Some("c4".into()),
        title: "c4".into(),
        lines: (0..30).map(|i| format!("draft line {i}")).collect(),
        scroll: 0,
        ..CardView::default()
    };
    let area = Rect::new(0, 0, 80, 24);
    let mut buf = Buffer::empty(area);
    let view_hits = crate::hollywood::render_view(&mut buf, area, &view, None, &palettes()[0]);
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("draft line 0"), "{text}");
    assert!(text.contains("╭"), "{text}");
    assert!(text.contains("c4"), "{text}");
    assert!(view_hits.metrics.unwrap().max_offset_from_bottom > 0);
}

fn border_of(buf: &Buffer, outer: Rect) -> Rect {
    if outer.width >= 4 && outer.height >= 4 && buf[(outer.x, outer.y)].symbol() == " " {
        Rect::new(outer.x + 1, outer.y + 1, outer.width - 2, outer.height - 2)
    } else {
        outer
    }
}

#[test]
fn the_card_view_has_a_complete_border_and_footer() {
    // §4: the view is a real box — `─` fills the top edge after the
    // title, `│` runs down both sides, and the bottom edge carries
    // `a apply  x cancel  esc back`.
    let view = CardView {
        card: Some("c4".into()),
        title: "c4".into(),
        lines: vec!["cast: alice".into()],
        scroll: 0,
        ..CardView::default()
    };
    let area = Rect::new(0, 0, 80, 24);
    let mut buf = Buffer::empty(area);
    let palette = palettes()[0].clone();
    let view = crate::hollywood::render_view(&mut buf, area, &view, None, &palette);
    let b = border_of(&buf, view.area);
    let top = row_of(&buf, area, b.y);
    assert!(top.contains('╭') && top.contains('╮'), "top edge: {top}");
    // Everything after the title up to `╮` is `─`.
    let title_end = top.rfind("c4 ").expect("the title is on the top edge") + 3;
    let fill = &top[title_end..top.rfind('╮').unwrap()];
    assert!(fill.chars().all(|c| c == '─'), "top fill: {fill:?}");
    let column = |x: u16| {
        (b.y..b.y + b.height)
            .map(|y| buf[(x, y)].symbol())
            .collect::<String>()
    };
    let left_side: Vec<&str> = (b.y + 1..b.bottom() - 1)
        .map(|y| buf[(b.x, y)].symbol())
        .collect();
    let right_side: Vec<&str> = (b.y + 1..b.bottom() - 1)
        .map(|y| buf[(b.right() - 1, y)].symbol())
        .collect();
    assert_eq!(left_side[left_side.len() - 2], "├");
    assert_eq!(right_side[right_side.len() - 2], "┤");
    assert!(
        left_side
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != left_side.len() - 2)
            .all(|(_, s)| *s == "│"),
        "left edge: {left_side:?}"
    );
    assert!(
        right_side
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != right_side.len() - 2)
            .all(|(_, s)| *s == "│"),
        "right edge: {right_side:?}"
    );
    let _ = column;
    let action_strip = row_of(&buf, area, b.bottom() - 2);
    assert!(
        action_strip.contains("[a] apply")
            && action_strip.contains("[x] cancel")
            && action_strip.contains("esc"),
        "action strip: {action_strip}"
    );
    let bottom = row_of(&buf, area, b.bottom() - 1);
    assert!(
        bottom.contains('╰') && bottom.contains('╯'),
        "bottom: {bottom}"
    );
}

#[test]
fn the_card_views_footer_shows_the_pending_mark() {
    // An action kicked off inside the view locks `a`/`x` and names the pending action.
    let view = CardView {
        card: Some("c4".into()),
        title: "c4".into(),
        lines: vec!["cast: alice".into()],
        scroll: 0,
        ..CardView::default()
    };
    let area = Rect::new(0, 0, 80, 24);
    let mut buf = Buffer::empty(area);
    let view =
        crate::hollywood::render_view(&mut buf, area, &view, Some("card apply c4"), &palettes()[0]);
    let b = border_of(&buf, view.area);
    let strip = row_of(&buf, area, b.bottom() - 2);
    assert!(
        strip.contains("■ card apply c4… (a/x locked)"),
        "action strip: {strip}"
    );
}

#[test]
fn a_read_only_card_view_draws_the_scroll_footer_without_card_keys() {
    let view = CardView {
        card: None,
        title: "hollywood #7".into(),
        lines: vec![
            "two studios checked".into(),
            String::new(),
            "detail line".into(),
        ],
        scroll: 0,
        ..CardView::default()
    };
    let area = Rect::new(0, 0, 80, 24);
    let mut buf = Buffer::empty(area);
    let view = crate::hollywood::render_view(&mut buf, area, &view, None, &palettes()[0]);
    let b = border_of(&buf, view.area);
    let top = row_of(&buf, area, b.y);
    assert!(top.contains("hollywood #7"), "top: {top}");
    let strip = row_of(&buf, area, b.bottom() - 2);
    assert!(
        strip.contains("[y] copy") && strip.contains("esc"),
        "read-only action strip: {strip}"
    );
    assert!(!strip.contains("[a] apply"), "no card keys: {strip}");
    assert!(!strip.contains("[x] cancel"), "no card keys: {strip}");
}

#[test]
fn the_stage_and_ready_rows_compact_like_the_others() {
    // At mobile widths every waiting row draws `[k]` chips — the
    // stage and ready rows cannot keep their full labels while the
    // prompt and card rows fold (round 2).
    let mut local = summary();
    local.studios[0].ready_to_finish = true;
    local.studios[0].tasks_done = 0;
    local.studios[0].tasks_total = 1;
    let list = vec![
        crate::panel::data::MachineSummary {
            machine: None,
            summary: local,
        },
        remote(&summary()),
    ];
    let status: &crate::hollywood::sheet::Status = &|machine| match machine {
        None => (crate::hollywood::sheet::Conn::Online, true),
        Some(_) => (crate::hollywood::sheet::Conn::Online, false),
    };
    for width in [40u16, 60] {
        let area = Rect::new(0, 0, width, 24);
        let mut buf = Buffer::empty(area);
        render(
            &mut buf,
            area,
            &list,
            Some(crate::project::LOCAL_LABEL),
            &status,
            &layer(LayerMode::Expanded),
            LayerOpts {
                agent: true,
                mobile: true,
                ..LayerOpts::default()
            },
            &palettes()[0],
        );
        let text = lines(&buf, area).join("\n");
        let stage = text
            .lines()
            .find(|l| l.contains("STAGE STOPPED"))
            .unwrap_or("");
        let ready = text.lines().find(|l| l.contains("READY")).unwrap_or("");
        assert!(
            !stage.contains("[s]") && !ready.contains("[f]") && !ready.contains("[↵]"),
            "unselected rows carry no chips at {width}: {stage} / {ready}\n{text}"
        );
    }
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
        &selecting(LayerMode::Expanded, Target::NewStudio),
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
    assert_eq!(buf[(new.x, new.y)].fg, palette.accent);
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
    let status: &crate::hollywood::sheet::Status = &stage_up;
    let hits = render(
        &mut buf,
        area,
        &list,
        Some(crate::project::LOCAL_LABEL),
        &status,
        &selecting(LayerMode::Expanded, Target::Studio("acme-shop".into())),
        LayerOpts {
            agent: true,
            mobile: false,
            ..LayerOpts::default()
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
fn the_label_fits_through_the_machine_truncation_steps() {
    // §3: the machine truncates to four chars plus `…`, the
    // separator shortens to `@`, the id truncates last — the
    // machine is never dropped whole (A8.3).
    let label = crate::panel::data::studio_label("beta-tools", Some("host-b"), Some("Local"), true);
    assert_eq!(label.fit(19), "beta-tools · host-b");
    assert_eq!(label.fit(18), "beta-tools · host…");
    assert_eq!(label.fit(16), "beta-tools@host…");
    assert_eq!(label.fit(14), "beta-to…@host…");
    // A bare id still truncates on its own.
    let local = crate::panel::data::studio_label("acme-shop", None, Some("Local"), false);
    assert_eq!(local.fit(6), "acme-…");
}

/// A studio frame carries the studio's own next step on its blank
/// row — `next: director reviewing` while reviews wait — and a calm
/// studio leaves the row blank.
#[test]
fn a_studio_frame_shows_its_next_step() {
    let mut value = summary_json();
    value["studios"][0]["open_reviews"] = serde_json::json!(2);
    let reviewing: Summary = serde_json::from_value(value).unwrap();
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 160, 45);
    let (buf, hits) = draw(
        area,
        Some(&reviewing),
        true,
        &selecting(LayerMode::Expanded, Target::Studio("acme-shop".into())),
        false,
        &palette,
    );
    let frame = frame_hit(&hits, &Target::Studio("acme-shop".into())).expect("acme-shop");
    let next: String = (frame.x..frame.x + frame.width)
        .map(|x| buf[(x, frame.y + 4)].symbol())
        .collect();
    assert!(next.contains("next: director reviewing"), "{next}");

    let calm = summary();
    let (buf, hits) = draw(
        area,
        Some(&calm),
        true,
        &selecting(LayerMode::Expanded, Target::Studio("acme-shop".into())),
        false,
        &palette,
    );
    let frame = frame_hit(&hits, &Target::Studio("acme-shop".into())).expect("acme-shop");
    let calm_row: String = (frame.x..frame.x + frame.width)
        .map(|x| buf[(x, frame.y + 4)].symbol())
        .collect();
    assert!(!calm_row.contains("next:"), "{calm_row}");
}

fn wide_row_of(buf: &Buffer, x0: u16, x1: u16, y: u16) -> String {
    use unicode_width::UnicodeWidthStr;
    let mut out = String::new();
    let mut x = x0;
    while x < x1 {
        let sym = buf[(x, y)].symbol();
        out.push_str(sym);
        x += UnicodeWidthStr::width(sym).max(1) as u16;
    }
    out.trim_end().to_string()
}

#[test]
fn card_view_cjk_title_places_right_corners_at_exact_right_edge() {
    let view = CardView {
        card: None,
        title: "OUTCOME #188 · acme-shop · 已记录最终需求 v6".into(),
        lines: vec!["ok".into()],
        scroll: 0,
        ..CardView::default()
    };
    let area = Rect::new(0, 0, 80, 24);
    let mut buf = Buffer::empty(area);
    let rendered = crate::hollywood::render_view(&mut buf, area, &view, None, &palettes()[0]);
    let b = border_of(&buf, rendered.area);
    let right_x = b.right() - 1;
    let bottom_y = b.bottom() - 1;
    assert_eq!(buf[(right_x, b.y)].symbol(), "╮");
    assert_eq!(buf[(right_x, bottom_y)].symbol(), "╯");
    let top = wide_row_of(&buf, b.x, b.right(), b.y);
    assert!(
        top.contains("已记录最终需求 v6"),
        "CJK title must not be overwritten by ─ dashes: {top}"
    );
}

#[test]
fn card_view_wraps_cjk_and_paths_without_ellipsis_and_keeps_kinsoku() {
    let cjk_90 = "已记录最终需求版本六，并已正式向导演派发方案甲。我们要求每条路径都完整展示，不能在行尾截断成省略号，标点符号也要紧贴前一个字符。";
    let path_90 = "crates/hc-tui/src/hollywood/deeply/nested/module/for/testing/long_path/reader_wrap_boundary.rs";
    let view = CardView {
        card: None,
        title: "wrap check".into(),
        lines: vec![cjk_90.into(), path_90.into()],
        scroll: 0,
        ..CardView::default()
    };
    let area = Rect::new(0, 0, 80, 24);
    let mut buf = Buffer::empty(area);
    let rendered = crate::hollywood::render_view(&mut buf, area, &view, None, &palettes()[0]);
    let body_rows: Vec<String> = (rendered.body.y..rendered.body.bottom())
        .map(|y| {
            wide_row_of(&buf, rendered.body.x, rendered.body.right(), y)
                .trim()
                .to_string()
        })
        .filter(|s| !s.is_empty())
        .collect();
    let joined: String = body_rows.concat();
    assert!(!joined.contains('…'), "no ellipsis loss: {body_rows:?}");
    assert!(
        joined.contains("标点符号也要紧贴前一个字符。"),
        "full CJK paragraph preserved across wrapped rows: {body_rows:?}"
    );
    assert!(
        joined.contains("reader_wrap_boundary.rs"),
        "full 90-char path preserved across wrapped rows: {body_rows:?}"
    );
    const CLOSING: &[char] = &[
        '，', '。', '：', '；', '！', '？', '、', '）', '》', '」', '』', '】', '.', ',', ':', ';',
        '!', '?', ')', ']', '}',
    ];
    for row in &body_rows {
        let first = row.chars().next().unwrap();
        assert!(
            !CLOSING.contains(&first),
            "wrapped row started with closing punctuation '{first}': {row:?}"
        );
    }
    assert!(
        body_rows.iter().any(|r| r.ends_with('/')),
        "90-char path should prefer breaking after '/': {body_rows:?}"
    );
}

#[test]
fn card_view_sizes_adaptively_for_short_and_long_notes() {
    let area = Rect::new(0, 0, 200, 44);

    let short = CardView {
        card: None,
        title: "OUTCOME #188".into(),
        lines: vec!["line one".into(), "line two".into()],
        scroll: 0,
        ..CardView::default()
    };
    let mut buf = Buffer::empty(area);
    let short_rendered =
        crate::hollywood::render_view(&mut buf, area, &short, None, &palettes()[0]);
    assert!(
        short_rendered.area.height <= 12,
        "2-line note should render compactly (<=12 including gutter/pad), got {}",
        short_rendered.area.height
    );

    let long = CardView {
        card: None,
        title: "REPORT #188".into(),
        lines: (1..=60).map(|i| format!("report line {i}")).collect(),
        scroll: 0,
        ..CardView::default()
    };
    let mut buf = Buffer::empty(area);
    let long_rendered = crate::hollywood::render_view(&mut buf, area, &long, None, &palettes()[0]);
    let long_border = border_of(&buf, long_rendered.area);
    assert_eq!(
        long_border.height, 34,
        "60-line note at 200x44 should cap border at min(34, 44 - 2) = 34"
    );
    assert_eq!(
        long_rendered.area.height, 36,
        "1-cell outer gutter wraps the 34-row border"
    );
    assert!(
        !long_rendered.scrollbar.is_empty(),
        "60-line note should show scrollbar"
    );
    let bottom = wide_row_of(
        &buf,
        long_border.x,
        long_border.right(),
        long_border.bottom() - 1,
    );
    assert!(
        bottom.contains("lines 1–") && bottom.contains(" of 60"),
        "bottom border should show scroll range: {bottom}"
    );
}

#[test]
fn card_view_renders_markdown_lite_styles_and_plain_log_mode() {
    let palette = palettes()[0].clone();
    let md = CardView {
        card: None,
        title: "reports/188.md".into(),
        lines: vec![
            "## MixedCase Heading".into(),
            "- **Bold** `code` [docs](https://example.invalid) dbbe825be crates/hc-tui/src/view.rs:82".into(),
            "".into(),
            "".into(),
            "".into(),
            "```rs".into(),
            "// 中文注释".into(),
            "```".into(),
        ],
        scroll: 0,
        ..CardView::default()
    };
    let area = Rect::new(0, 0, 100, 30);
    let mut buf = Buffer::empty(area);
    let rendered = crate::hollywood::render_view(&mut buf, area, &md, None, &palette);
    let text: Vec<String> = (rendered.body.y..rendered.body.bottom())
        .map(|y| wide_row_of(&buf, rendered.body.x, rendered.body.right(), y))
        .collect();
    let joined = text.join("\n");
    assert!(
        joined.contains("MixedCase Heading"),
        "case preserved: {joined}"
    );
    assert!(joined.contains("▪ Bold code docs dbbe825be"), "{joined}");
    assert!(
        !joined.contains("https://example.invalid"),
        "link URL hidden: {joined}"
    );
    assert!(
        joined.contains("▏ // 中文注释"),
        "code block gutter + text: {joined}"
    );

    // Verify heading accent, code surface0, SHA/path teal, and code surface1 display-width pad.
    let rb = border_of(&buf, rendered.area);
    let text_x = rb.x + 3;
    let heading_y = (rendered.body.y..rendered.body.bottom())
        .find(|&y| buf[(text_x, y)].symbol() == "M")
        .expect("MixedCase Heading row");
    assert_eq!(buf[(text_x, heading_y)].fg, palette.accent);
    // Find `▏` row and ensure all content cells up to right pad have `surface1`.
    let code_y = (rendered.body.y..rendered.body.bottom())
        .find(|&y| buf[(text_x, y)].symbol() == "▏")
        .expect("fenced code row");
    assert_eq!(buf[(text_x, code_y)].bg, palette.surface1);
    assert_eq!(buf[(rb.right() - 4, code_y)].bg, palette.surface1);

    // PlainLog skips markdown parsing, highlights error/pass, and uses `↪ ` continuation.
    let log = CardView {
        card: None,
        title: "checks/tests.log".into(),
        lines: vec![
            "## not a heading".into(),
            "error[E0001]: very long failure message that wraps across narrow card width cleanly without ellipsis".into(),
            "test result: ok. 18 passed".into(),
        ],
        scroll: 0,
        ..CardView::default()
    };
    let narrow = Rect::new(0, 0, 60, 24);
    let mut log_buf = Buffer::empty(narrow);
    let log_ren = crate::hollywood::render_view(&mut log_buf, narrow, &log, None, &palette);
    let lb = border_of(&log_buf, log_ren.area);
    let log_x = lb.x + 3;
    let log_rows: Vec<String> = (log_ren.body.y..log_ren.body.bottom())
        .map(|y| wide_row_of(&log_buf, log_ren.body.x, log_ren.body.right(), y))
        .collect();
    let log_joined = log_rows.join("\n");
    assert!(
        log_joined.contains("## not a heading"),
        "no markdown parse in PlainLog: {log_joined}"
    );
    assert!(
        log_joined.contains("↪ "),
        "PlainLog continuation glyph: {log_joined}"
    );
    let err_y = (log_ren.body.y..log_ren.body.bottom())
        .find(|&y| log_buf[(log_x, y)].symbol() == "e")
        .expect("error line");
    assert_eq!(log_buf[(log_x, err_y)].fg, palette.red);
    let ok_y = (log_ren.body.y..log_ren.body.bottom())
        .find(|&y| {
            wide_row_of(&log_buf, log_ren.body.x, log_ren.body.right(), y)
                .trim_start()
                .starts_with("test result: ok")
        })
        .expect("ok line");
    assert_eq!(log_buf[(log_x, ok_y)].fg, palette.green);
}

#[test]
fn card_view_renders_all_five_entry_kinds_and_omits_missing_optional_fields() {
    use crate::hollywood::{ReaderKind, ReaderMeta, ReaderOrigin, age_label};
    use crate::studio::StudioKey;

    let palette = &palettes()[0];
    let now_ms = 10 * 86_400_000;
    assert_eq!(
        age_label(Some(now_ms - 5_000), now_ms).as_deref(),
        Some("now")
    );
    assert_eq!(
        age_label(Some(now_ms - 4 * 60_000), now_ms).as_deref(),
        Some("4m")
    );
    assert_eq!(
        age_label(Some(now_ms - 3 * 3_600_000), now_ms).as_deref(),
        Some("3h")
    );
    assert_eq!(
        age_label(Some(now_ms - 2 * 86_400_000), now_ms).as_deref(),
        Some("2d")
    );
    assert_eq!(age_label(None, now_ms), None);

    let area = Rect::new(0, 0, 100, 16);

    // 1a. Studio Outcome with full metadata & working square mark (yellow).
    let outcome = CardView {
        card: None,
        title: "acme-shop #12".into(),
        lines: vec!["Build summary".into()],
        scroll: 0,
        meta: ReaderMeta {
            kind: ReaderKind::Outcome,
            studio: Some("acme-shop".into()),
            seq: Some(12),
            from: Some("producer".into()),
            to: Some("user".into()),
            task: Some("t1".into()),
            file: Some("reports/12.md".into()),
            ts_ms: Some(now_ms - 4 * 60_000),
            role: Some("producer".into()),
            role_state: Some("working".into()),
            runner: Some("agy".into()),
            model: Some("pro".into()),
            nav_index: Some(3),
            nav_total: Some(6),
            origin: ReaderOrigin::StudioExchange(StudioKey::local("acme-shop"), 12),
            ..ReaderMeta::default()
        },
        ..CardView::default()
    };
    let mut buf = Buffer::empty(area);
    let ren = crate::hollywood::render_view_at(&mut buf, area, &outcome, None, palette, now_ms);
    let b = border_of(&buf, ren.area);
    assert!(ren.prev_hit.is_some(), "◀ hit rect recorded");
    assert!(ren.next_hit.is_some(), "▶ hit rect recorded");
    let top_row = wide_row_of(&buf, b.x, b.right(), b.y);
    let ctx_row = wide_row_of(&buf, b.x, b.right(), b.y + 1);
    assert!(top_row.contains("■ OUTCOME #12 · acme-shop"), "{top_row}");
    assert!(top_row.contains("◀ 3/6 ▶ · 4m"), "{top_row}");
    assert!(
        ctx_row.contains("producer → you · agy · pro · ■ working"),
        "{ctx_row}"
    );
    assert!(ctx_row.contains("t1 · reports/12.md"), "{ctx_row}");
    assert_eq!(
        buf[(b.x, b.y + 2)].symbol(),
        "├",
        "Row 2 header divider present"
    );
    let sq_x = (b.x..b.right())
        .find(|&x| buf[(x, b.y + 1)].symbol() == "■")
        .expect("working square mark");
    assert_eq!(buf[(sq_x, b.y + 1)].fg, palette.yellow);

    // 1b. Legacy studio entry with absent optional fields: no `· ·` or stray `→`.
    let legacy = CardView {
        card: None,
        title: "acme-shop #12".into(),
        lines: vec!["Legacy body".into()],
        scroll: 0,
        meta: ReaderMeta {
            kind: ReaderKind::Outcome,
            studio: Some("acme-shop".into()),
            seq: Some(12),
            from: Some("producer".into()),
            file: Some("reports/12.md".into()),
            origin: ReaderOrigin::StudioExchange(StudioKey::local("acme-shop"), 12),
            ..ReaderMeta::default()
        },
        ..CardView::default()
    };
    let mut lbuf = Buffer::empty(area);
    let lren = crate::hollywood::render_view_at(&mut lbuf, area, &legacy, None, palette, now_ms);
    let lb = border_of(&lbuf, lren.area);
    let ltop = wide_row_of(&lbuf, lb.x, lb.right(), lb.y);
    let lctx = wide_row_of(&lbuf, lb.x, lb.right(), lb.y + 1);
    assert!(ltop.contains("■ OUTCOME #12 · acme-shop"), "{ltop}");
    assert!(!ltop.contains("◀"), "{ltop}");
    assert!(lctx.contains("producer"), "{lctx}");
    assert!(lctx.contains("reports/12.md"), "{lctx}");
    assert!(!lctx.contains("→"), "no stray arrow: {lctx}");
    assert!(!lctx.contains("· ·"), "no stray separator: {lctx}");
    assert!(!lctx.contains("●") && !lctx.contains("○"), "{lctx}");

    // 2. Hollywood report / tell + closed role square mark.
    let hw = CardView {
        card: None,
        title: "hollywood #8".into(),
        lines: vec!["Digest".into()],
        scroll: 0,
        meta: ReaderMeta {
            kind: ReaderKind::Report,
            studio: Some("hollywood".into()),
            seq: Some(8),
            from: Some("hollywood".into()),
            to: Some("user".into()),
            ts_ms: Some(now_ms - 120_000),
            role_state: Some("closed".into()),
            runner: Some("claude".into()),
            model: Some("sonnet".into()),
            resumable: true,
            nav_index: Some(2),
            nav_total: Some(4),
            origin: ReaderOrigin::HollywoodExchange(8),
            ..ReaderMeta::default()
        },
        ..CardView::default()
    };
    let mut hbuf = Buffer::empty(area);
    let hren = crate::hollywood::render_view_at(&mut hbuf, area, &hw, None, palette, now_ms);
    let hb = border_of(&hbuf, hren.area);
    let htop = wide_row_of(&hbuf, hb.x, hb.right(), hb.y);
    let hctx = wide_row_of(&hbuf, hb.x, hb.right(), hb.y + 1);
    assert!(htop.contains("■ REPORT #8 · hollywood"), "{htop}");
    assert!(htop.contains("◀ 2/4 ▶ · 2m"), "{htop}");
    assert!(hctx.contains("□ closed"), "{hctx}");
    let closed_x = (hb.x..hb.right())
        .find(|&x| hbuf[(x, hb.y + 1)].symbol() == "□")
        .expect("closed hollow square mark");
    assert_eq!(hbuf[(closed_x, hb.y + 1)].fg, palette.overlay0);

    // 3. Actionable card (`■ CARD c3 · acme-shop · open`).
    let card = CardView {
        card: Some("c3".into()),
        title: "c3".into(),
        lines: vec!["Action: pause t2".into()],
        scroll: 0,
        meta: ReaderMeta {
            kind: ReaderKind::Card,
            studio: Some("acme-shop".into()),
            machine: Some("host-a".into()),
            from: Some("hollywood".into()),
            ts_ms: Some(now_ms - 60_000),
            card_action: Some("pause t2".into()),
            card_status: Some("open".into()),
            nav_index: Some(1),
            nav_total: Some(2),
            origin: ReaderOrigin::Card("c3".into()),
            ..ReaderMeta::default()
        },
        ..CardView::default()
    };
    let mut cbuf = Buffer::empty(area);
    let cren = crate::hollywood::render_view_at(&mut cbuf, area, &card, None, palette, now_ms);
    let cb = border_of(&cbuf, cren.area);
    let ctop = wide_row_of(&cbuf, cb.x, cb.right(), cb.y);
    let cctx = wide_row_of(&cbuf, cb.x, cb.right(), cb.y + 1);
    assert!(ctop.contains("■ CARD c3 · acme-shop · open"), "{ctop}");
    assert!(ctop.contains("◀ 1/2 ▶ · 1m"), "{ctop}");
    assert!(cctx.contains("hollywood proposes: pause t2"), "{cctx}");
    assert!(cctx.contains("host-a"), "{cctx}");

    // 4. Task Sheet Check artifact (`■ CHECK · acme-shop`) + loading tag.
    let check = CardView {
        card: None,
        title: "checks/d1-units.txt".into(),
        lines: vec!["cargo test failed".into()],
        scroll: 0,
        meta: ReaderMeta {
            kind: ReaderKind::Check,
            studio: Some("acme-shop".into()),
            task: Some("t1".into()),
            file: Some("checks/d1-units.txt".into()),
            check_status: Some("FAIL (exit 1)".into()),
            nav_index: Some(2),
            nav_total: Some(3),
            loading_label: Some("#13".into()),
            origin: ReaderOrigin::SheetDeliveries(StudioKey::local("acme-shop"), Some("t1".into())),
            ..ReaderMeta::default()
        },
        ..CardView::default()
    };
    let mut kbuf = Buffer::empty(area);
    let kren = crate::hollywood::render_view_at(&mut kbuf, area, &check, None, palette, now_ms);
    let kb = border_of(&kbuf, kren.area);
    let ktop = wide_row_of(&kbuf, kb.x, kb.right(), kb.y);
    let kctx = wide_row_of(&kbuf, kb.x, kb.right(), kb.y + 1);
    assert!(ktop.contains("■ loading #13…"), "{ktop}");
    assert!(
        kctx.contains("task t1 · FAIL (exit 1)") && kctx.contains("checks/d1-units.txt"),
        "{kctx}"
    );

    // 5. Actor Records (`■ RECORDS · acme-shop`).
    let records = CardView {
        card: None,
        title: "records acme-shop".into(),
        lines: vec!["task-1 accepted".into()],
        scroll: 0,
        meta: ReaderMeta {
            kind: ReaderKind::Records,
            studio: Some("acme-shop".into()),
            machine: Some("host-a".into()),
            studio_counts: Some((2, 3, 5)),
            origin: ReaderOrigin::Records(StudioKey::local("acme-shop")),
            ..ReaderMeta::default()
        },
        ..CardView::default()
    };
    let mut rbuf = Buffer::empty(area);
    let rren = crate::hollywood::render_view_at(&mut rbuf, area, &records, None, palette, now_ms);
    let rb = border_of(&rbuf, rren.area);
    let rtop = wide_row_of(&rbuf, rb.x, rb.right(), rb.y);
    let rctx = wide_row_of(&rbuf, rb.x, rb.right(), rb.y + 1);
    assert!(rtop.contains("■ RECORDS · acme-shop"), "{rtop}");
    assert!(rtop.contains("host-a"), "{rtop}");
    assert!(rctx.contains("2/3 tasks · 5 roles"), "{rctx}");
}

#[test]
fn card_view_action_bar_degrades_across_width_tiers_and_records_chip_hits() {
    use crate::hollywood::{ReaderKind, ReaderMeta, ReaderOrigin};
    use crate::studio::StudioKey;

    let palette = &palettes()[0];
    let make_outcome = |lines: Vec<String>| CardView {
        card: None,
        title: "acme-shop #188".into(),
        lines,
        scroll: 0,
        meta: ReaderMeta {
            kind: ReaderKind::Outcome,
            studio: Some("acme-shop".into()),
            seq: Some(188),
            from: Some("producer".into()),
            to: Some("user".into()),
            role: Some("producer".into()),
            role_state: Some("ready".into()),
            role_terminal: Some("t1".into()),
            runner: Some("agy".into()),
            model: Some("pro".into()),
            nav_index: Some(2),
            nav_total: Some(4),
            origin: ReaderOrigin::StudioExchange(StudioKey::local("acme-shop"), 188),
            ..ReaderMeta::default()
        },
        ..CardView::default()
    };

    // Tier 0 full width (>=70 inside 100-col surface).
    let wide_outcome = make_outcome(vec![
        "detailed delivery summary reaching across at least seventy columns wide".into(),
    ]);
    let wide = Rect::new(0, 0, 100, 16);
    let mut wbuf = Buffer::empty(wide);
    let wren = crate::hollywood::render_view(&mut wbuf, wide, &wide_outcome, None, palette);
    let wb = border_of(&wbuf, wren.area);
    let wstrip = wide_row_of(&wbuf, wb.x, wb.right(), wb.bottom() - 2);
    assert!(
        wstrip.contains("[t] tell")
            && wstrip.contains("[o] focus")
            && wstrip.contains("[p] peek")
            && wstrip.contains("[v] sheet")
            && wstrip.contains("[y] copy"),
        "full tier strip: {wstrip}"
    );
    for expected in ['t', 'o', 'p', 'v', 'y'] {
        let (rect, _) = wren
            .chips
            .iter()
            .find(|(_, ch)| *ch == expected)
            .unwrap_or_else(|| panic!("missing chip [{expected}] in {wstrip}"));
        let text = wide_row_of(&wbuf, rect.x, rect.right(), rect.y);
        assert!(
            text.starts_with(&format!("[{expected}]")),
            "chip [{expected}] rect mismatch: got {text:?} from {wstrip:?}"
        );
    }

    // Tier 1 medium 52-col border (`avail = 48`), +2 gutter = 54 outer: `[t] tell [o] focus [y] copy  ←/→ · esc`.
    let medium_view = make_outcome(vec!["ok".into()]);
    let medium = Rect::new(0, 0, 56, 12);
    let mut mbuf = Buffer::empty(medium);
    let mren = crate::hollywood::render_view(&mut mbuf, medium, &medium_view, None, palette);
    let mb = border_of(&mbuf, mren.area);
    assert_eq!(mb.width, 52);
    assert_eq!(mren.area.width, 54);
    let mstrip = wide_row_of(&mbuf, mb.x, mb.right(), mb.bottom() - 2);
    assert!(
        mstrip.contains("[t] tell")
            && mstrip.contains("[o] focus")
            && mstrip.contains("[y] copy")
            && !mstrip.contains("[p]"),
        "medium tier strip: {mstrip}"
    );

    // Tier 2 squeezed fallback: `[t] [o] [y]  ? keys`, never mid-word cut.
    let narrow = Rect::new(0, 0, 40, 12);
    let mut nbuf = Buffer::empty(narrow);
    let nren = crate::hollywood::render_view(&mut nbuf, narrow, &medium_view, None, palette);
    let nb = border_of(&nbuf, nren.area);
    assert_eq!(nb.width, 40);
    let nstrip = wide_row_of(&nbuf, nb.x, nb.right(), nb.bottom() - 2);
    assert!(
        nstrip.contains("[t] [o] [y]") && nstrip.contains("? keys"),
        "narrow fallback strip: {nstrip}"
    );

    // Narrow actionable card: `[a] [x] [y]  ? keys`, and pending card locks `a`/`x`.
    let card_view = CardView {
        card: Some("c3".into()),
        title: "c3".into(),
        lines: vec!["Action: pause t2".into()],
        scroll: 0,
        meta: ReaderMeta {
            kind: ReaderKind::Card,
            studio: Some("acme-shop".into()),
            card_action: Some("pause t2".into()),
            card_status: Some("open".into()),
            nav_index: Some(1),
            nav_total: Some(2),
            origin: ReaderOrigin::Card("c3".into()),
            ..ReaderMeta::default()
        },
        ..CardView::default()
    };
    let squeezed_card = Rect::new(0, 0, 38, 12);
    let mut cbuf = Buffer::empty(squeezed_card);
    let cren = crate::hollywood::render_view(&mut cbuf, squeezed_card, &card_view, None, palette);
    let cb = border_of(&cbuf, cren.area);
    let cstrip = wide_row_of(&cbuf, cb.x, cb.right(), cb.bottom() - 2);
    assert!(
        cstrip.contains("[a] [x] [y]") && cstrip.contains("? keys"),
        "narrow card strip: {cstrip}"
    );

    let mut pbuf = Buffer::empty(wide);
    let pren = crate::hollywood::render_view(
        &mut pbuf,
        wide,
        &card_view,
        Some("applying card c3"),
        palette,
    );
    let pb = border_of(&pbuf, pren.area);
    let pstrip = wide_row_of(&pbuf, pb.x, pb.right(), pb.bottom() - 2);
    assert!(
        pstrip.contains("■ applying card c3… (a/x locked)"),
        "{pstrip}"
    );
    assert!(!pren.chips.iter().any(|(_, c)| *c == 'a' || *c == 'x'));
    assert!(pren.chips.iter().any(|(_, c)| *c == 'y'));

    // Closed non-resumable actor omits `[o]` and `[p]`.
    let closed_view = CardView {
        card: None,
        title: "acme-shop #22".into(),
        lines: vec!["done".into()],
        scroll: 0,
        meta: ReaderMeta {
            kind: ReaderKind::Outcome,
            studio: Some("acme-shop".into()),
            seq: Some(22),
            from: Some("actor-eve".into()),
            to: Some("user".into()),
            role: Some("actor-eve".into()),
            role_state: Some("closed".into()),
            resumable: false,
            origin: ReaderOrigin::StudioExchange(StudioKey::local("acme-shop"), 22),
            ..ReaderMeta::default()
        },
        ..CardView::default()
    };
    let mut clbuf = Buffer::empty(wide);
    let clren = crate::hollywood::render_view(&mut clbuf, wide, &closed_view, None, palette);
    let clb = border_of(&clbuf, clren.area);
    let clstrip = wide_row_of(&clbuf, clb.x, clb.right(), clb.bottom() - 2);
    assert!(!clstrip.contains("[p]"), "{clstrip}");
    assert!(!clstrip.contains("[o]"), "{clstrip}");
    assert!(
        clstrip.contains("[t] tell") && clstrip.contains("[y] copy"),
        "{clstrip}"
    );

    // Records at 40 cols keeps `[y] copy  esc`, and below Tier 1 degrades to `[y]  ? keys`.
    let records_view = CardView {
        card: None,
        title: "records acme-shop".into(),
        lines: vec!["task-1".into()],
        scroll: 0,
        meta: ReaderMeta {
            kind: ReaderKind::Records,
            studio: Some("acme-shop".into()),
            studio_counts: Some((1, 2, 3)),
            studio_finished: true,
            origin: ReaderOrigin::Records(StudioKey::local("acme-shop")),
            ..ReaderMeta::default()
        },
        ..CardView::default()
    };
    let mut rbuf = Buffer::empty(narrow);
    let rren = crate::hollywood::render_view(&mut rbuf, narrow, &records_view, None, palette);
    let rb = border_of(&rbuf, rren.area);
    let rstrip = wide_row_of(&rbuf, rb.x, rb.right(), rb.bottom() - 2);
    assert!(rstrip.contains("[y] copy"), "records tier1: {rstrip}");

    let tiny = Rect::new(0, 0, 16, 10);
    let mut tbuf = Buffer::empty(tiny);
    let tren = crate::hollywood::render_view(&mut tbuf, tiny, &records_view, None, palette);
    let tb = border_of(&tbuf, tren.area);
    let tstrip = wide_row_of(&tbuf, tb.x, tb.right(), tb.bottom() - 2);
    assert!(tstrip.contains("[y]  ? keys"), "records tier2: {tstrip}");
}

#[test]
fn card_view_inline_compose_states_render_band_and_never_overflow_80x24() {
    use crate::hollywood::{InlineTellState, ReaderKind, ReaderMeta, ReaderOrigin};
    use crate::studio::StudioKey;

    let palette = &palettes()[0];
    let mut view = CardView {
        card: None,
        title: "acme-shop #188".into(),
        lines: (1..=25).map(|i| format!("▪ report line {i}")).collect(),
        scroll: 0,
        meta: ReaderMeta {
            kind: ReaderKind::Report,
            studio: Some("acme-shop".into()),
            seq: Some(188),
            from: Some("director".into()),
            to: Some("user".into()),
            role: Some("director".into()),
            role_state: Some("working".into()),
            role_terminal: Some("t2".into()),
            origin: ReaderOrigin::StudioExchange(StudioKey::local("acme-shop"), 188),
            ..ReaderMeta::default()
        },
        tell: InlineTellState::Composing,
        ..CardView::default()
    };

    let narrow = Rect::new(0, 1, 80, 22);
    let mut nbuf = Buffer::empty(narrow);
    let editor = crate::chrome::text_editor::TextEditor::new("hi", false);
    let opts = crate::hollywood::ReaderComposeOpts {
        editor: Some(&editor),
        target: "director",
        studio: true,
    };
    let nren = crate::hollywood::render_view_with_compose(
        &mut nbuf,
        narrow,
        &view,
        None,
        Some(opts),
        palette,
        0,
    );
    assert!(nren.area.bottom() <= narrow.bottom());
    assert!(nren.compose_rect.is_some());
    assert!(nren.cursor.is_some());

    view.tell = InlineTellState::Sending {
        action_id: 7,
        target: "director".into(),
        text: "hi".into(),
    };
    let sren = crate::hollywood::render_view_with_compose(
        &mut nbuf,
        narrow,
        &view,
        None,
        Some(opts),
        palette,
        0,
    );
    let sb = border_of(&nbuf, sren.area);
    let sline = wide_row_of(&nbuf, sb.x, sb.right(), sb.bottom() - 4);
    assert!(sline.contains("■ sending to director…"), "{sline}");
}

/// The ASKS row shows its exact shape, age, and chips without folding
/// sibling cards into `· +N` — the studio's card renders on its own row.
#[test]
fn the_selected_asks_row_reads_exactly_and_shows_its_chips() {
    let mut value = summary_json();
    value["studios"][0]["roles"][0]["asks"] =
        serde_json::json!("What should we build? Start small.");
    value["studios"][0]["exchange"] = serde_json::json!([
        { "seq": 10, "who": "producer", "to": "user", "kind": "tell", "ask": true,
          "ts_ms": 1, "text": "What should we build? Start small." }
    ]);
    let summary: Summary = serde_json::from_value(value).unwrap();
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 120, 45);
    let target = Target::Role(
        crate::studio::StudioKey::local("acme-shop"),
        "producer".into(),
    );
    let (buf, _) = draw(
        area,
        Some(&summary),
        true,
        &selecting(LayerMode::Expanded, target),
        false,
        &palette,
    );
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("? ASKS acme-shop · Ⓟ · "), "{text}");
    assert!(text.contains("— What should we build?"), "{text}");
    assert!(!text.contains("· +1"), "{text}");
    assert!(text.contains("CARD c3  acme-shop · pause t1"), "{text}");
    assert!(text.contains(" reply r "), "{text}");
    assert!(text.contains(" ack x "), "{text}");
}

#[test]
fn a_studio_needing_upgrade_draws_single_line_in_call_sheet() {
    let mut value = summary_json();
    value["studios"][0]["needs_upgrade"] = serde_json::json!(true);
    value["studios"][0]["unknown_kinds"] = serde_json::json!(["design.opened"]);
    value["hollywood"]["cards"] = serde_json::json!([]);
    let summary: Summary = serde_json::from_value(value).unwrap();
    let palette = palettes()[0].clone();
    let elev = palette.elevation();
    let area = Rect::new(0, 0, 120, 45);
    let (buf, hits) = draw(
        area,
        Some(&summary),
        true,
        &layer(LayerMode::Expanded),
        false,
        &palette,
    );
    let text = lines(&buf, area).join("\n");
    assert!(text.contains("3 studios · !1"), "{text}");
    assert!(text.contains("! UPGRADE"), "{text}");
    assert!(text.contains("upgrade backlot (design.opened)"), "{text}");

    let row_y = hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Upgrade(_)))
        .expect("upgrade hit row")
        .0
        .y;
    assert_eq!(buf[(2, row_y)].symbol(), "▎");
    assert_eq!(buf[(2, row_y)].fg, palette.peach);
    assert_eq!(buf[(2, row_y)].bg, elev.surface);

    let target = Target::Upgrade(crate::studio::StudioKey::local("acme-shop"));
    let (sbuf, _) = draw(
        area,
        Some(&summary),
        true,
        &selecting(LayerMode::Expanded, target),
        false,
        &palette,
    );
    assert_eq!(sbuf[(0, row_y)].symbol(), "▌");
    assert_eq!(sbuf[(1, row_y)].symbol(), "▸");
    assert_eq!(sbuf[(2, row_y)].symbol(), "▎");
    assert_eq!(sbuf[(2, row_y)].fg, palette.peach);
    assert_eq!(sbuf[(2, row_y)].bg, elev.raised_row);
}

#[test]
fn six_studios_ready_recast_200x17_and_100x17_show_every_card_and_match_header_count() {
    let summary = six_studios_ready_recast_summary();
    let palette = palettes()[0].clone();

    for width in [200u16, 100u16] {
        let area = Rect::new(0, 0, width, 17);
        let (buf, hits) = draw(
            area,
            Some(&summary),
            true,
            &layer(LayerMode::Expanded),
            false,
            &palette,
        );
        let text = lines(&buf, Rect::new(0, 0, width, hits.area.height)).join("\n");
        assert!(
            text.contains("6 studios · 3c · ✓2 ready"),
            "{width}x17 missing matching header count:\n{text}"
        );
        for needle in [
            "CARD c1372  acme-shop · recast Ⓟ → agy/gemini-3-pro/high",
            "CARD c1373  acme-shop · recast Ⓓ → agy/gemini-3-pro/high",
            "CARD c1374  acme-shop · recast Ⓦ → agy/gemini-3-pro/high",
            "READY acme-shop · 12/13 tasks accepted",
            "READY beta-api · 13/13 tasks accepted",
        ] {
            assert!(
                text.contains(needle),
                "{width}x17 missing `{needle}`:\n{text}"
            );
        }
        assert!(
            !text.contains("· +3"),
            "{width}x17 must not fold cards:\n{text}"
        );
        assert!(
            !text.contains("· +2"),
            "{width}x17 must not fold cards:\n{text}"
        );
        assert!(
            !text.contains("· +1"),
            "{width}x17 must not fold cards:\n{text}"
        );

        // Role badges carry their semantic role colours.
        for (glyph, color) in [
            ("Ⓟ", palette.mauve),
            ("Ⓓ", palette.blue),
            ("Ⓦ", palette.teal),
        ] {
            let found = (0..hits.area.height).any(|y| {
                (0..width).any(|x| buf[(x, y)].symbol() == glyph && buf[(x, y)].fg == color)
            });
            assert!(found, "{width}x17 missing coloured badge {glyph}:\n{text}");
        }
    }

    // Selected recast card keeps its chips (`apply a`, `cancel x`, `show ↵`) and `why:` line
    // while keeping all 3 card IDs visible on screen at 100x17 and 200x17.
    for width in [200u16, 100u16] {
        let area = Rect::new(0, 0, width, 17);
        let (buf, hits) = draw(
            area,
            Some(&summary),
            true,
            &selecting(LayerMode::Expanded, Target::Card("c1372".into())),
            false,
            &palette,
        );
        let text = lines(&buf, Rect::new(0, 0, width, hits.area.height)).join("\n");
        assert!(
            text.contains("c1372") && text.contains("c1373") && text.contains("c1374"),
            "{text}"
        );
        assert!(
            text.contains("apply a") && text.contains("cancel x") && text.contains("show ↵"),
            "{text}"
        );
        assert!(text.contains("why: producer hit quota limit"), "{text}");
    }
}

#[test]
fn card_row_shows_role_badge_and_from_to_when_present_in_summary() {
    let mut value = summary_json();
    value["hollywood"]["cards"] = serde_json::json!([
        {
            "id": "c40",
            "studio": "acme-shop",
            "action": "recast actor-t1-c1 claude/sonnet-4 → agy/gemini-3-pro/high",
            "why": "timeout"
        },
        {
            "id": "c41",
            "studio": "hollywood",
            "action": "Hollywood: cmd / deepseek-v4.1-flash → claude / claude-opus-5-5",
            "why": "user requested switch"
        }
    ]);
    let summary: Summary = serde_json::from_value(value).unwrap();
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 140, 30);
    let (buf, hits) = draw(
        area,
        Some(&summary),
        true,
        &layer(LayerMode::Expanded),
        false,
        &palette,
    );
    let text = lines(&buf, Rect::new(0, 0, 140, hits.area.height)).join("\n");
    assert!(
        text.contains("CARD c40  acme-shop · recast Ⓐ t1 claude/sonnet-4 → agy/gemini-3-pro/high"),
        "{text}"
    );
    assert!(
        text.contains(
            "CARD c41  hollywood · Hollywood: cmd / deepseek-v4.1-flash → claude / claude-opus-5-5"
        ),
        "{text}"
    );
    let actor_coloured = (0..hits.area.height)
        .any(|y| (0..140).any(|x| buf[(x, y)].symbol() == "Ⓐ" && buf[(x, y)].fg == palette.peach));
    assert!(actor_coloured, "actor badge must use peach:\n{text}");
}

#[test]
fn decision_and_prompt_on_ready_studio_render_their_own_rows() {
    let mut value = summary_json();
    value["hollywood"]["cards"] = serde_json::json!([]);
    value["studios"][0]["ready_to_finish"] = serde_json::json!(true);
    value["studios"][0]["tasks_done"] = serde_json::json!(4);
    value["studios"][0]["tasks_total"] = serde_json::json!(4);
    value["studios"][0]["roles"][1]["attention_user"] = serde_json::json!(true);
    value["studios"][0]["roles"][1]["attention_seq"] = serde_json::json!(7);
    value["studios"][0]["roles"][1]["attention"] = serde_json::json!("permission prompt");
    value["studios"][0]["roles"][1]["attention_detail"] = serde_json::json!("cargo test");
    value["studios"][0]["design"] = serde_json::json!({
        "version": 1,
        "round": 2,
        "phase": "waiting_user",
        "waiting_user": true,
        "suggest_skip": false,
        "doc_file": "design/v1.md",
        "summary": "Keep SQLite WAL",
        "questions": [
            { "id": "Q1", "prompt": "Keep WAL?", "recommendation": "yes", "section_ref": "§R2.W" }
        ]
    });
    let summary: Summary = serde_json::from_value(value).unwrap();
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 140, 44);
    let (buf, hits) = draw(
        area,
        Some(&summary),
        true,
        &layer(LayerMode::Expanded),
        false,
        &palette,
    );
    let text = lines(&buf, Rect::new(0, 0, 140, hits.area.height)).join("\n");
    assert!(text.contains("3 studios · !1 · ?1 · ✓1 ready"), "{text}");
    let prompt_y = hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Prompt(..)))
        .expect("prompt hit")
        .0
        .y;
    let decision_y = hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Decision(..)))
        .expect("decision hit")
        .0
        .y;
    let ready_y = hits
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Ready(..)))
        .expect("ready hit")
        .0
        .y;
    assert!(
        prompt_y < decision_y && decision_y < ready_y,
        "order: {prompt_y}, {decision_y}, {ready_y}\n{text}"
    );
    assert!(!text.contains("· +"), "{text}");
}

#[test]
fn waiting_scroll_selecting_last_detailable_item_never_overflows_band() {
    let mut value = serde_json::to_value(six_studios_ready_recast_summary()).unwrap();
    value["hollywood"]["cards"] = serde_json::json!([
        { "id": "c1", "studio": "acme-shop", "action": "recast producer → agy/m", "why": "one" },
        { "id": "c2", "studio": "acme-shop", "action": "recast director → agy/m", "why": "two" },
        { "id": "c3", "studio": "acme-shop", "action": "recast writer → agy/m", "why": "three" },
        { "id": "c4", "studio": "gamma-web", "action": "pause t1", "why": "four" },
        { "id": "c5", "studio": "delta-ops", "action": "pause t2", "why": "five" },
        { "id": "c6", "studio": "eps-ui", "action": "pause t3", "why": "six" },
        { "id": "c7", "studio": "zeta-jobs", "action": "pause t4", "why": "seven" }
    ]);
    let summary: Summary = serde_json::from_value(value).unwrap();
    let palette = palettes()[0].clone();
    // Wait window overflow at 100x17 cap (height 34 => cap 17) keeps `why:` detail, compose, and `▀▀`.
    let area = Rect::new(0, 0, 100, 34);
    let (buf, hits) = draw(
        area,
        Some(&summary),
        true,
        &selecting(LayerMode::Expanded, Target::Card("c7".into())),
        false,
        &palette,
    );
    assert!(hits.area.height <= 17, "height={} > 17", hits.area.height);
    let bottom = row_of(&buf, area, hits.area.bottom() - 1);
    assert!(
        bottom.contains("▀▀"),
        "bottom sprocket kept inside area: {bottom}"
    );
    let text = lines(&buf, Rect::new(0, 0, 100, hits.area.height)).join("\n");
    assert!(text.contains("CARD c7"), "{text}");
    assert!(text.contains("why: seven"), "{text}");
    assert!(text.contains("tell hollywood"), "{text}");

    // And under extreme pressure (frame height 17 => cap 12 with 9 waits), never overflows cap
    // and always keeps the selected waiting item (`c7`) and compose visible.
    let tight = Rect::new(0, 0, 100, 17);
    let (tbuf, thits) = draw(
        tight,
        Some(&summary),
        true,
        &selecting(LayerMode::Expanded, Target::Card("c7".into())),
        false,
        &palette,
    );
    assert!(thits.area.height <= 12, "height={} > 12", thits.area.height);
    let ttext = lines(&tbuf, Rect::new(0, 0, 100, thits.area.height)).join("\n");
    assert!(ttext.contains("CARD c7"), "{ttext}");
    assert!(ttext.contains("tell hollywood"), "{ttext}");
}

#[test]
fn repo_note_card_renders_like_other_cards_on_call_sheet() {
    let mut value = summary_json();
    value["hollywood"]["cards"] = serde_json::json!([
        {
            "id": "c5",
            "studio": "acme-shop",
            "action": "repo-note build-cmd",
            "why": "share fast inner-loop test command across studios"
        }
    ]);
    let summary: Summary = serde_json::from_value(value).unwrap();
    let palette = palettes()[0].clone();
    let area = Rect::new(0, 0, 120, 24);

    // Unfocused: one-row card entry.
    let (ubuf, _) = draw(
        area,
        Some(&summary),
        true,
        &layer(LayerMode::Expanded),
        false,
        &palette,
    );
    let utext = lines(&ubuf, area).join("\n");
    assert!(
        utext.contains("CARD c5  acme-shop · repo-note build-cmd"),
        "{utext}"
    );

    // Focused on card c5: shows why line and a / x / ↵ chips.
    let (sbuf, hits) = draw(
        area,
        Some(&summary),
        true,
        &selecting(LayerMode::Expanded, Target::Card("c5".into())),
        false,
        &palette,
    );
    let rendered = lines(&sbuf, Rect::new(0, 0, 120, hits.area.height)).join("\n");
    assert!(
        rendered.contains("CARD c5  acme-shop · repo-note build-cmd"),
        "{rendered}"
    );
    assert!(
        rendered.contains("why: share fast inner-loop test command"),
        "{rendered}"
    );
    assert!(
        rendered.contains("apply a")
            && rendered.contains("cancel x")
            && rendered.contains("show ↵"),
        "{rendered}"
    );
    println!("CALL_SHEET_REPO_NOTE_BEGIN\n{rendered}\nCALL_SHEET_REPO_NOTE_END");
}
