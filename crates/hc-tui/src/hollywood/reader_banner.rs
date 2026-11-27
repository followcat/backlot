//! Babylon Markdown banners (`───══╡ ‹✦› LABEL ‹✦› ╞══───`), `gilt` `║`
//! margin bars, and block rendering helpers for the Reader (`reader_wrap.rs`).

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::reader_md::{
    MdBlock, MdInline, design_section_for_heading, is_banner_heading, parse_markdown,
};
use super::reader_wrap::{Cell, cell_of, emit_wrapped, push_inline_cells, to_line, wrap_cells};
use crate::chrome::intertitle::{Flank, Part, intertitle};
use crate::chrome::palette::Palette;
use crate::chrome::popup_frame::overlay_bg;
use crate::chrome::text::display_width;

/// Render Markdown lines into styled wrapped lines plus design section offsets.
pub(super) fn render_markdown(
    lines: &[String],
    content_w: usize,
    palette: &Palette,
    plain: bool,
) -> (Vec<Line<'static>>, Vec<(String, usize)>) {
    let blocks = parse_markdown(lines);
    let mut out = Vec::new();
    let mut sections = Vec::new();
    let indent_2 = [
        cell_of(' ', Style::default()),
        cell_of(' ', Style::default()),
    ];
    let bar = [
        cell_of('║', margin_bar_style(palette)),
        cell_of(' ', Style::default()),
    ];
    let bullet_mark = [
        cell_of('▪', Style::default().fg(palette.accent)),
        cell_of(' ', Style::default().fg(palette.accent)),
    ];
    let quote_plain = [
        cell_of('>', Style::default().fg(palette.text)),
        cell_of(' ', Style::default().fg(palette.text)),
    ];
    let mut in_q_block = false;
    for block in blocks {
        if !matches!(block, MdBlock::Paragraph { .. }) {
            in_q_block = false;
        }
        match block {
            MdBlock::Blank => out.push(Line::raw("")),
            MdBlock::Heading(lvl, h) => {
                if let Some((_anchor, label)) = design_section_for_heading(&h) {
                    sections.push((label, out.len()));
                }
                if !plain && is_banner_heading(lvl, &h) {
                    out.push(banner_line(&h, content_w, palette));
                } else {
                    let st = Style::default()
                        .fg(palette.accent)
                        .add_modifier(Modifier::BOLD);
                    let cells: Vec<Cell> = h.chars().map(|c| cell_of(c, st)).collect();
                    emit_wrapped(&mut out, &cells, content_w, 0, &[]);
                }
            }
            MdBlock::Bullet(spans) => {
                let q = !plain && starts_with_q_marker(&spans);
                let (lead, cont) = if q {
                    (&bar[..], &bar[..])
                } else {
                    (&bullet_mark[..], &indent_2[..])
                };
                let mut cells = lead.to_vec();
                push_inline_cells(&spans, palette, &mut cells);
                emit_wrapped(&mut out, &cells, content_w, 2, cont);
            }
            MdBlock::Quote(spans) => {
                let (lead, indent, cont) = if plain {
                    (&quote_plain[..], 0usize, &[][..])
                } else {
                    (&bar[..], 2usize, &bar[..])
                };
                let mut cells = lead.to_vec();
                push_inline_cells(&spans, palette, &mut cells);
                emit_wrapped(&mut out, &cells, content_w, indent, cont);
            }
            MdBlock::Paragraph { indented, spans } => {
                if starts_with_q_marker(&spans) {
                    in_q_block = true;
                }
                let mut cells = Vec::new();
                if !plain && in_q_block {
                    cells.extend_from_slice(&bar);
                    if indented {
                        cells.extend_from_slice(&indent_2);
                    }
                    push_inline_cells(&spans, palette, &mut cells);
                    emit_wrapped(
                        &mut out,
                        &cells,
                        content_w,
                        if indented { 4 } else { 2 },
                        &bar,
                    );
                } else {
                    push_inline_cells(&spans, palette, &mut cells);
                    emit_wrapped(&mut out, &cells, content_w, 0, &[]);
                }
            }
            MdBlock::Code(code) => render_code_block(&code, content_w, palette, &mut out),
        }
    }
    (out, sections)
}

fn render_code_block(
    code: &str,
    content_w: usize,
    palette: &Palette,
    out: &mut Vec<Line<'static>>,
) {
    let box_bg = Style::default().fg(palette.text).bg(palette.surface1);
    let gutter = Style::default().fg(palette.accent).bg(palette.surface1);
    let code_w = content_w.saturating_sub(2).max(1);
    let cells: Vec<Cell> = code.chars().map(|c| cell_of(c, box_bg)).collect();
    for row in wrap_cells(&cells, code_w, 0, &[]) {
        let mut full = Vec::with_capacity(row.len() + 2);
        if content_w >= 2 {
            full.push(cell_of('▏', gutter));
            full.push(cell_of(' ', gutter));
        }
        let used: usize =
            full.iter().map(|c| c.w).sum::<usize>() + row.iter().map(|c| c.w).sum::<usize>();
        full.extend(row);
        for _ in used..content_w {
            full.push(cell_of(' ', box_bg));
        }
        out.push(to_line(&full));
    }
}

/// Render a `##` or `### §R<r>.*` heading as a centred chevron intertitle when
/// the full form fits `content_w`, or the narrower ladder form otherwise.
pub(super) fn banner_line(label: &str, content_w: usize, palette: &Palette) -> Line<'static> {
    let w = u16::try_from(content_w.max(1)).unwrap_or(u16::MAX);
    let label_w = display_width(label);
    let full_w = 22usize.saturating_add(label_w);
    let target_w = if full_w <= content_w {
        full_w as u16
    } else {
        w
    };
    let segs = intertitle(label, target_w, Flank::Chevron);
    let pad_left = content_w.saturating_sub(usize::from(target_w)) / 2;

    let bg = overlay_bg(palette);
    let gilt = palette.elevation().gilt_style(bg);
    let star = Style::default()
        .fg(palette.accent)
        .bg(bg)
        .add_modifier(Modifier::BOLD);
    let heading = Style::default()
        .fg(palette.accent)
        .add_modifier(Modifier::BOLD);

    let mut spans = Vec::with_capacity(segs.len() + 1);
    if pad_left > 0 {
        spans.push(Span::raw(" ".repeat(pad_left)));
    }
    for (text, part) in segs {
        if text.is_empty() {
            continue;
        }
        let style = match part {
            Part::Rule | Part::Clasp => gilt,
            Part::Star => star,
            Part::Label => heading,
        };
        spans.push(Span::styled(text, style));
    }
    Line::from(spans)
}

/// Whether inline spans open with `Q1`, `Q2`, or `Q3` (optionally bolded).
pub(super) fn starts_with_q_marker(spans: &[MdInline]) -> bool {
    let mut head = String::new();
    for sp in spans {
        let s = match sp {
            MdInline::Text(t) | MdInline::Bold(t) | MdInline::Code(t) | MdInline::Teal(t) => t,
        };
        for ch in s.chars() {
            if head.is_empty() && ch.is_whitespace() {
                continue;
            }
            head.push(ch);
            if head.len() >= 3 {
                break;
            }
        }
        if head.len() >= 3 {
            break;
        }
    }
    let Some(rest) = head
        .strip_prefix("Q1")
        .or_else(|| head.strip_prefix("Q2"))
        .or_else(|| head.strip_prefix("Q3"))
    else {
        return false;
    };
    !rest.starts_with(|c: char| c.is_ascii_alphanumeric())
}

fn margin_bar_style(palette: &Palette) -> Style {
    palette.elevation().gilt_style(overlay_bg(palette))
}
