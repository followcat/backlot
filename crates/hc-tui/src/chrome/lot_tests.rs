//! Unit tests for the bucketed Lot Pill tiers/colors/segments and the
//! role-badged studio breadcrumb header with runtime metadata degradation.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

use super::*;
use crate::hollywood::HollywoodActivity;

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
    crate::chrome::text::display_width(&text[..byte]) as u16 + area.x
}

#[test]
fn non_blocking_pill_uses_raised_row_with_teal_and_peach_segments() {
    let palette = Palette::backlot();
    let elev = palette.elevation();
    let counts = LotCounts {
        studios: 4,
        blocking: 0,
        asks: 1,
        cards: 2,
        ready: 1,
        unread: 0,
        activity: HollywoodActivity::Idle,
    };
    let mut hits = ShellHitMap::default();
    let w = pill_width(&counts, PillTier::Full, true);
    let area = Rect::new(0, 0, w, 1);
    let mut buf = Buffer::empty(area);
    render_pill_recorded(
        &mut buf,
        area,
        &counts,
        PillTier::Full,
        true,
        0,
        &palette,
        &mut hits,
    );
    let text = row_text(&buf, area);
    assert!(text.contains("✦ 4 · ?1 · 2c · ✓1 ready"), "{text:?}");
    assert!(text.contains("alt+y"), "{text:?}");

    for x in 0..w {
        assert_ne!(buf[(x, 0)].bg, palette.red, "non-blocking pill never red");
        assert_eq!(buf[(x, 0)].bg, elev.raised_row);
    }
    let ask_x = col_of(&buf, area, "?1");
    let card_x = col_of(&buf, area, "2c");
    let ready_x = col_of(&buf, area, "✓1 ready");
    let hint_x = col_of(&buf, area, "alt+y");
    assert_eq!(buf[(ask_x, 0)].fg, palette.peach);
    assert_eq!(buf[(card_x, 0)].fg, palette.peach);
    assert_eq!(buf[(ready_x, 0)].fg, palette.teal);
    assert_eq!(buf[(hint_x, 0)].fg, palette.overlay0);

    assert!(
        hits.lot_pill_segments
            .iter()
            .any(|(r, a)| *a == PillAction::Asks && r.x == ask_x)
    );
    assert!(
        hits.lot_pill_segments
            .iter()
            .any(|(r, a)| *a == PillAction::Cards && r.x == card_x)
    );
    assert!(
        hits.lot_pill_segments
            .iter()
            .any(|(r, a)| *a == PillAction::Ready && r.x == ready_x)
    );
}

#[test]
fn blocking_pill_paints_red_and_tiny_follows_bucket_priority() {
    let palette = Palette::catppuccin();
    let counts = LotCounts {
        studios: 3,
        blocking: 2,
        asks: 1,
        cards: 1,
        ready: 1,
        unread: 0,
        activity: HollywoodActivity::Idle,
    };
    let mut hits = ShellHitMap::default();
    let w = pill_width(&counts, PillTier::Wide, true);
    let area = Rect::new(0, 0, w, 1);
    let mut buf = Buffer::empty(area);
    render_pill_recorded(
        &mut buf,
        area,
        &counts,
        PillTier::Wide,
        true,
        0,
        &palette,
        &mut hits,
    );
    let text = row_text(&buf, area);
    assert!(text.contains("✦ 3 · !2 · ?1 · 1c · ✓1"), "{text:?}");
    for x in 0..w {
        assert_eq!(buf[(x, 0)].bg, palette.red);
    }

    // Tiny priority: blocking > asks > cards > unread > ready.
    let tiny = |c: LotCounts| -> String {
        let tw = pill_width(&c, PillTier::Tiny, true);
        let a = Rect::new(0, 0, tw, 1);
        let mut b = Buffer::empty(a);
        let mut h = ShellHitMap::default();
        render_pill_recorded(&mut b, a, &c, PillTier::Tiny, true, 0, &palette, &mut h);
        row_text(&b, a)
    };
    assert_eq!(tiny(counts), " ✦!2 ");
    assert_eq!(
        tiny(LotCounts {
            blocking: 0,
            ..counts
        }),
        " ✦?1 "
    );
    assert_eq!(
        tiny(LotCounts {
            blocking: 0,
            asks: 0,
            ..counts
        }),
        " ✦c1 "
    );
    assert_eq!(
        tiny(LotCounts {
            blocking: 0,
            asks: 0,
            cards: 0,
            unread: 3,
            ..counts
        }),
        " ✦+3 "
    );
    assert_eq!(
        tiny(LotCounts {
            blocking: 0,
            asks: 0,
            cards: 0,
            unread: 0,
            ..counts
        }),
        " ✦✓1 "
    );
}

#[test]
fn studio_header_draws_role_badge_color_and_degrades_runner_model_effort() {
    let palette = Palette::backlot();
    let counts = LotCounts {
        studios: 1,
        ..LotCounts::default()
    };
    let header = StudioHeader {
        key: Some(StudioKey::local("beta-api")),
        studio: "beta-api".into(),
        machine: None,
        studio_focused: false,
        kind: HeaderKind::Role {
            role_key: "producer".into(),
            name: "producer".into(),
            task: None,
            mark: Some(Person::Idle),
            priority: None,
            runner: Some("agy".into()),
            model: Some("gemini-4-neon".into()),
            effort: Some("xhigh".into()),
        },
    };
    let frame = LotFrame {
        counts,
        agent: true,
        pill: true,
        header: Some(&header),
        cols: 120,
        spin: 0,
    };

    // Wide enough for full `runner · model (effort)`.
    let wide = Rect::new(0, 0, 80, 1);
    let mut wbuf = Buffer::empty(wide);
    let mut whits = ShellHitMap::default();
    render_studio_header(
        &mut wbuf,
        wide,
        &frame,
        PillTier::Wide,
        &palette,
        &mut whits,
    );
    let wtext = row_text(&wbuf, wide);
    assert!(
        wtext.contains("beta-api › Ⓟ ○  agy · gemini-4-neon (xhigh)"),
        "{wtext:?}"
    );
    let badge_x = col_of(&wbuf, wide, "Ⓟ");
    assert_eq!(wbuf[(badge_x, 0)].fg, palette.mauve);
    assert_ne!(wbuf[(badge_x, 0)].bg, Color::Reset);

    // Tighter: drops `(xhigh)` while keeping `agy · gemini-4-neon`.
    let mid_frame = LotFrame { cols: 50, ..frame };
    let mid = Rect::new(0, 0, 50, 1);
    let mut mbuf = Buffer::empty(mid);
    let mut mhits = ShellHitMap::default();
    render_studio_header(
        &mut mbuf,
        mid,
        &mid_frame,
        PillTier::Compact,
        &palette,
        &mut mhits,
    );
    let mtext = row_text(&mbuf, mid);
    assert!(mtext.contains("agy · gemini-4-neon"), "{mtext:?}");
    assert!(!mtext.contains("(xhigh)"), "{mtext:?}");

    // Narrower still: drops model, keeps runner `agy`.
    let tight_frame = LotFrame { cols: 30, ..frame };
    let tight = Rect::new(0, 0, 30, 1);
    let mut tbuf = Buffer::empty(tight);
    let mut thits = ShellHitMap::default();
    render_studio_header(
        &mut tbuf,
        tight,
        &tight_frame,
        PillTier::Tiny,
        &palette,
        &mut thits,
    );
    let ttext = row_text(&tbuf, tight);
    assert!(ttext.contains("agy"), "{ttext:?}");
    assert!(!ttext.contains("gemini-4-neon"), "{ttext:?}");
}
