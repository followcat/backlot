//! Centred Babylon intertitle rules (`───══╡ ‹✦› LABEL ‹✦› ╞══───`) shared
//! by S3 (Reader & Sheet), S4 (Lot), and S5 (opening card, toasts, modals).

use ratatui::{buffer::Buffer, style::Style};

use super::text::{display_width, truncate_end};
use super::text_draw::put_text;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flank {
    /// Chevron-bracketed star (`‹✦›`).
    Chevron,
    /// Bare star (`✦`).
    Star,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    Rule,
    Clasp,
    Star,
    Label,
}

fn seg(s: impl Into<String>, part: Part) -> (String, Part) {
    (s.into(), part)
}

fn padded_center(w: usize, core: usize, mid: Vec<(String, Part)>) -> Vec<(String, Part)> {
    let left = (w - core) / 2;
    let right = (w - core) - left;
    let mut out = Vec::with_capacity(mid.len() + 2);
    if left > 0 {
        out.push(seg("─".repeat(left), Part::Rule));
    }
    out.extend(mid);
    if right > 0 {
        out.push(seg("─".repeat(right), Part::Rule));
    }
    out
}

/// The widest form that fits `width` cells, centred and padded with rule to
/// exactly `width`.
///
/// Ladder: full `───══╡ ‹✦› L ‹✦› ╞══───` → `══╡ ✦ L ╞══` → `╡ ✦ L ╞` →
/// `✦ L…` (`L` truncated with `…`). An empty label gives
/// `─────═══╡ ‹✦› ╞═══─────` down to `✦`.
pub(crate) fn intertitle(label: &str, width: u16, flank: Flank) -> Vec<(String, Part)> {
    let w = usize::from(width);
    if w == 0 {
        return Vec::new();
    }
    if label.trim().is_empty() {
        return empty_intertitle(w, flank);
    }

    let lw = display_width(label);
    let full_core = match flank {
        Flank::Chevron => 16 + lw,
        Flank::Star => 12 + lw,
    };
    if w >= full_core + 2 {
        let mid = match flank {
            Flank::Chevron => vec![
                seg("══╡ ‹", Part::Clasp),
                seg("✦", Part::Star),
                seg("› ", Part::Clasp),
                seg(label, Part::Label),
                seg(" ‹", Part::Clasp),
                seg("✦", Part::Star),
                seg("› ╞══", Part::Clasp),
            ],
            Flank::Star => vec![
                seg("══╡ ", Part::Clasp),
                seg("✦", Part::Star),
                seg(" ", Part::Clasp),
                seg(label, Part::Label),
                seg(" ", Part::Clasp),
                seg("✦", Part::Star),
                seg(" ╞══", Part::Clasp),
            ],
        };
        return padded_center(w, full_core, mid);
    }
    for (overhead, lc, rc) in [(10, "══╡ ", " ╞══"), (6, "╡ ", " ╞")] {
        if w >= overhead + lw {
            let mid = vec![
                seg(lc, Part::Clasp),
                seg("✦", Part::Star),
                seg(" ", Part::Clasp),
                seg(label, Part::Label),
                seg(rc, Part::Clasp),
            ];
            return padded_center(w, overhead + lw, mid);
        }
    }

    if w == 1 {
        return vec![seg("✦", Part::Star)];
    }
    if w == 2 {
        return vec![seg("✦", Part::Star), seg("…", Part::Label)];
    }

    let avail = w - 2;
    let fitted = if lw <= avail {
        label.to_string()
    } else {
        truncate_end(label, avail)
    };
    let used = 2 + display_width(&fitted);
    let mut out = vec![
        seg("✦", Part::Star),
        seg(" ", Part::Clasp),
        seg(fitted, Part::Label),
    ];
    if used < w {
        out.push(seg(" ".repeat(w - used), Part::Rule));
    }
    out
}

fn empty_intertitle(w: usize, flank: Flank) -> Vec<(String, Part)> {
    let full_core = match flank {
        Flank::Chevron => 13,
        Flank::Star => 11,
    };
    if w >= full_core + 10 {
        let (lc, rc) = match flank {
            Flank::Chevron => ("═══╡ ‹", "› ╞═══"),
            Flank::Star => ("═══╡ ", " ╞═══"),
        };
        return padded_center(
            w,
            full_core,
            vec![
                seg(lc, Part::Clasp),
                seg("✦", Part::Star),
                seg(rc, Part::Clasp),
            ],
        );
    }
    for (core, lc, rc) in [(9, "══╡ ", " ╞══"), (5, "╡ ", " ╞")] {
        if w >= core {
            return padded_center(
                w,
                core,
                vec![
                    seg(lc, Part::Clasp),
                    seg("✦", Part::Star),
                    seg(rc, Part::Clasp),
                ],
            );
        }
    }
    padded_center(w, 1, vec![seg("✦", Part::Star)])
}

/// Paint pre-built [`intertitle`] segments horizontally inside `[x, x + width)`
/// at row `y`, centring them when narrower than `width` and clipping cleanly at
/// wide-character boundaries when wider.
pub(crate) fn draw_intertitle(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    segs: &[(String, Part)],
    style: impl Fn(Part) -> Style,
) {
    if width == 0 || y >= buf.area.bottom() || x >= buf.area.right() {
        return;
    }
    let avail = usize::from(width.min(buf.area.right().saturating_sub(x)));
    if avail == 0 {
        return;
    }
    let total_w: usize = segs.iter().map(|(s, _)| display_width(s)).sum();
    let pad_left = avail.saturating_sub(total_w) / 2;
    let mut cursor = x.saturating_add(pad_left as u16);
    let right = x.saturating_add(avail as u16);

    for (text, part) in segs {
        if cursor >= right {
            break;
        }
        let seg_w = display_width(text);
        if seg_w == 0 {
            continue;
        }
        let rem = usize::from(right - cursor);
        let st = style(*part);
        if seg_w <= rem {
            put_text(buf, cursor, y, seg_w as u16, text, st);
            cursor = cursor.saturating_add(seg_w as u16);
        } else {
            let clipped = truncate_end(text, rem);
            let cw = display_width(&clipped);
            put_text(buf, cursor, y, cw as u16, &clipped, st);
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Modifier};

    fn joined(segs: &[(String, Part)]) -> String {
        segs.iter().map(|(s, _)| s.as_str()).collect()
    }

    #[test]
    fn exact_width_across_ladder_and_cjk() {
        for flank in [Flank::Chevron, Flank::Star] {
            for label in ["", "Label", " T H E   E N D ", "§R2.D · 版本與架構"] {
                for w in 0..=64u16 {
                    let segs = intertitle(label, w, flank);
                    let got: usize = segs.iter().map(|(s, _)| display_width(s)).sum();
                    assert_eq!(got, usize::from(w), "width mismatch at {w} for {label:?}");
                }
            }
        }
    }

    #[test]
    fn chevron_ladder_transitions() {
        assert_eq!(
            joined(&intertitle("Label", 27, Flank::Chevron)),
            "───══╡ ‹✦› Label ‹✦› ╞══───"
        );
        assert_eq!(
            joined(&intertitle("Label", 30, Flank::Chevron)),
            "────══╡ ‹✦› Label ‹✦› ╞══─────"
        );
        assert_eq!(
            joined(&intertitle("Label", 23, Flank::Chevron)),
            "─══╡ ‹✦› Label ‹✦› ╞══─"
        );
        assert_eq!(
            joined(&intertitle("Label", 22, Flank::Chevron)),
            "───══╡ ✦ Label ╞══────"
        );
        assert_eq!(
            joined(&intertitle("Label", 15, Flank::Chevron)),
            "══╡ ✦ Label ╞══"
        );
        assert_eq!(
            joined(&intertitle("Label", 11, Flank::Chevron)),
            "╡ ✦ Label ╞"
        );
        assert_eq!(
            joined(&intertitle("Label", 10, Flank::Chevron)),
            "✦ Label   "
        );
        assert_eq!(joined(&intertitle("Label", 6, Flank::Chevron)), "✦ Lab…");
        assert_eq!(joined(&intertitle("Label", 3, Flank::Chevron)), "✦ …");
        assert_eq!(joined(&intertitle("Label", 2, Flank::Chevron)), "✦…");
        assert_eq!(joined(&intertitle("Label", 1, Flank::Chevron)), "✦");
        assert!(intertitle("Label", 0, Flank::Chevron).is_empty());
    }

    #[test]
    fn star_the_end_and_parts() {
        let segs = intertitle(" T H E   E N D ", 33, Flank::Star);
        assert_eq!(joined(&segs), "───══╡ ✦  T H E   E N D  ✦ ╞══───");
        assert_eq!(
            segs,
            vec![
                ("───".to_string(), Part::Rule),
                ("══╡ ".to_string(), Part::Clasp),
                ("✦".to_string(), Part::Star),
                (" ".to_string(), Part::Clasp),
                (" T H E   E N D ".to_string(), Part::Label),
                (" ".to_string(), Part::Clasp),
                ("✦".to_string(), Part::Star),
                (" ╞══".to_string(), Part::Clasp),
                ("───".to_string(), Part::Rule),
            ]
        );
        assert_eq!(
            joined(&intertitle(" T H E   E N D ", 25, Flank::Star)),
            "══╡ ✦  T H E   E N D  ╞══"
        );
        assert_eq!(
            joined(&intertitle(" T H E   E N D ", 21, Flank::Star)),
            "╡ ✦  T H E   E N D  ╞"
        );
    }

    #[test]
    fn empty_label_ladder() {
        assert_eq!(
            joined(&intertitle("", 23, Flank::Chevron)),
            "─────═══╡ ‹✦› ╞═══─────"
        );
        assert_eq!(
            joined(&intertitle("", 21, Flank::Star)),
            "─────═══╡ ✦ ╞═══─────"
        );
        assert_eq!(joined(&intertitle("", 9, Flank::Chevron)), "══╡ ✦ ╞══");
        assert_eq!(joined(&intertitle("", 5, Flank::Chevron)), "╡ ✦ ╞");
        assert_eq!(joined(&intertitle("", 3, Flank::Chevron)), "─✦─");
        assert_eq!(joined(&intertitle("", 1, Flank::Chevron)), "✦");
        assert!(intertitle("", 0, Flank::Chevron).is_empty());
    }

    #[test]
    fn cjk_narrow_truncation_keeps_boundary_and_width() {
        let s4 = joined(&intertitle("版本", 4, Flank::Chevron));
        assert_eq!(s4, "✦ … ");
        assert_eq!(display_width(&s4), 4);
        let s5 = joined(&intertitle("版本設計", 5, Flank::Chevron));
        assert_eq!(s5, "✦ 版…");
        assert_eq!(display_width(&s5), 5);
    }

    #[test]
    fn draw_intertitle_centres_styles_and_clips() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 41, 1));
        let segs = intertitle(" T H E   E N D ", 33, Flank::Star);
        draw_intertitle(&mut buf, 0, 0, 41, &segs, |p| match p {
            Part::Rule => Style::default().fg(Color::DarkGray),
            Part::Clasp => Style::default().fg(Color::Yellow),
            Part::Star => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            Part::Label => Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        });
        let row: String = (0..41).map(|x| buf[(x, 0)].symbol()).collect();
        assert_eq!(row, "    ───══╡ ✦  T H E   E N D  ✦ ╞══───    ");
        assert_eq!(buf[(4, 0)].fg, Color::DarkGray);
        assert_eq!(buf[(7, 0)].fg, Color::Yellow);
        assert_eq!(buf[(11, 0)].fg, Color::Red);
        assert_eq!(buf[(14, 0)].fg, Color::White);

        let mut narrow = Buffer::empty(Rect::new(0, 0, 7, 1));
        let wide_segs = intertitle("版本設計圖", 16, Flank::Chevron);
        draw_intertitle(&mut narrow, 0, 0, 7, &wide_segs, |_| Style::default());
        let clipped_row: String = (0..7).map(|x| narrow[(x, 0)].symbol()).collect();
        assert!(clipped_row.contains('✦') && clipped_row.contains('…'));
    }

    #[test]
    fn empty_label_scales_across_ladder_and_exact_widths() {
        assert_eq!(joined(&intertitle("", 0, Flank::Chevron)), "");
        assert_eq!(joined(&intertitle("", 1, Flank::Chevron)), "✦");
        assert_eq!(joined(&intertitle("", 3, Flank::Chevron)), "─✦─");
        assert_eq!(joined(&intertitle("", 5, Flank::Chevron)), "╡ ✦ ╞");
        assert_eq!(joined(&intertitle("", 9, Flank::Chevron)), "══╡ ✦ ╞══");
        assert_eq!(
            joined(&intertitle("", 23, Flank::Chevron)),
            "─────═══╡ ‹✦› ╞═══─────"
        );
        for w in 0..=49 {
            let segs = intertitle("", w, Flank::Chevron);
            assert_eq!(display_width(&joined(&segs)), w as usize, "empty width {w}");
        }
    }

    #[test]
    fn labeled_scales_across_full_mid_compact_and_truncated() {
        let s = "ACT II";
        assert_eq!(
            joined(&intertitle(s, 28, Flank::Chevron)),
            "───══╡ ‹✦› ACT II ‹✦› ╞══───"
        );
        assert_eq!(
            joined(&intertitle(s, 18, Flank::Chevron)),
            "─══╡ ✦ ACT II ╞══─"
        );
        assert_eq!(
            joined(&intertitle(s, 16, Flank::Chevron)),
            "══╡ ✦ ACT II ╞══"
        );
        assert_eq!(joined(&intertitle(s, 12, Flank::Chevron)), "╡ ✦ ACT II ╞");
        assert_eq!(joined(&intertitle(s, 7, Flank::Chevron)), "✦ ACT …");
        assert_eq!(joined(&intertitle(s, 2, Flank::Chevron)), "✦…");
        assert_eq!(joined(&intertitle(s, 1, Flank::Chevron)), "✦");
        for w in 0..=40 {
            let segs = intertitle(s, w, Flank::Chevron);
            assert_eq!(
                display_width(&joined(&segs)),
                w as usize,
                "labeled width {w}"
            );
        }
        assert_eq!(
            joined(&intertitle(s, 22, Flank::Star)),
            "──══╡ ✦ ACT II ✦ ╞══──"
        );
    }

    #[test]
    fn draw_intertitle_applies_per_part_styles() {
        let segs = intertitle("", 23, Flank::Chevron);
        let mut buf = Buffer::empty(Rect::new(0, 0, 23, 1));
        draw_intertitle(&mut buf, 0, 0, 23, &segs, |p| match p {
            Part::Rule => Style::default().fg(Color::DarkGray),
            Part::Clasp => Style::default().fg(Color::Yellow),
            Part::Star => Style::default().fg(Color::Cyan),
            Part::Label => Style::default().fg(Color::White),
        });
        assert_eq!(buf[(0, 0)].symbol(), "─");
        assert_eq!(buf[(0, 0)].fg, Color::DarkGray);
        assert_eq!(buf[(5, 0)].symbol(), "═");
        assert_eq!(buf[(5, 0)].fg, Color::Yellow);
        assert_eq!(buf[(11, 0)].symbol(), "✦");
        assert_eq!(buf[(11, 0)].fg, Color::Cyan);
    }
}
