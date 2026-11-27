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
        switch_state: None,
    };
    let mut hits = ShellHitMap::default();
    let w = pill_width(&counts, PillTier::Full, true, true);
    let area = Rect::new(0, 0, w, 1);
    let mut buf = Buffer::empty(area);
    render_pill_recorded(
        &mut buf,
        area,
        &counts,
        PillTier::Full,
        true,
        0,
        true,
        &palette,
        &mut hits,
    );
    let text = row_text(&buf, area);
    assert!(
        text.contains("╡ ✦ 4 · ?1 · 2c · ✓1 ready · alt+y ╞"),
        "{text:?}"
    );

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
        switch_state: None,
    };
    let mut hits = ShellHitMap::default();
    let w = pill_width(&counts, PillTier::Wide, true, true);
    let area = Rect::new(0, 0, w, 1);
    let mut buf = Buffer::empty(area);
    render_pill_recorded(
        &mut buf,
        area,
        &counts,
        PillTier::Wide,
        true,
        0,
        true,
        &palette,
        &mut hits,
    );
    let text = row_text(&buf, area);
    assert!(text.contains("╡ ◉ 3 · !2 · ?1 · 1c · ✓1 ╞"), "{text:?}");
    for x in 0..w {
        assert_eq!(buf[(x, 0)].bg, palette.red);
    }

    // Tiny priority: blocking > asks > cards > unread > ready.
    let tiny = |c: LotCounts| -> String {
        let tw = pill_width(&c, PillTier::Tiny, true, true);
        let a = Rect::new(0, 0, tw, 1);
        let mut b = Buffer::empty(a);
        let mut h = ShellHitMap::default();
        render_pill_recorded(
            &mut b,
            a,
            &c,
            PillTier::Tiny,
            true,
            0,
            true,
            &palette,
            &mut h,
        );
        row_text(&b, a)
    };
    assert_eq!(tiny(counts), "╡ ◉!2 ╞");
    assert_eq!(
        tiny(LotCounts {
            blocking: 0,
            ..counts
        }),
        "╡ ✦?1 ╞"
    );
    assert_eq!(
        tiny(LotCounts {
            blocking: 0,
            asks: 0,
            ..counts
        }),
        "╡ ✦c1 ╞"
    );
    assert_eq!(
        tiny(LotCounts {
            blocking: 0,
            asks: 0,
            cards: 0,
            unread: 3,
            ..counts
        }),
        "╡ ✦+3 ╞"
    );
    assert_eq!(
        tiny(LotCounts {
            blocking: 0,
            asks: 0,
            cards: 0,
            unread: 0,
            ..counts
        }),
        "╡ ✦✓1 ╞"
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
        unfocused_cue: false,
        cols: 120,
        spin: 0,
        studio_chrome: true,
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

#[test]
fn studio_header_paints_raised_row_capsule_with_terminal_16_fallback_and_wide_cjk() {
    use ratatui::style::Modifier;
    let counts = LotCounts {
        studios: 1,
        ..LotCounts::default()
    };
    let make_header = |name: &str| StudioHeader {
        key: Some(StudioKey::local(name)),
        studio: name.into(),
        machine: None,
        kind: HeaderKind::Role {
            role_key: "producer".into(),
            name: "producer".into(),
            task: None,
            mark: Some(Person::Idle),
            priority: None,
            runner: None,
            model: None,
            effort: None,
        },
    };
    // RGB theme (`backlot`): `elev.raised_row` + bold `palette.text`, 1-cell padded capsule + hit.
    let header = make_header("beta-api");
    let frame = LotFrame {
        counts,
        agent: false,
        pill: true,
        header: Some(&header),
        unfocused_cue: false,
        cols: 80,
        spin: 0,
        studio_chrome: true,
    };
    let pal = Palette::backlot();
    let elev = pal.elevation();
    let area = Rect::new(0, 0, 60, 1);
    let mut buf = Buffer::empty(area);
    let mut hits = ShellHitMap::default();
    render_studio_header(&mut buf, area, &frame, PillTier::Wide, &pal, &mut hits);
    assert_eq!(hits.studio_header, Rect::new(0, 0, 10, 1));
    for x in 0..10u16 {
        assert_eq!(buf[(x, 0)].bg, elev.raised_row, "capsule bg x={x}");
        assert_eq!(buf[(x, 0)].fg, pal.text, "capsule fg x={x}");
        assert!(buf[(x, 0)].modifier.contains(Modifier::BOLD));
    }
    assert_ne!(buf[(10, 0)].bg, elev.raised_row);

    // 16-color terminal fallback: BOLD + REVERSED.
    let term_pal = Palette::terminal();
    let mut tbuf = Buffer::empty(area);
    let mut thits = ShellHitMap::default();
    render_studio_header(
        &mut tbuf,
        area,
        &frame,
        PillTier::Wide,
        &term_pal,
        &mut thits,
    );
    for x in 0..10u16 {
        assert!(tbuf[(x, 0)].modifier.contains(Modifier::BOLD));
        assert!(tbuf[(x, 0)].modifier.contains(Modifier::REVERSED));
    }

    // CJK wide studio name: full display width + 2 pad cells covered by capsule and hit rect.
    let cjk = make_header("中文工作");
    let cframe = LotFrame {
        header: Some(&cjk),
        ..frame
    };
    let mut cbuf = Buffer::empty(area);
    let mut chits = ShellHitMap::default();
    render_studio_header(&mut cbuf, area, &cframe, PillTier::Wide, &pal, &mut chits);
    assert_eq!(chits.studio_header, Rect::new(0, 0, 10, 1));
    assert_eq!(cbuf[(0, 0)].bg, elev.raised_row);
    assert_eq!(cbuf[(9, 0)].bg, elev.raised_row);
    assert_eq!(cbuf[(10, 0)].symbol(), "›");
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
