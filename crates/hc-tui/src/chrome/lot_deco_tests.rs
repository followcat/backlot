//! Babylon S4 Lot Pill clasp, header well, blocking `◉` swap, hit-map, and bypass tests.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::lot::{
    HeaderKind, LotCounts, LotFrame, PillAction, PillTier, StudioHeader, render_studio_header,
};
use super::palette::Palette;
use super::render::ShellHitMap;
use crate::hollywood::HollywoodActivity;
use crate::studio::StudioKey;
use crate::studio::glyphs::Person;

fn row_text(buf: &Buffer, area: Rect) -> String {
    (area.x..area.x + area.width)
        .map(|x| buf[(x, area.y)].symbol())
        .collect()
}

fn col_of(buf: &Buffer, area: Rect, needle: &str) -> u16 {
    let text = row_text(buf, area);
    let byte = text
        .find(needle)
        .unwrap_or_else(|| panic!("missing {needle:?} in {text:?}"));
    super::text::display_width(&text[..byte]) as u16 + area.x
}

#[test]
fn header_and_pill_at_120_cols_calm_blocking_and_herdr_bypass() {
    let pal = Palette::backlot();
    let elev = pal.elevation();
    let header = StudioHeader {
        key: Some(StudioKey::local("beta-api")),
        studio: "beta-api".into(),
        machine: None,
        kind: HeaderKind::Role {
            role_key: "producer".into(),
            name: "producer".into(),
            task: None,
            mark: Some(Person::Idle),
            priority: None,
            runner: Some("agy".into()),
            model: Some("gemini-4".into()),
            effort: None,
        },
    };
    let area = Rect::new(0, 0, 120, 1);

    // 1. Calm 120 cols: top bar on `stage_well`, pill clasped on `raised_row`, exact hit rects.
    let calm_counts = LotCounts {
        studios: 6,
        blocking: 0,
        asks: 1,
        cards: 2,
        ready: 2,
        unread: 0,
        activity: HollywoodActivity::Idle,
        switch_state: None,
    };
    let calm_frame = LotFrame {
        counts: calm_counts,
        agent: true,
        pill: true,
        header: Some(&header),
        unfocused_cue: false,
        cols: 120,
        spin: 0,
        studio_chrome: true,
    };
    let mut cbuf = Buffer::empty(area);
    let mut chits = ShellHitMap::default();
    render_studio_header(
        &mut cbuf,
        area,
        &calm_frame,
        PillTier::Full,
        &pal,
        &mut chits,
    );
    let crow = row_text(&cbuf, area);
    assert!(
        crow.contains("╡ ✦ 6 · ?1 · 2c · ✓2 ready · alt+y ╞"),
        "{crow:?}"
    );
    assert_eq!(cbuf[(15, 0)].bg, elev.stage_well);
    assert_eq!(chits.studio_header, Rect::new(0, 0, 10, 1));
    let clasp_x = col_of(&cbuf, area, "╡");
    assert_eq!(cbuf[(clasp_x, 0)].bg, elev.raised_row);
    assert_eq!(cbuf[(clasp_x, 0)].fg, elev.gilt);
    assert!(
        chits
            .lot_pill_segments
            .iter()
            .any(|(r, a)| *a == PillAction::Asks && row_text(&cbuf, *r) == "?1")
            && chits
                .lot_pill_segments
                .iter()
                .any(|(r, a)| *a == PillAction::Cards && row_text(&cbuf, *r) == "2c")
            && chits
                .lot_pill_segments
                .iter()
                .any(|(r, a)| *a == PillAction::Ready && row_text(&cbuf, *r) == "✓2 ready"),
        "hit rects must align after leading clasp: {:?}",
        chits.lot_pill_segments
    );

    // 2. Blocking 120 cols: solid red pill with `◉` in place of `✦`.
    let blocking_counts = LotCounts {
        blocking: 1,
        ..calm_counts
    };
    let block_frame = LotFrame {
        counts: blocking_counts,
        ..calm_frame
    };
    let mut bbuf = Buffer::empty(area);
    let mut bhits = ShellHitMap::default();
    render_studio_header(
        &mut bbuf,
        area,
        &block_frame,
        PillTier::Full,
        &pal,
        &mut bhits,
    );
    let brow = row_text(&bbuf, area);
    assert!(
        brow.contains("╡ ◉ 6 · !1 · ?1 · 2c · ✓2 ready · alt+y ╞"),
        "{brow:?}"
    );
    for x in bhits.lot_pill.x..bhits.lot_pill.right() {
        assert_eq!(bbuf[(x, 0)].bg, pal.red, "solid red blocking pill x={x}");
    }
    assert!(
        bhits
            .lot_pill_segments
            .iter()
            .any(|(r, a)| *a == PillAction::Blocking && row_text(&bbuf, *r) == "!1")
    );

    // 3. Plain / `source = "herdr"` bypass (`studio_chrome = false`): `elev.surface`, no clasps, keeps `✦`.
    let herdr_frame = LotFrame {
        studio_chrome: false,
        ..block_frame
    };
    let mut hbuf = Buffer::empty(area);
    let mut hhits = ShellHitMap::default();
    render_studio_header(
        &mut hbuf,
        area,
        &herdr_frame,
        PillTier::Full,
        &pal,
        &mut hhits,
    );
    let hrow = row_text(&hbuf, area);
    assert_eq!(hbuf[(15, 0)].bg, elev.surface);
    assert!(
        !hrow.contains('╡') && !hrow.contains('╞') && !hrow.contains('◉'),
        "{hrow:?}"
    );
    assert!(hrow.contains("✦ 6 · !1"), "{hrow:?}");
    if std::env::var_os("BABYLON_LOT_DUMP").is_some() {
        eprintln!(
            "=== 120-COL CALM HEADER+PILL ===\n{crow}\n=== 120-COL BLOCKING HEADER+PILL ===\n{brow}"
        );
    }
}
