// Derived from herdr src/ui/text.rs (Apache-2.0).

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub(crate) fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

pub(crate) fn truncate_end(text: &str, max_width: usize) -> String {
    if display_width(text) <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        return "…".to_string();
    }

    let prefix = take_prefix_width(text, max_width.saturating_sub(1));
    format!("{prefix}…")
}

fn take_prefix_width(text: &str, max_width: usize) -> String {
    let mut output = String::new();
    let mut width = 0usize;
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_width > max_width {
            break;
        }
        output.push(ch);
        width += ch_width;
    }
    output
}

/// The longest hint tier that fits `width` — never a mid-word cut
/// (A1.2/A1.3). Tiers are tried in order, so callers list them
/// longest first and end with a tiny fallback like `"esc"` or
/// `"? keys"`; `""` when even the last tier is too wide.
pub(crate) fn fit_hints<'a>(tiers: &[&'a str], width: usize) -> &'a str {
    tiers
        .iter()
        .copied()
        .find(|tier| display_width(tier) <= width)
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_hints_picks_the_longest_tier_that_fits() {
        const TIERS: [&str; 4] = [
            "↑↓ move  ⏎ open  m menu  ? keys  esc",
            "↑↓ move  ⏎ open  esc",
            "↑↓ move  ? keys",
            "? keys",
        ];
        for width in [160, 120, 80, 36] {
            assert_eq!(fit_hints(&TIERS, width), TIERS[0], "width {width}");
        }
        for width in [35, 28, 25, 22, 20] {
            assert_eq!(fit_hints(&TIERS, width), TIERS[1], "width {width}");
        }
        for width in [19, 16, 15] {
            assert_eq!(fit_hints(&TIERS, width), TIERS[2], "width {width}");
        }
        for width in [14, 6] {
            assert_eq!(fit_hints(&TIERS, width), TIERS[3], "width {width}");
        }
        assert_eq!(fit_hints(&TIERS, 5), "");
        assert_eq!(fit_hints(&TIERS, 0), "");
    }

    #[test]
    fn truncate_end_uses_display_width() {
        let text = truncate_end("提交 herdr 的反馈", 16);

        assert_eq!(text, "提交 herdr 的反…");
        assert!(display_width(&text) <= 16);
    }
}
