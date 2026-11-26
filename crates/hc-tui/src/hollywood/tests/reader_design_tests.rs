//! Fail-first and buffer tests for PR 6 (`ReaderKind::Design`), section
//! anchor parsing, header/chip layout, 200x44 / 100x40 renders, and no-design
//! identity.

use super::draw_tests::{draw, layer, lines};
use super::summary;
use crate::chrome::palette::Palette;
use crate::hollywood::{
    CardView, DesignSection, ReaderKind, ReaderMeta, ReaderOrigin, ReaderRenderOpts,
    parse_design_sections, render_view, render_view_with_opts,
};
use crate::studio::StudioKey;
use backlot_model::design::{DesignPhase, SignMode};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

fn sample_design_lines() -> Vec<String> {
    [
        "# Design v1 · acme-shop",
        "",
        "```markdown",
        "## R99",
        "### §R99.W Ignored inside code block",
        "```",
        "",
        "## R1",
        "",
        "### §R1.W Proposal · writer (#14)",
        "",
        "Define the Reader header layout cleanly across wide and medium viewports so all status tokens remain readable without clipping.",
        "",
        "### §R1.D Buildability · director (#15)",
        "",
        "Verify anchor parsing works against long wrapped paragraphs and skips fenced examples before jumping to round two.",
        "",
        "### §R1.P Summary · producer (#16)",
        "",
        "Aligned on single-line header navigation plus bottom action strip with reply and sign off controls.",
        "",
        "## R2",
        "",
        "### §R2.U User input · user (#17)",
        "",
        "Keep finalized documents read-only without a sign-off prompt and retain identical buffers when design is absent.",
        "",
        "### §R2.W Proposal · writer (#18)",
        "",
        "Second round refinements covering SuggestSkip copy, section index indicators, and keyboard ergonomics.",
        "",
        "### §R2.W+ Supplement · writer (#19)",
        "",
        "Added narrow-width chip fallback checks at 100x40 alongside the 200x44 wide viewport dump.",
        "",
        "## Sign-off (#20)",
        "",
        "Approved by user.",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn design_view(phase: DesignPhase, file: &str, finalized: bool) -> CardView {
    let raw_lines = sample_design_lines();
    CardView {
        card: None,
        title: "Design".into(),
        raw_body: raw_lines.join("\n"),
        lines: raw_lines,
        scroll: 0,
        meta: ReaderMeta {
            kind: ReaderKind::Design,
            studio: Some("acme-shop".into()),
            file: Some(file.into()),
            design_version: Some(1),
            design_round: Some(2),
            design_phase: Some(phase),
            design_sign_mode: finalized.then_some(SignMode::Approved),
            origin: ReaderOrigin::Design(StudioKey::local("acme-shop")),
            ..ReaderMeta::default()
        },
        ..CardView::default()
    }
}

#[test]
fn parse_design_sections_extracts_rounds_roles_and_skips_fences() {
    let doc = sample_design_lines();
    let sections: Vec<DesignSection> = parse_design_sections(&doc);
    let anchors: Vec<&str> = sections.iter().map(|s| s.anchor.as_str()).collect();
    assert_eq!(
        anchors,
        vec![
            "R1", "§R1.W", "§R1.D", "§R1.P", "R2", "§R2.U", "§R2.W", "§R2.W+", "Sign-off",
        ],
        "expected ordered round/role anchors outside code fences, got: {sections:?}"
    );
}

#[test]
fn design_reader_header_and_chips_reflect_phase_and_omit_sign_off_when_finalized() {
    let palette = Palette::catppuccin();
    let area = Rect::new(0, 0, 120, 32);

    // WaitingUser: shows DESIGN v1 · R2 · acme-shop, waiting phase, file, [r] reply, [a] sign off.
    let waiting = design_view(DesignPhase::WaitingUser, "design/v1.md", false);
    let mut buf = Buffer::empty(area);
    let vr = render_view(&mut buf, area, &waiting, None, &palette);
    let text = lines(&buf, area).join("\n");
    assert!(
        text.contains("DESIGN v1"),
        "missing DESIGN v1 header:\n{text}"
    );
    assert!(text.contains("R2"), "missing R2:\n{text}");
    assert!(text.contains("acme-shop"), "missing studio:\n{text}");
    assert!(text.contains("design/v1.md"), "missing file path:\n{text}");
    assert!(
        text.contains("reply") && text.contains("sign off"),
        "waiting design reader must show reply and sign off actions:\n{text}"
    );
    assert!(
        text.contains("esc back"),
        "waiting design reader footer must include esc back:\n{text}"
    );
    assert!(
        !vr.sections.is_empty(),
        "expected ViewRender.sections to record design anchor offsets"
    );

    // SuggestSkip: also shows skip suggested and keeps sign off available.
    let skip = design_view(DesignPhase::SuggestSkip, "design/v1.md", false);
    let mut skip_buf = Buffer::empty(area);
    render_view(&mut skip_buf, area, &skip, None, &palette);
    let skip_text = lines(&skip_buf, area).join("\n");
    assert!(
        skip_text.contains("skip suggested"),
        "expected skip suggested phase label:\n{skip_text}"
    );
    assert!(
        skip_text.contains("sign off") && skip_text.contains("reply"),
        "SuggestSkip design reader must show reply and sign off:\n{skip_text}"
    );

    // Intermediate phases show semantic labels without sign off.
    for (phase, expected_word) in [
        (DesignPhase::WaitingProposal, "proposal"),
        (DesignPhase::WaitingBuildability, "review"),
        (DesignPhase::WaitingSummary, "summary"),
        (DesignPhase::Idle, "idle"),
    ] {
        let mid = design_view(phase, "design/v1.md", false);
        let mut mid_buf = Buffer::empty(area);
        render_view(&mut mid_buf, area, &mid, None, &palette);
        let mid_text = lines(&mid_buf, area).join("\n");
        assert!(
            mid_text.contains(expected_word),
            "expected phase `{expected_word}`:\n{mid_text}"
        );
        assert!(
            !mid_text.contains("sign off"),
            "non-user phase `{expected_word}` must omit sign off:\n{mid_text}"
        );
    }

    // Skipped sign-off renders `skipped` without sign off.
    let mut skipped_view = design_view(DesignPhase::Signed, "design/v1-final.md", true);
    skipped_view.meta.design_sign_mode = Some(SignMode::Skipped);
    let mut skipped_buf = Buffer::empty(area);
    render_view(&mut skipped_buf, area, &skipped_view, None, &palette);
    let skipped_text = lines(&skipped_buf, area).join("\n");
    assert!(skipped_text.contains("skipped"), "{skipped_text}");
    assert!(!skipped_text.contains("sign off"), "{skipped_text}");

    // Finalized / Signed: never shows sign off chip.
    let signed = design_view(DesignPhase::Signed, "design/v1-final.md", true);
    let mut signed_buf = Buffer::empty(area);
    render_view(&mut signed_buf, area, &signed, None, &palette);
    let signed_text = lines(&signed_buf, area).join("\n");
    assert!(
        signed_text.contains("design/v1-final.md"),
        "finalized reader must show final path:\n{signed_text}"
    );
    assert!(
        !signed_text.contains("sign off"),
        "finalized design document must NOT render sign off chip:\n{signed_text}"
    );
}

#[test]
fn design_reader_200x44_and_100x40_render_cleanly_with_scroll_anchors() {
    let palette = Palette::catppuccin();
    let mut view = design_view(DesignPhase::WaitingUser, "design/v1.md", false);
    view.scroll = 4;

    let area_wide = Rect::new(0, 0, 200, 44);
    let mut buf_wide = Buffer::empty(area_wide);
    let vr_wide = render_view_with_opts(
        &mut buf_wide,
        area_wide,
        &view,
        None,
        &palette,
        ReaderRenderOpts {
            compose: None,
            now_ms: 0,
            shadow: false,
        },
    );
    let wide_text = lines(&buf_wide, area_wide).join("\n");
    assert!(wide_text.contains("DESIGN v1"), "{wide_text}");
    assert!(wide_text.contains("sign off"), "{wide_text}");
    assert!(!vr_wide.sections.is_empty(), "expected sections at 200x44");

    let area_100 = Rect::new(0, 0, 100, 40);
    let mut buf_100 = Buffer::empty(area_100);
    let vr_100 = render_view_with_opts(
        &mut buf_100,
        area_100,
        &view,
        None,
        &palette,
        ReaderRenderOpts {
            compose: None,
            now_ms: 0,
            shadow: false,
        },
    );
    let text_100 = lines(&buf_100, area_100).join("\n");
    assert!(text_100.contains("DESIGN v1"), "{text_100}");
    assert!(text_100.contains("sign off"), "{text_100}");
    assert!(!vr_100.sections.is_empty(), "expected sections at 100x40");

    if std::env::var_os("DESIGN_READER_DUMP").is_some() {
        eprintln!("=== DESIGN READER 200x44 ===");
        eprintln!("{wide_text}");
        eprintln!("=== DESIGN READER 100x40 ===");
        eprintln!("{text_100}");
    }
}

#[test]
fn no_design_renders_unchanged_without_design_markers() {
    let palette = Palette::catppuccin();
    let s = summary();
    let layer_state = layer(crate::hollywood::LayerMode::Expanded);

    let area = Rect::new(0, 0, 120, 32);
    let (call_buf, _) = draw(area, Some(&s), true, &layer_state, false, &palette);
    let (call_buf_2, _) = draw(area, Some(&s), true, &layer_state, false, &palette);
    assert_eq!(call_buf.content(), call_buf_2.content());
    let call_text = lines(&call_buf, area).join("\n");
    assert!(!call_text.contains("DESIGN"), "{call_text}");
    assert!(!call_text.contains("sign off"), "{call_text}");

    let report = CardView {
        card: None,
        title: "Report".into(),
        raw_body: "Shipped feature safely.".into(),
        lines: vec!["Shipped feature safely.".into()],
        meta: ReaderMeta {
            kind: ReaderKind::Report,
            studio: Some("acme-shop".into()),
            seq: Some(12),
            from: Some("producer".into()),
            file: Some("reports/12.md".into()),
            origin: ReaderOrigin::StudioExchange(StudioKey::local("acme-shop"), 12),
            ..ReaderMeta::default()
        },
        ..CardView::default()
    };
    let mut report_buf = Buffer::empty(area);
    render_view(&mut report_buf, area, &report, None, &palette);
    let report_text = lines(&report_buf, area).join("\n");
    assert!(report_text.contains("■ REPORT #12"), "{report_text}");
    assert!(!report_text.contains("DESIGN"), "{report_text}");
    assert!(!report_text.contains("sign off"), "{report_text}");
}
