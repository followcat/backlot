//! Shared Art-Deco intertitle separator (`─────═══╡ ‹✦› ╞═══─────`
//! and labeled `───══╡ ‹✦› L ‹✦› ╞══───` with width-adaptive ladder).

use ratatui::buffer::Buffer;
use ratatui::style::Style;

use crate::chrome::render::{display_width, put_text};
use crate::chrome::text::truncate_end;

/// Ornament style on the full tier of an intertitle rule.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flank {
    Chevron,
    Star,
}

impl Flank {
    fn glyph(self) -> &'static str {
        match self {
            Self::Chevron => "‹✦›",
            Self::Star => "✦",
        }
    }

    fn width(self) -> u16 {
        match self {
            Self::Chevron => 3,
            Self::Star => 1,
        }
    }
}

/// Semantic segment role so the caller can apply palette/elevation styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Part {
    Rule,
    Clasp,
    Star,
    Label,
}

/// The widest form that fits `width` cells, centred and padded with rule to
/// exactly `width`.
///
/// Ladder: full `───══╡ ‹✦› L ‹✦› ╞══───` → `══╡ ✦ L ╞══` → `╡ ✦ L ╞` →
/// `✦ L…` (`L` truncated with `…`).
/// An empty label gives `─────═══╡ ‹✦› ╞═══─────` down to `✦`.
pub(crate) fn intertitle(label: &str, width: u16, flank: Flank) -> Vec<(String, Part)> {
    if width == 0 {
        return Vec::new();
    }
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return empty_intertitle(width, flank);
    }
    labeled_intertitle(trimmed, width, flank)
}

fn empty_intertitle(width: u16, flank: Flank) -> Vec<(String, Part)> {
    // Tier 1: `─*═══╡ ‹✦› ╞═══─*` (needs at least 1 `─` arm per side).
    let full_core = 10 + flank.width();
    if width >= full_core + 2 {
        let rem = width - full_core;
        let left = rem / 2;
        let right = rem - left;
        return vec![
            ("─".repeat(left as usize), Part::Rule),
            ("═══╡".into(), Part::Clasp),
            (" ".into(), Part::Rule),
            (flank.glyph().into(), Part::Star),
            (" ".into(), Part::Rule),
            ("╞═══".into(), Part::Clasp),
            ("─".repeat(right as usize), Part::Rule),
        ];
    }

    // Tier 2: `══╡ ✦ ╞══` (core width 9), padded with `─` when wider.
    if width >= 9 {
        return pad_empty_rule(width, "══╡", "╞══", 9);
    }

    // Tier 3: `╡ ✦ ╞` (core width 5), padded with `─` when wider.
    if width >= 5 {
        return pad_empty_rule(width, "╡", "╞", 5);
    }

    // Tier 4: centred `✦` padded with `─` down to width 1.
    if width == 1 {
        return vec![("✦".into(), Part::Star)];
    }
    let rem = width - 1;
    let left = rem / 2;
    let right = rem - left;
    let mut out = Vec::new();
    if left > 0 {
        out.push(("─".repeat(left as usize), Part::Rule));
    }
    out.push(("✦".into(), Part::Star));
    if right > 0 {
        out.push(("─".repeat(right as usize), Part::Rule));
    }
    out
}

fn pad_empty_rule(width: u16, l_clasp: &str, r_clasp: &str, core_w: u16) -> Vec<(String, Part)> {
    let rem = width - core_w;
    let left = rem / 2;
    let right = rem - left;
    let mut out = Vec::with_capacity(7);
    if left > 0 {
        out.push(("─".repeat(left as usize), Part::Rule));
    }
    out.push((l_clasp.into(), Part::Clasp));
    out.push((" ".into(), Part::Rule));
    out.push(("✦".into(), Part::Star));
    out.push((" ".into(), Part::Rule));
    out.push((r_clasp.into(), Part::Clasp));
    if right > 0 {
        out.push(("─".repeat(right as usize), Part::Rule));
    }
    out
}

fn labeled_intertitle(label: &str, width: u16, flank: Flank) -> Vec<(String, Part)> {
    let label_w = display_width(label);

    // Tier 1: `─*══╡ ‹✦› L ‹✦› ╞══─*` (at least 1 `─` per side).
    let full_core = 10 + flank.width() * 2 + label_w;
    if width >= full_core + 2 {
        let rem = width - full_core;
        let left = rem / 2;
        let right = rem - left;
        return vec![
            ("─".repeat(left as usize), Part::Rule),
            ("══╡".into(), Part::Clasp),
            (" ".into(), Part::Rule),
            (flank.glyph().into(), Part::Star),
            (" ".into(), Part::Rule),
            (label.into(), Part::Label),
            (" ".into(), Part::Rule),
            (flank.glyph().into(), Part::Star),
            (" ".into(), Part::Rule),
            ("╞══".into(), Part::Clasp),
            ("─".repeat(right as usize), Part::Rule),
        ];
    }

    // Tier 2: `══╡ ✦ L ╞══` (core 10 + label_w).
    let mid_core = 10 + label_w;
    if width >= mid_core {
        return pad_labeled_rule(width, mid_core, "══╡", "╞══", label);
    }

    // Tier 3: `╡ ✦ L ╞` (core 6 + label_w).
    let compact_core = 6 + label_w;
    if width >= compact_core {
        return pad_labeled_rule(width, compact_core, "╡", "╞", label);
    }

    // Tier 4: `✦ L` / `✦ L…` down to `✦…` / `✦`.
    if width == 1 {
        return vec![("✦".into(), Part::Star)];
    }
    if width == 2 {
        return vec![("✦".into(), Part::Star), ("…".into(), Part::Label)];
    }
    let prefix_w = 2u16; // `"✦ "`
    let avail = width - prefix_w;
    let shown = if label_w <= avail {
        label.to_owned()
    } else {
        truncate_end(label, avail as usize)
    };
    let core_w = prefix_w + display_width(&shown);
    let rem = width.saturating_sub(core_w);
    let left = rem / 2;
    let right = rem - left;
    let mut out = Vec::with_capacity(5);
    if left > 0 {
        out.push(("─".repeat(left as usize), Part::Rule));
    }
    out.push(("✦".into(), Part::Star));
    out.push((" ".into(), Part::Rule));
    if !shown.is_empty() {
        out.push((shown, Part::Label));
    }
    if right > 0 {
        out.push(("─".repeat(right as usize), Part::Rule));
    }
    out
}

fn pad_labeled_rule(
    width: u16,
    core_w: u16,
    l_clasp: &str,
    r_clasp: &str,
    label: &str,
) -> Vec<(String, Part)> {
    let rem = width - core_w;
    let left = rem / 2;
    let right = rem - left;
    let mut out = Vec::with_capacity(9);
    if left > 0 {
        out.push(("─".repeat(left as usize), Part::Rule));
    }
    out.push((l_clasp.into(), Part::Clasp));
    out.push((" ".into(), Part::Rule));
    out.push(("✦".into(), Part::Star));
    out.push((" ".into(), Part::Rule));
    out.push((label.into(), Part::Label));
    out.push((" ".into(), Part::Rule));
    out.push((r_clasp.into(), Part::Clasp));
    if right > 0 {
        out.push(("─".repeat(right as usize), Part::Rule));
    }
    out
}

/// Paint pre-built [`intertitle`] segments across `[x .. x + width)` at row `y`.
pub(crate) fn draw_intertitle(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    segs: &[(String, Part)],
    style: impl Fn(Part) -> Style,
) {
    let mut cx = x;
    let end = x.saturating_add(width);
    for (text, part) in segs {
        if cx >= end {
            break;
        }
        let rem = end - cx;
        let w = display_width(text).min(rem);
        if w > 0 {
            put_text(buf, cx, y, rem, text, style(*part));
            cx = cx.saturating_add(w);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(segs: &[(String, Part)]) -> String {
        segs.iter().map(|(s, _)| s.as_str()).collect()
    }

    #[test]
    fn empty_label_scales_across_ladder_and_exact_widths() {
        assert_eq!(flat(&intertitle("", 0, Flank::Chevron)), "");
        assert_eq!(flat(&intertitle("", 1, Flank::Chevron)), "✦");
        assert_eq!(flat(&intertitle("", 3, Flank::Chevron)), "─✦─");
        assert_eq!(flat(&intertitle("", 5, Flank::Chevron)), "╡ ✦ ╞");
        assert_eq!(flat(&intertitle("", 9, Flank::Chevron)), "══╡ ✦ ╞══");
        assert_eq!(
            flat(&intertitle("", 23, Flank::Chevron)),
            "─────═══╡ ‹✦› ╞═══─────"
        );
        for w in 0..=49 {
            let segs = intertitle("", w, Flank::Chevron);
            assert_eq!(display_width(&flat(&segs)), w, "empty width {w}");
        }
    }

    #[test]
    fn labeled_scales_across_full_mid_compact_and_truncated() {
        let s = "ACT II";
        // Full requires 10 + 6 + 6 + 2 = 24 (`─══╡ ‹✦› ACT II ‹✦› ╞══─`).
        assert_eq!(
            flat(&intertitle(s, 28, Flank::Chevron)),
            "───══╡ ‹✦› ACT II ‹✦› ╞══───"
        );
        assert_eq!(
            flat(&intertitle(s, 18, Flank::Chevron)),
            "─══╡ ✦ ACT II ╞══─"
        );
        assert_eq!(flat(&intertitle(s, 16, Flank::Chevron)), "══╡ ✦ ACT II ╞══");
        assert_eq!(flat(&intertitle(s, 12, Flank::Chevron)), "╡ ✦ ACT II ╞");
        assert_eq!(flat(&intertitle(s, 7, Flank::Chevron)), "✦ ACT …");
        assert_eq!(flat(&intertitle(s, 2, Flank::Chevron)), "✦…");
        assert_eq!(flat(&intertitle(s, 1, Flank::Chevron)), "✦");
        for w in 0..=40 {
            let segs = intertitle(s, w, Flank::Chevron);
            assert_eq!(display_width(&flat(&segs)), w, "labeled width {w}");
        }
        // Star flank full tier uses single `✦` on both sides.
        assert_eq!(
            flat(&intertitle(s, 22, Flank::Star)),
            "──══╡ ✦ ACT II ✦ ╞══──"
        );
    }

    #[test]
    fn draw_intertitle_applies_per_part_styles() {
        let segs = intertitle("", 23, Flank::Chevron);
        let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, 23, 1));
        draw_intertitle(&mut buf, 0, 0, 23, &segs, |p| match p {
            Part::Rule => Style::default().fg(ratatui::style::Color::DarkGray),
            Part::Clasp => Style::default().fg(ratatui::style::Color::Yellow),
            Part::Star => Style::default().fg(ratatui::style::Color::Cyan),
            Part::Label => Style::default().fg(ratatui::style::Color::White),
        });
        assert_eq!(buf[(0, 0)].symbol(), "─");
        assert_eq!(buf[(0, 0)].fg, ratatui::style::Color::DarkGray);
        assert_eq!(buf[(5, 0)].symbol(), "═");
        assert_eq!(buf[(5, 0)].fg, ratatui::style::Color::Yellow);
        assert_eq!(buf[(11, 0)].symbol(), "✦");
        assert_eq!(buf[(11, 0)].fg, ratatui::style::Color::Cyan);
    }
}
