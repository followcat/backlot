//! Flourished intertitle rules (`───══╡ ‹✦› TITLE ‹✦› ╞══───`) shared across Babylon surfaces (common rule 11).
#![allow(dead_code)]

use ratatui::buffer::Buffer;
use ratatui::style::Style;
use unicode_width::UnicodeWidthStr;

use super::text::truncate_end;
use super::text_draw::put_segment;

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Part {
    Rule,
    Clasp,
    Star,
    Label,
}

fn seg_width(segs: &[(String, Part)]) -> usize {
    segs.iter().map(|(s, _)| s.width()).sum()
}

fn pad_with_rules(core: Vec<(String, Part)>, width: usize) -> Vec<(String, Part)> {
    let core_w = seg_width(&core);
    let pad = width.saturating_sub(core_w);
    let left = pad / 2;
    let right = pad - left;
    let mut out = Vec::with_capacity(core.len() + 2);
    if left > 0 {
        out.push(("─".repeat(left), Part::Rule));
    }
    out.extend(core);
    if right > 0 {
        out.push(("─".repeat(right), Part::Rule));
    }
    out
}

/// Returns the widest intertitle form that fits `width` cells, centred and
/// padded with rule characters to exactly `width` display columns.
///
/// Ladder (non-empty label):
/// `───══╡ ‹✦› L ‹✦› ╞══───` → `══╡ ✦ L ╞══` → `╡ ✦ L ╞` → `✦ L…` (truncated with `…`).
/// Empty label: `─────═══╡ ‹✦› ╞═══─────` down to `✦`.
pub(crate) fn intertitle(label: &str, width: u16, flank: Flank) -> Vec<(String, Part)> {
    let target = usize::from(width);
    if target == 0 {
        return Vec::new();
    }

    if label.trim().is_empty() {
        let full = vec![
            ("═══╡ ".to_string(), Part::Clasp),
            (flank.glyph().to_string(), Part::Star),
            (" ╞═══".to_string(), Part::Clasp),
        ];
        if seg_width(&full) + 2 <= target {
            return pad_with_rules(full, target);
        }
        let mid = vec![
            ("══╡ ".to_string(), Part::Clasp),
            ("✦".to_string(), Part::Star),
            (" ╞══".to_string(), Part::Clasp),
        ];
        if seg_width(&mid) <= target {
            return pad_with_rules(mid, target);
        }
        let clasp = vec![
            ("╡ ".to_string(), Part::Clasp),
            ("✦".to_string(), Part::Star),
            (" ╞".to_string(), Part::Clasp),
        ];
        if seg_width(&clasp) <= target {
            return pad_with_rules(clasp, target);
        }
        return pad_with_rules(vec![("✦".to_string(), Part::Star)], target);
    }

    // 1. Full symmetrical flanked form with at least 1 rule cell on each side.
    let full = vec![
        ("══╡ ".to_string(), Part::Clasp),
        (flank.glyph().to_string(), Part::Star),
        (" ".to_string(), Part::Clasp),
        (label.to_string(), Part::Label),
        (" ".to_string(), Part::Clasp),
        (flank.glyph().to_string(), Part::Star),
        (" ╞══".to_string(), Part::Clasp),
    ];
    if seg_width(&full) + 2 <= target {
        return pad_with_rules(full, target);
    }

    // 2. Single-star double-clasp form: `══╡ ✦ L ╞══`.
    let mid = vec![
        ("══╡ ".to_string(), Part::Clasp),
        ("✦".to_string(), Part::Star),
        (" ".to_string(), Part::Clasp),
        (label.to_string(), Part::Label),
        (" ╞══".to_string(), Part::Clasp),
    ];
    if seg_width(&mid) <= target {
        return pad_with_rules(mid, target);
    }

    // 3. Minimal clasp form: `╡ ✦ L ╞`.
    let compact = vec![
        ("╡ ".to_string(), Part::Clasp),
        ("✦".to_string(), Part::Star),
        (" ".to_string(), Part::Clasp),
        (label.to_string(), Part::Label),
        (" ╞".to_string(), Part::Clasp),
    ];
    if seg_width(&compact) <= target {
        return pad_with_rules(compact, target);
    }

    // 4. Bare star + truncated label (`✦ L…`), padded with spaces if wide CJK leaves 1 cell.
    if target == 1 {
        return vec![("✦".to_string(), Part::Star)];
    }
    if target == 2 {
        return vec![
            ("✦".to_string(), Part::Star),
            (" ".to_string(), Part::Clasp),
        ];
    }
    let trunc = truncate_end(label, target.saturating_sub(2));
    let mut out = vec![
        ("✦".to_string(), Part::Star),
        (" ".to_string(), Part::Clasp),
        (trunc, Part::Label),
    ];
    let used = seg_width(&out);
    if used < target {
        out.push((" ".repeat(target - used), Part::Clasp));
    }
    out
}

pub(crate) fn draw_intertitle(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    segs: &[(String, Part)],
    style: impl Fn(Part) -> Style,
) {
    let right = x.saturating_add(width);
    let mut cursor = x;
    for (text, part) in segs {
        if cursor >= right {
            break;
        }
        let w = text.width().min(usize::from(right.saturating_sub(cursor))) as u16;
        if w == 0 {
            continue;
        }
        put_segment(buf, cursor, y, right, text, style(*part));
        cursor = cursor.saturating_add(w);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn join(segs: &[(String, Part)]) -> String {
        segs.iter().map(|(s, _)| s.as_str()).collect()
    }

    #[test]
    fn ladder_steps_down_cleanly_to_exact_width() {
        let wide = intertitle("SCENE", 36, Flank::Chevron);
        assert_eq!(join(&wide).width(), 36);
        assert_eq!(join(&wide), "───────══╡ ‹✦› SCENE ‹✦› ╞══────────");

        let end_rule = intertitle(" T H E   E N D ", 34, Flank::Star);
        assert_eq!(join(&end_rule).width(), 34);
        assert_eq!(join(&end_rule), "───══╡ ✦  T H E   E N D  ✦ ╞══────");

        let mid = intertitle("SCENE", 20, Flank::Chevron);
        assert_eq!(join(&mid).width(), 20);
        assert_eq!(join(&mid), "──══╡ ✦ SCENE ╞══───");

        let compact = intertitle("SCENE", 11, Flank::Chevron);
        assert_eq!(join(&compact).width(), 11);
        assert_eq!(join(&compact), "╡ ✦ SCENE ╞");

        let trunc = intertitle("LONG TITLE HERE", 9, Flank::Chevron);
        assert_eq!(join(&trunc).width(), 9);
        assert_eq!(join(&trunc), "✦ LONG T…");
    }

    #[test]
    fn empty_label_and_cjk_widths_stay_exact() {
        let empty = intertitle("", 25, Flank::Chevron);
        assert_eq!(join(&empty).width(), 25);
        assert_eq!(join(&empty), "──────═══╡ ‹✦› ╞═══──────");

        let tiny = intertitle("", 3, Flank::Chevron);
        assert_eq!(join(&tiny).width(), 3);
        assert_eq!(join(&tiny), "─✦─");

        let cjk = intertitle("中文标题测试", 10, Flank::Star);
        assert_eq!(join(&cjk).width(), 10);
        assert!(join(&cjk).starts_with("✦ "));
        assert!(join(&cjk).contains('…'));
    }
}
