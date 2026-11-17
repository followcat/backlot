// Derived from herdr src/ui/release_notes.rs (Apache-2.0): the
// release-note line styles (heading, bullet, inline code, fenced block)
// and the install callout.
//! The release-notes body of the update modal: the Markdown subset as
//! styled, width-wrapped lines, the multi-release stream with its
//! dividers, and the install callout above it.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

use super::palette::Palette;
use crate::update_markdown::{Block, Inline, parse};
use crate::update_state::CachedRelease;

/// Where the notes for the latest tag stand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NotesState {
    Loaded,
    Loading,
    Offline,
}

/// The top-of-body install callout, shown while the update is offered.
pub(crate) struct Callout<'a> {
    pub writable: bool,
    pub install_dir: &'a str,
    /// The manual command shown as the chip (`hc-tui --upgrade`, or
    /// the `HC_TUI_INSTALL_DIR=… hc-tui --upgrade` form).
    pub chip: &'a str,
}

/// Everything the notes body is built from.
pub(crate) struct Stream<'a> {
    pub releases: &'a [CachedRelease],
    pub installed: &'a str,
    pub latest: &'a str,
    pub notes: NotesState,
    /// `Some` while the update is still offered (not in what's-new).
    pub callout: Option<Callout<'a>>,
}

pub(crate) type Cell = (char, Style);

const RELEASE_URL: &str = "https://github.com/followcat/hc-tui/releases/tag";

/// Every release body as styled lines wrapped to `width` cells.
pub(crate) fn stream_lines(stream: &Stream<'_>, width: u16, p: &Palette) -> Vec<Line<'static>> {
    let width = usize::from(width).max(4);
    let mut out = Vec::new();
    if let Some(callout) = &stream.callout {
        out.extend(callout_lines(callout, p));
        out.push(Line::raw(""));
    }
    match stream.notes {
        NotesState::Loaded => {}
        NotesState::Loading => {
            out.push(line(&[
                (" ".into(), text(p)),
                ("◐".into(), warn(p)),
                (" loading release notes from GitHub…".into(), overlay1(p)),
            ]));
            out.push(Line::raw(""));
        }
        NotesState::Offline => {
            out.push(line(&[(
                " release notes unavailable offline".into(),
                overlay1(p),
            )]));
            out.push(line(&[(
                format!(" {RELEASE_URL}/v{}", stream.latest),
                Style::default().fg(p.subtext0),
            )]));
            out.push(Line::raw(""));
        }
    }
    for (index, release) in stream.releases.iter().enumerate() {
        if index > 0 {
            out.push(Line::raw(""));
            out.push(line(&[(
                format!(" {}", "─".repeat(width.saturating_sub(2))),
                Style::default().fg(p.surface0),
            )]));
            out.push(Line::raw(""));
        }
        out.push(subheader(release, p));
        for block in parse(&release.body) {
            block_lines(&block, p, width, &mut out);
        }
    }
    if !stream.releases.is_empty() {
        let label = format!(" ── installed v{} ", stream.installed);
        let fill = width
            .saturating_sub(2)
            .saturating_sub(label.chars().count());
        out.push(Line::raw(""));
        out.push(line(&[(
            format!("{label}{}", "─".repeat(fill)),
            overlay0(p),
        )]));
    }
    out
}

/// The two-line install callout plus its trailing blank row.
fn callout_lines(callout: &Callout<'_>, p: &Palette) -> Vec<Line<'static>> {
    let copy = Style::default().fg(p.text).add_modifier(Modifier::BOLD);
    let chip = chip_style(p);
    if callout.writable {
        vec![
            line(&[
                (" ".into(), text(p)),
                ("●".into(), accent_bold(p)),
                (" updates this client · programs keep running".into(), copy),
            ]),
            line(&[
                (" press enter to install, or run ".into(), text(p)),
                (callout.chip.into(), chip),
            ]),
        ]
    } else {
        vec![
            line(&[
                (" ".into(), text(p)),
                ("●".into(), warn_bold(p)),
                (
                    format!(
                        " cannot update in place: {} is not writable",
                        callout.install_dir
                    ),
                    copy,
                ),
            ]),
            line(&[
                (" upgrade from a shell: ".into(), text(p)),
                (callout.chip.into(), chip),
            ]),
        ]
    }
}

fn subheader(release: &CachedRelease, p: &Palette) -> Line<'static> {
    let mut spans = vec![(
        format!(" v{}", release.version),
        Style::default().fg(p.text).add_modifier(Modifier::BOLD),
    )];
    if !release.date.is_empty() {
        spans.push((format!(" · {}", release.date), overlay0(p)));
    }
    line(&spans)
}

fn block_lines(block: &Block, p: &Palette, width: usize, out: &mut Vec<Line<'static>>) {
    match block {
        Block::Blank => out.push(Line::raw("")),
        Block::Heading(title) => {
            let cells = styled(&format!(" {}", title.to_uppercase()), accent_bold(p));
            push_wrapped(&cells, width, 1, out);
        }
        Block::Bullet(spans) => {
            let mut cells = styled(" • ", Style::default().fg(p.accent));
            inline_cells(spans, p, &mut cells);
            push_wrapped(&cells, width, 3, out);
        }
        Block::Paragraph(spans) => {
            let mut cells = styled(" ", text(p));
            inline_cells(spans, p, &mut cells);
            push_wrapped(&cells, width, 1, out);
        }
        Block::Code(code) => out.extend(code_rows(
            code,
            Style::default().fg(p.text).bg(p.surface1),
            width,
            p,
        )),
    }
}

/// Fenced-code rows: the `▏` gutter, then the text wrapped to the body
/// width, every row filled to that width on `surface1`.
pub(crate) fn code_rows(code: &str, style: Style, width: usize, p: &Palette) -> Vec<Line<'static>> {
    let box_bg = Style::default().bg(p.surface1);
    let gutter = Style::default().fg(p.accent).bg(p.surface1);
    let width = width.max(4);
    wrap_rows(&styled(code, style), width - 2, 2)
        .into_iter()
        .map(|row| {
            let mut cells = styled("▏", gutter);
            cells.extend(styled(" ", box_bg));
            cells.extend(row);
            while cells.len() < width {
                cells.push((' ', box_bg));
            }
            to_line(&cells)
        })
        .collect()
}

fn inline_cells(spans: &[Inline], p: &Palette, out: &mut Vec<Cell>) {
    for span in spans {
        match span {
            Inline::Text(t) => out.extend(styled(t, text(p))),
            Inline::Bold(t) => out.extend(styled(t, text(p).add_modifier(Modifier::BOLD))),
            Inline::Code(t) => out.extend(styled(
                t,
                Style::default()
                    .fg(p.accent)
                    .bg(p.surface0)
                    .add_modifier(Modifier::BOLD),
            )),
        }
    }
}

pub(crate) fn styled(text: &str, style: Style) -> Vec<Cell> {
    text.chars().map(|c| (c, style)).collect()
}

/// Greedy word wrap by display cells. A continuation row starts with
/// `indent` spaces (the 3-cell hang under a bullet). A word longer than
/// the row breaks where it must.
pub(crate) fn wrap_rows(cells: &[Cell], width: usize, indent: usize) -> Vec<Vec<Cell>> {
    let mut rows: Vec<Vec<Cell>> = Vec::new();
    let mut row: Vec<Cell> = Vec::new();
    let mut used = 0usize;
    let mut space: Option<usize> = None;
    for &cell in cells {
        let w = cell.0.width().unwrap_or(0);
        if used + w > width && !row.is_empty() {
            let rest = match space.take() {
                Some(at) => {
                    let mut tail = row.split_off(at);
                    tail.remove(0);
                    tail
                }
                None => Vec::new(),
            };
            rows.push(std::mem::take(&mut row));
            row.extend(std::iter::repeat_n((' ', Style::default()), indent));
            used = indent;
            for rest_cell in rest {
                used += rest_cell.0.width().unwrap_or(0);
                row.push(rest_cell);
            }
        }
        if cell.0 == ' ' {
            space = Some(row.len());
        }
        used += w;
        row.push(cell);
    }
    rows.push(row);
    rows
}

fn push_wrapped(cells: &[Cell], width: usize, indent: usize, out: &mut Vec<Line<'static>>) {
    out.extend(
        wrap_rows(cells, width, indent)
            .iter()
            .map(|row| to_line(row)),
    );
}

/// Merges runs of equal style into spans.
pub(crate) fn to_line(cells: &[Cell]) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut run = String::new();
    let mut current: Option<Style> = None;
    for &(c, style) in cells {
        if current != Some(style) {
            if let Some(prev) = current {
                spans.push(Span::styled(std::mem::take(&mut run), prev));
            }
            current = Some(style);
        }
        run.push(c);
    }
    if let Some(style) = current {
        spans.push(Span::styled(run, style));
    }
    Line::from(spans)
}

pub(crate) fn line(parts: &[(String, Style)]) -> Line<'static> {
    let cells: Vec<Cell> = parts
        .iter()
        .flat_map(|(t, style)| styled(t, *style))
        .collect();
    to_line(&cells)
}

pub(crate) fn text(p: &Palette) -> Style {
    Style::default().fg(p.text)
}

pub(crate) fn overlay0(p: &Palette) -> Style {
    Style::default().fg(p.overlay0)
}

pub(crate) fn overlay1(p: &Palette) -> Style {
    Style::default().fg(p.overlay1)
}

pub(crate) fn accent_bold(p: &Palette) -> Style {
    Style::default().fg(p.accent).add_modifier(Modifier::BOLD)
}

pub(crate) fn warn(p: &Palette) -> Style {
    Style::default().fg(p.yellow)
}

pub(crate) fn warn_bold(p: &Palette) -> Style {
    warn(p).add_modifier(Modifier::BOLD)
}

/// The inline-code chip style used for the manual commands.
pub(crate) fn chip_style(p: &Palette) -> Style {
    Style::default()
        .fg(p.accent)
        .bg(p.surface0)
        .add_modifier(Modifier::BOLD)
}

#[cfg(test)]
#[path = "release_notes_tests.rs"]
mod tests;
