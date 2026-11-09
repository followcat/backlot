// Derived from herdr src/pane/terminal.rs `RetainedTextBuffer` and its
// `word_motion_target`/`paragraph_motion_target`/`search_text_window`
// entry points (Apache-2.0).
//! The retained-buffer engine behind copy-mode word/paragraph motions and
//! `/`/`?` search. Rows come from `Vt::screen_text_rows_range` — absolute
//! screen coordinates, per-cell wide tags, and the soft-wrap flags that
//! join physical rows into logical lines. The algorithms are the
//! reference's verbatim: atoms classify whitespace/separator/word, small
//! words split on tmux separators, big words split only on whitespace.

use hc_vt::{ScreenTextRow, Wide};

use crate::copy_mode::{CopySearchDirection, PaneTextPoint, PaneTextRange};

/// tmux's small-word separator set — the reference's
/// `COPY_MODE_WORD_SEPARATORS`.
const COPY_MODE_WORD_SEPARATORS: &str = "!\"#$%&'()*+,-./:;<=>?@[\\]^`{|}~";

/// Rows a word-motion scan window starts with; it doubles until the
/// answer stops depending on off-window context (or the buffer edge).
pub(crate) const WORD_MOTION_WINDOW: usize = 64;

/// Paragraph motions look at most this many rows away.
pub(crate) const PARAGRAPH_SCAN_LIMIT: usize = 1000;

/// The server's `MAX_RETURNED_MATCHES` cap on retained matches.
pub(crate) const COPY_SEARCH_RETAINED_MAX: usize = 1024;

/// The server's `MAX_QUERY_BYTES` cap.
pub(crate) const COPY_SEARCH_QUERY_MAX: usize = 4096;

/// The reference's `TerminalWordMotion`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WordMotion {
    NextStart,
    PreviousStart,
    NextEnd,
    NextBigStart,
    PreviousBigStart,
    NextBigEnd,
}

/// The reference's `TerminalSearchWindow`.
#[derive(Debug, Default)]
pub(crate) struct CopySearchWindow {
    pub matches: Vec<PaneTextRange>,
    pub current: Option<usize>,
    pub current_global: Option<usize>,
    pub total: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextClass {
    Whitespace,
    Separator,
    Word,
}

#[derive(Debug)]
struct TextAtom {
    point: Option<PaneTextPoint>,
    end_col: u16,
    class: TextClass,
}

#[derive(Debug)]
struct TextSpan {
    byte_start: usize,
    byte_end: usize,
    start: PaneTextPoint,
    end: PaneTextPoint,
}

#[derive(Debug, Default)]
struct LogicalTextLine {
    text: String,
    spans: Vec<TextSpan>,
}

#[derive(Debug)]
pub(crate) struct RetainedTextBuffer {
    lines: Vec<LogicalTextLine>,
    atoms: Vec<TextAtom>,
}

impl RetainedTextBuffer {
    #[cfg(test)]
    pub(crate) fn new(rows: Vec<ScreenTextRow>) -> Self {
        Self::build(rows, 0, true, true)
    }

    pub(crate) fn new_search(rows: Vec<ScreenTextRow>, row_offset: u32) -> Self {
        Self::build(rows, row_offset, true, false)
    }

    pub(crate) fn new_words(rows: Vec<ScreenTextRow>, row_offset: u32) -> Self {
        Self::build(rows, row_offset, false, true)
    }

    fn build(
        rows: Vec<ScreenTextRow>,
        row_offset: u32,
        build_lines: bool,
        build_atoms: bool,
    ) -> Self {
        let mut lines = Vec::new();
        let mut line = LogicalTextLine::default();
        let mut atoms: Vec<TextAtom> = Vec::new();

        for (row_idx, row) in rows.into_iter().enumerate() {
            let Some(row_idx) = u32::try_from(row_idx).ok() else {
                break;
            };
            let row_idx = row_offset.saturating_add(row_idx);
            for (col, cell) in row.cells.into_iter().enumerate() {
                let Ok(col) = u16::try_from(col) else {
                    break;
                };
                if cell.wide == Wide::SpacerTail {
                    continue;
                }
                if cell.wide == Wide::SpacerHead {
                    if build_atoms {
                        atoms.push(TextAtom {
                            point: Some(PaneTextPoint { row: row_idx, col }),
                            end_col: col,
                            class: atoms
                                .last()
                                .map_or(TextClass::Whitespace, |atom| atom.class),
                        });
                    }
                    continue;
                }
                let width = if cell.wide == Wide::Wide { 2 } else { 1 };
                let text = terminal_cell_text(&cell.graphemes);
                let start = PaneTextPoint { row: row_idx, col };
                let end = PaneTextPoint {
                    row: row_idx,
                    col: col.saturating_add(width - 1),
                };
                if build_lines {
                    let byte_start = line.text.len();
                    line.text.push_str(&text);
                    let byte_end = line.text.len();
                    line.spans.push(TextSpan {
                        byte_start,
                        byte_end,
                        start,
                        end,
                    });
                }
                if build_atoms {
                    atoms.push(TextAtom {
                        point: Some(start),
                        end_col: end.col,
                        class: text_class(&text),
                    });
                }
            }

            if row.soft_wrapped {
                continue;
            }

            if build_lines {
                let trimmed_len = line.text.trim_end().len();
                while line
                    .spans
                    .last()
                    .is_some_and(|span| span.byte_start >= trimmed_len)
                {
                    line.spans.pop();
                }
                line.text.truncate(trimmed_len);
                lines.push(std::mem::take(&mut line));
            }
            if build_atoms {
                atoms.push(TextAtom {
                    point: None,
                    end_col: 0,
                    class: TextClass::Whitespace,
                });
            }
        }

        if build_lines && (!line.text.is_empty() || !line.spans.is_empty()) {
            lines.push(line);
        }

        Self { lines, atoms }
    }

    pub(crate) fn search_window(
        &self,
        query: &str,
        case_sensitive: bool,
        direction: CopySearchDirection,
        cursor: PaneTextPoint,
        previous: Option<PaneTextRange>,
        limit: usize,
    ) -> CopySearchWindow {
        if query.is_empty() || limit == 0 {
            return CopySearchWindow::default();
        }
        let Ok(regex) = regex::RegexBuilder::new(&regex::escape(query))
            .case_insensitive(!case_sensitive)
            .build()
        else {
            return CopySearchWindow::default();
        };
        let to_match = |line: &LogicalTextLine, found: regex::Match<'_>| {
            let start_index = line
                .spans
                .binary_search_by_key(&found.start(), |span| span.byte_start)
                .ok()?;
            let end_index = line
                .spans
                .binary_search_by_key(&found.end(), |span| span.byte_end)
                .ok()?;
            let start_span = &line.spans[start_index];
            let end_span = &line.spans[end_index];
            Some(PaneTextRange {
                start: start_span.start,
                end: end_span.end,
            })
        };

        let origin = match direction {
            CopySearchDirection::Forward => previous.map_or(cursor, |range| range.end),
            CopySearchDirection::Backward => previous.map_or(cursor, |range| range.start),
        };
        let mut total = 0usize;
        let mut target = None;
        for line in &self.lines {
            for found in regex.find_iter(&line.text) {
                let Some(text_match) = to_match(line, found) else {
                    continue;
                };
                match direction {
                    CopySearchDirection::Forward
                        if target.is_none() && text_match.start > origin =>
                    {
                        target = Some(total);
                    }
                    CopySearchDirection::Backward if text_match.end < origin => {
                        target = Some(total);
                    }
                    _ => {}
                }
                total = total.saturating_add(1);
            }
        }
        if total == 0 {
            return CopySearchWindow::default();
        }
        let target = target.unwrap_or(match direction {
            CopySearchDirection::Forward => 0,
            CopySearchDirection::Backward => total - 1,
        });
        let retained = limit.min(total);
        let start = target
            .saturating_sub(retained / 2)
            .min(total.saturating_sub(retained));
        let end = start.saturating_add(retained);
        let mut index = 0usize;
        let mut matches = Vec::with_capacity(retained);
        for line in &self.lines {
            for found in regex.find_iter(&line.text) {
                let Some(text_match) = to_match(line, found) else {
                    continue;
                };
                if index >= start && index < end {
                    matches.push(text_match);
                }
                index = index.saturating_add(1);
                if index >= end {
                    break;
                }
            }
            if index >= end {
                break;
            }
        }
        CopySearchWindow {
            matches,
            current: Some(target - start),
            current_global: Some(target),
            total,
        }
    }

    pub(crate) fn word_motion(
        &self,
        row: u32,
        col: u16,
        motion: WordMotion,
    ) -> Option<PaneTextPoint> {
        let current = self.atoms.iter().position(|atom| {
            atom.point
                .is_some_and(|point| point.row == row && col >= point.col && col <= atom.end_col)
        })?;
        match motion {
            WordMotion::NextStart => self.next_word_start(current),
            WordMotion::PreviousStart => self.previous_word_start(current),
            WordMotion::NextEnd => self.next_word_end(current),
            WordMotion::NextBigStart => self.next_big_word_start(current),
            WordMotion::PreviousBigStart => self.previous_big_word_start(current),
            WordMotion::NextBigEnd => self.next_big_word_end(current),
        }
    }

    fn next_word_start(&self, current: usize) -> Option<PaneTextPoint> {
        let current_class = self.atoms.get(current)?.class;
        let mut next = current.saturating_add(1);
        if current_class != TextClass::Whitespace {
            while self
                .atoms
                .get(next)
                .is_some_and(|atom| atom.class == current_class)
            {
                next += 1;
            }
        }
        while self
            .atoms
            .get(next)
            .is_some_and(|atom| atom.class == TextClass::Whitespace)
        {
            next += 1;
        }
        self.next_point(next)
    }

    fn previous_word_start(&self, current: usize) -> Option<PaneTextPoint> {
        let mut previous = current.checked_sub(1)?;
        while self
            .atoms
            .get(previous)
            .is_some_and(|atom| atom.class == TextClass::Whitespace)
        {
            previous = previous.checked_sub(1)?;
        }
        let class = self.atoms.get(previous)?.class;
        while previous > 0
            && self
                .atoms
                .get(previous - 1)
                .is_some_and(|atom| atom.class == class)
        {
            previous -= 1;
        }
        self.previous_point(previous)
    }

    fn next_word_end(&self, current: usize) -> Option<PaneTextPoint> {
        let mut next = current.saturating_add(1);
        while self
            .atoms
            .get(next)
            .is_some_and(|atom| atom.class == TextClass::Whitespace)
        {
            next += 1;
        }
        let class = self.atoms.get(next)?.class;
        while self
            .atoms
            .get(next + 1)
            .is_some_and(|atom| atom.class == class)
        {
            next += 1;
        }
        self.previous_point(next)
    }

    fn next_big_word_start(&self, current: usize) -> Option<PaneTextPoint> {
        let mut next = current.saturating_add(1);
        if self
            .atoms
            .get(current)
            .is_some_and(|atom| atom.class != TextClass::Whitespace)
        {
            while self
                .atoms
                .get(next)
                .is_some_and(|atom| atom.class != TextClass::Whitespace)
            {
                next += 1;
            }
        }
        while self
            .atoms
            .get(next)
            .is_some_and(|atom| atom.class == TextClass::Whitespace)
        {
            next += 1;
        }
        self.next_point(next)
    }

    fn previous_big_word_start(&self, current: usize) -> Option<PaneTextPoint> {
        let mut previous = current.checked_sub(1)?;
        while self
            .atoms
            .get(previous)
            .is_some_and(|atom| atom.class == TextClass::Whitespace)
        {
            previous = previous.checked_sub(1)?;
        }
        while previous > 0
            && self
                .atoms
                .get(previous - 1)
                .is_some_and(|atom| atom.class != TextClass::Whitespace)
        {
            previous -= 1;
        }
        self.previous_point(previous)
    }

    fn next_big_word_end(&self, current: usize) -> Option<PaneTextPoint> {
        let mut next = current.saturating_add(1);
        while self
            .atoms
            .get(next)
            .is_some_and(|atom| atom.class == TextClass::Whitespace)
        {
            next += 1;
        }
        self.atoms.get(next)?;
        while self
            .atoms
            .get(next + 1)
            .is_some_and(|atom| atom.class != TextClass::Whitespace)
        {
            next += 1;
        }
        self.previous_point(next)
    }

    fn next_point(&self, mut index: usize) -> Option<PaneTextPoint> {
        while let Some(atom) = self.atoms.get(index) {
            if let Some(point) = atom.point {
                return Some(point);
            }
            index += 1;
        }
        None
    }

    fn previous_point(&self, mut index: usize) -> Option<PaneTextPoint> {
        loop {
            if let Some(point) = self.atoms.get(index)?.point {
                return Some(point);
            }
            index = index.checked_sub(1)?;
        }
    }

    pub(crate) fn point_is_final_atom(&self, point: PaneTextPoint) -> bool {
        // Word motion targets are atom start points, so compare against the
        // final atom's start point. Comparing against `end_col` would never
        // match a wide glyph, whose end column is one past its start.
        self.atoms
            .iter()
            .rev()
            .find(|atom| atom.point.is_some())
            .is_some_and(|atom| atom.point == Some(point))
    }
}

pub(crate) fn terminal_cell_text(graphemes: &[u32]) -> String {
    if graphemes.is_empty() || graphemes.first().copied() == Some(hc_vt::KITTY_UNICODE_PLACEHOLDER)
    {
        return " ".to_string();
    }
    graphemes
        .iter()
        .map(|codepoint| char::from_u32(*codepoint).unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

fn text_class(text: &str) -> TextClass {
    let Some(ch) = text.chars().next() else {
        return TextClass::Whitespace;
    };
    if ch.is_whitespace() {
        TextClass::Whitespace
    } else if ch.is_ascii() && COPY_MODE_WORD_SEPARATORS.contains(ch) {
        TextClass::Separator
    } else {
        TextClass::Word
    }
}
