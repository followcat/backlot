//! Width-accurate CJK/Markdown/log wrapping for the Lot Reader (`spec.md` §2).

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::reader_md::{MdBlock, MdInline, inline_width, normalize_line, parse_markdown};
use crate::chrome::palette::Palette;

/// Body rendering mode chosen by entry kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum BodyMode {
    #[default]
    Markdown,
    PlainLog,
}

/// Infer [`BodyMode`] from a Reader title/path before explicit metadata lands.
pub(crate) fn body_mode_for_title(title: &str) -> BodyMode {
    let lower = title.trim();
    if lower.starts_with("records")
        || (lower.contains("checks/") && lower.ends_with(".log"))
        || lower.ends_with(".log")
        || (lower.contains("attention/") && lower.ends_with(".txt"))
    {
        BodyMode::PlainLog
    } else {
        BodyMode::Markdown
    }
}

#[derive(Debug, Clone, Copy)]
struct Cell {
    ch: char,
    w: usize,
    style: Style,
}

/// Unwrapped natural display width of the body for adaptive sizing.
pub(crate) fn natural_body_width(lines: &[String], mode: BodyMode) -> usize {
    match mode {
        BodyMode::PlainLog => lines
            .iter()
            .map(|l| UnicodeWidthStr::width(normalize_line(l).as_str()))
            .max()
            .unwrap_or(0),
        BodyMode::Markdown => parse_markdown(lines)
            .into_iter()
            .map(|block| match block {
                MdBlock::Blank => 0,
                MdBlock::Heading(h) => UnicodeWidthStr::width(h.as_str()),
                MdBlock::Bullet(spans) => 2 + inline_width(&spans),
                MdBlock::Paragraph(spans) => inline_width(&spans),
                MdBlock::Code(c) => 2 + UnicodeWidthStr::width(c.as_str()),
            })
            .max()
            .unwrap_or(0),
    }
}

/// Wrap `lines` at `content_w` cells using `mode` and theme `palette`.
pub(crate) fn render_body_lines(
    lines: &[String],
    mode: BodyMode,
    content_w: usize,
    palette: &Palette,
) -> Vec<Line<'static>> {
    let content_w = content_w.max(1);
    match mode {
        BodyMode::PlainLog => render_plain_log(lines, content_w, palette),
        BodyMode::Markdown => render_markdown(lines, content_w, palette),
    }
}

fn render_plain_log(lines: &[String], content_w: usize, palette: &Palette) -> Vec<Line<'static>> {
    if lines.is_empty() {
        return Vec::new();
    }
    let cont = vec![
        cell_of('↪', Style::default().fg(palette.overlay0)),
        cell_of(' ', Style::default().fg(palette.overlay0)),
    ];
    let mut out = Vec::new();
    for raw in lines {
        let norm = normalize_line(raw);
        let trimmed = norm.trim_start();
        let fg = if trimmed.starts_with("error")
            || trimmed.starts_with("FAILED")
            || trimmed.starts_with("exit ")
        {
            palette.red
        } else if trimmed.starts_with("test result: ok") || trimmed.starts_with("pass") {
            palette.green
        } else {
            palette.text
        };
        let style = Style::default().fg(fg);
        let cells: Vec<Cell> = norm.chars().map(|ch| cell_of(ch, style)).collect();
        for row in wrap_cells(&cells, content_w, 0, &cont) {
            out.push(to_line(&row));
        }
    }
    out
}

fn render_markdown(lines: &[String], content_w: usize, palette: &Palette) -> Vec<Line<'static>> {
    let blocks = parse_markdown(lines);
    let mut out = Vec::new();
    let indent_2 = [
        cell_of(' ', Style::default()),
        cell_of(' ', Style::default()),
    ];
    for block in blocks {
        match block {
            MdBlock::Blank => out.push(Line::raw("")),
            MdBlock::Heading(h) => {
                let style = Style::default()
                    .fg(palette.accent)
                    .add_modifier(Modifier::BOLD);
                let cells: Vec<Cell> = h.chars().map(|c| cell_of(c, style)).collect();
                for row in wrap_cells(&cells, content_w, 0, &[]) {
                    out.push(to_line(&row));
                }
            }
            MdBlock::Bullet(spans) => {
                let bullet_style = Style::default().fg(palette.accent);
                let mut cells = vec![cell_of('▪', bullet_style), cell_of(' ', bullet_style)];
                push_inline_cells(&spans, palette, &mut cells);
                for row in wrap_cells(&cells, content_w, 2, &indent_2) {
                    out.push(to_line(&row));
                }
            }
            MdBlock::Paragraph(spans) => {
                let mut cells = Vec::new();
                push_inline_cells(&spans, palette, &mut cells);
                for row in wrap_cells(&cells, content_w, 0, &[]) {
                    out.push(to_line(&row));
                }
            }
            MdBlock::Code(code) => {
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
                    let used: usize = full.iter().map(|c| c.w).sum::<usize>()
                        + row.iter().map(|c| c.w).sum::<usize>();
                    full.extend(row);
                    for _ in used..content_w {
                        full.push(cell_of(' ', box_bg));
                    }
                    out.push(to_line(&full));
                }
            }
        }
    }
    out
}

fn push_inline_cells(spans: &[MdInline], palette: &Palette, out: &mut Vec<Cell>) {
    for span in spans {
        let (text, style) = match span {
            MdInline::Text(t) => (t.as_str(), Style::default().fg(palette.text)),
            MdInline::Bold(t) => (
                t.as_str(),
                Style::default()
                    .fg(palette.text)
                    .add_modifier(Modifier::BOLD),
            ),
            MdInline::Code(t) => (
                t.as_str(),
                Style::default().fg(palette.accent).bg(palette.surface0),
            ),
            MdInline::Teal(t) => (t.as_str(), Style::default().fg(palette.teal)),
        };
        out.extend(text.chars().map(|c| cell_of(c, style)));
    }
}

fn cell_of(ch: char, style: Style) -> Cell {
    Cell {
        ch,
        w: UnicodeWidthChar::width(ch).unwrap_or(0),
        style,
    }
}

fn is_closing_punct(c: char) -> bool {
    matches!(
        c,
        '，' | '。'
            | '：'
            | '；'
            | '！'
            | '？'
            | '、'
            | '）'
            | '》'
            | '」'
            | '』'
            | '】'
            | '.'
            | ','
            | ':'
            | ';'
            | '!'
            | '?'
            | ')'
            | ']'
            | '}'
    )
}

fn is_opening_punct(c: char) -> bool {
    matches!(c, '（' | '《' | '「' | '『' | '【')
}

fn is_path_sep(c: char) -> bool {
    matches!(c, '/' | '-' | '_' | '.' | ':')
}

fn wrap_cells(
    cells: &[Cell],
    content_w: usize,
    prefix_len: usize,
    cont_prefix: &[Cell],
) -> Vec<Vec<Cell>> {
    if cells.is_empty() {
        return vec![Vec::new()];
    }
    let cont_w: usize = cont_prefix.iter().map(|c| c.w).sum();
    let effective_cont: &[Cell] = if cont_w < content_w { cont_prefix } else { &[] };
    let effective_cont_w: usize = effective_cont.iter().map(|c| c.w).sum();
    let cont_avail = content_w.saturating_sub(effective_cont_w);

    let mut rows: Vec<Vec<Cell>> = Vec::new();
    let mut row: Vec<Cell> = Vec::new();
    let mut used = 0usize;
    let mut min_split = prefix_len.min(cells.len());
    let mut pos = 0usize;

    while pos < cells.len() {
        let cell = cells[pos];
        if used + cell.w <= content_w || row.len() <= min_split {
            row.push(cell);
            used += cell.w;
            pos += 1;
            continue;
        }
        if cell.ch == ' ' {
            rows.push(std::mem::take(&mut row));
            row.extend_from_slice(effective_cont);
            used = effective_cont_w;
            min_split = effective_cont.len();
            pos += 1;
            continue;
        }
        let last_space = (min_split..row.len()).rfind(|&i| row[i].ch == ' ');
        let tok_start = last_space.map_or(min_split, |s| s + 1);
        let tok_w: usize = row[tok_start..].iter().map(|c| c.w).sum::<usize>()
            + cells[pos..]
                .iter()
                .take_while(|c| c.ch != ' ')
                .map(|c| c.w)
                .sum::<usize>();
        let (split_at, consume_space) = find_break(
            &row,
            cell,
            min_split,
            tok_start,
            last_space,
            tok_w <= cont_avail,
        );
        let rewind = if consume_space {
            let k = row.len() - (split_at + 1);
            row.truncate(split_at);
            k
        } else {
            let k = row.len() - split_at;
            row.truncate(split_at);
            k
        };
        rows.push(std::mem::take(&mut row));
        row.extend_from_slice(effective_cont);
        used = effective_cont_w;
        min_split = effective_cont.len();
        pos -= rewind;
    }
    rows.push(row);
    rows
}

fn find_break(
    row: &[Cell],
    incoming: Cell,
    min_split: usize,
    tok_start: usize,
    last_space: Option<usize>,
    tok_fits_next: bool,
) -> (usize, bool) {
    let tok_low = tok_start.max(min_split + 1);
    for i in (tok_low..=row.len()).rev() {
        let prev = row[i - 1];
        let next = if i == row.len() { incoming } else { row[i] };
        if !is_closing_punct(next.ch) && !is_opening_punct(prev.ch) && (prev.w == 2 || next.w == 2)
        {
            return (i, false);
        }
    }
    if let Some(sp) = last_space
        && sp >= min_split
        && tok_fits_next
    {
        let next = if sp + 1 >= row.len() {
            incoming.ch
        } else {
            row[sp + 1].ch
        };
        if !is_closing_punct(next) {
            return (sp, true);
        }
    }
    for i in (tok_low..=row.len()).rev() {
        let prev = row[i - 1].ch;
        let next = if i == row.len() {
            incoming.ch
        } else {
            row[i].ch
        };
        if is_path_sep(prev) && !is_closing_punct(next) {
            return (i, false);
        }
    }
    if let Some(sp) = last_space
        && sp >= min_split
    {
        let next = if sp + 1 >= row.len() {
            incoming.ch
        } else {
            row[sp + 1].ch
        };
        if !is_closing_punct(next) {
            return (sp, true);
        }
    }
    let mut split_at = row.len();
    while split_at > min_split + 1 {
        let next = if split_at == row.len() {
            incoming.ch
        } else {
            row[split_at].ch
        };
        let prev = row[split_at - 1].ch;
        if (is_closing_punct(next) && prev != ' ') || is_opening_punct(prev) {
            split_at -= 1;
            continue;
        }
        break;
    }
    (split_at, false)
}

fn to_line(cells: &[Cell]) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut run = String::new();
    let mut current: Option<Style> = None;
    for &cell in cells {
        if current != Some(cell.style) {
            if let Some(prev) = current {
                spans.push(Span::styled(std::mem::take(&mut run), prev));
            }
            current = Some(cell.style);
        }
        run.push(cell.ch);
    }
    if let Some(style) = current {
        spans.push(Span::styled(run, style));
    }
    Line::from(spans)
}
