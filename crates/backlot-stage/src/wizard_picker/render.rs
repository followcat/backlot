//! The picker's face: `draw` fills a ratatui `Buffer` from the
//! state machine's rows — title and input up top, the tree in the
//! scroll area, status and key hints at the bottom. It asks
//! [`Picker::repo`] once per visible row, so `git log` probes stay
//! per-visible-row and cached. Narrow widths first drop the meta
//! column, then truncate names with `…`; tiny areas just draw less —
//! nothing here panics.

use std::path::Path;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use unicode_width::UnicodeWidthStr;

use super::state::{Focus, Picker};
use super::tree::{Repo, Row, age_text};

/// backlot's palette — the same colors `hc-tui`'s `own_theme` gives
/// the shell, repeated here because the wizard runs inside the
/// popup's pty, a crate below the theme.
const ACCENT: Color = Color::Rgb(240, 178, 74);
const PANEL: Color = Color::Rgb(17, 20, 29);
const SEL_BG: Color = Color::Rgb(44, 53, 80);
const OVERLAY: Color = Color::Rgb(111, 120, 147);
const SUBTEXT: Color = Color::Rgb(185, 179, 164);
const TEXT: Color = Color::Rgb(236, 229, 211);

/// The layout: four lines on top (title, input, gap, root heading)
/// and two at the bottom (status, hints) frame the scroll area.
const HEAD: u16 = 4;
const TAIL: u16 = 2;

/// The meta column drops below this width.
const META_MIN_W: u16 = 50;
/// The meta column starts this far from the right edge.
const META_OFF: u16 = 26;

const HINTS: &str =
    " ↑↓ move   → open   ← close/up   Tab complete   Enter pick   Esc back to typing";
const LEGEND: &str = "git repository with a commit — Enter picks it";

/// The scroll area's height at `area` — the driver hands it to
/// [`Picker::set_page`] so PgUp/PgDn move by a page.
pub fn view_height(area: Rect) -> usize {
    area.height.saturating_sub(HEAD + TAIL) as usize
}

/// Draw the whole picker into `buf`; answer the terminal cursor's
/// cell — the input cursor while the input has focus, the selected
/// row's marker while the tree has it.
pub fn draw(picker: &mut Picker, area: Rect, buf: &mut Buffer) -> Option<(u16, u16)> {
    buf.set_style(area, Style::default().bg(PANEL).fg(TEXT));
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let (x, y, w, h) = (area.x, area.y, area.width, area.height);
    put(buf, x, y, w, " New studio", TEXT);
    put(
        buf,
        x + 11,
        y,
        w.saturating_sub(11),
        " · repository",
        SUBTEXT,
    );
    if h < 2 {
        return Some((x, y));
    }
    let cursor = input_line(picker, x, y + 1, w, buf);
    if h < 3 {
        return Some(cursor);
    }
    let root = format!("   {}", picker.root_display());
    put(buf, x, y + 3, w, &root, SUBTEXT);
    let vh = view_height(area);
    let sel = picker.sel();
    let scroll = sel.saturating_sub(vh.saturating_sub(1));
    let sel_line = tree_rows(picker, x, y + HEAD, w, vh, scroll, buf);
    if h >= 6 {
        status_line(picker, x, y + h - 2, w, buf);
        put(buf, x, y + h - 1, w, HINTS, OVERLAY);
    }
    match picker.focus() {
        Focus::Input => Some(cursor),
        Focus::Tree => Some(sel_line.map_or(cursor, |ly| (x + 1, ly))),
    }
}

/// ` path › {text}` with the cursor block; `Tab completes` sits at
/// the right edge while there is room. Answers the cursor's cell.
fn input_line(picker: &Picker, x: u16, y: u16, w: u16, buf: &mut Buffer) -> (u16, u16) {
    /// ` path › ` — its display width; `›` is multi-byte.
    const PRE_W: u16 = 8;
    put(buf, x, y, w, " path ", SUBTEXT);
    put(buf, x + 6, y, w.saturating_sub(6), "›", ACCENT);
    let text = picker.input();
    put(buf, x + PRE_W, y, w.saturating_sub(PRE_W), text, TEXT);
    let cx = x + PRE_W + UnicodeWidthStr::width(&text[..picker.cursor()]) as u16;
    if cx < x + w {
        put(buf, cx, y, 1, "▏", OVERLAY);
    }
    if w >= 40 {
        let hint = "Tab completes";
        put(
            buf,
            x + w - hint.len() as u16,
            y,
            hint.len() as u16,
            hint,
            OVERLAY,
        );
    }
    (cx.min(x + w - 1), y)
}

/// The scroll area: `Used` rows lead (the "repositories in use"
/// group), then the tree under the root, then `(no access)` or
/// `(no match)` when nothing is listed. Answers the selected row's
/// line when it is inside the viewport.
fn tree_rows(
    picker: &mut Picker,
    x: u16,
    y: u16,
    w: u16,
    vh: usize,
    scroll: usize,
    buf: &mut Buffer,
) -> Option<u16> {
    if vh == 0 {
        return None;
    }
    let rows: Vec<Row> = picker
        .rows()
        .iter()
        .skip(scroll)
        .take(vh)
        .cloned()
        .collect();
    let mut sel_line = None;
    for (i, row) in rows.iter().enumerate() {
        let ly = y + i as u16;
        let idx = scroll + i;
        if idx == picker.sel() {
            buf.set_style(Rect::new(x, ly, w, 1), Style::default().bg(SEL_BG));
            put(buf, x + 1, ly, 1, "▶", ACCENT);
            sel_line = Some(ly);
        }
        let at = Rect::new(x, ly, w, 1);
        match row {
            Row::Used { path } => {
                let shown = picker.display(path);
                row_line(picker, buf, at, &shown, path, 7);
                put(buf, x + 5, ly, 1, "●", ACCENT);
            }
            Row::Dir { path, lasts } => {
                let mut head = String::new();
                for &last in &lasts[..lasts.len().saturating_sub(1)] {
                    head.push_str(if last { "    " } else { "│   " });
                }
                if let Some(&last) = lasts.last() {
                    head.push_str(if last { "└ " } else { "├ " });
                }
                let marker = if picker.is_expanded(path) {
                    '▾'
                } else if picker.has_children(path) {
                    '▸'
                } else {
                    ' '
                };
                head.push(marker);
                let name_x = 3 + head.chars().count() as u16 + 1;
                put(buf, x + 3, ly, w.saturating_sub(3), &head, OVERLAY);
                row_line(picker, buf, at, &name_of(path), path, name_x)
            }
            Row::Cap { depth, left } => {
                let pad = "    ".repeat(*depth);
                put(
                    buf,
                    x + 3,
                    ly,
                    w.saturating_sub(3),
                    &format!("{pad}… {left} left out"),
                    OVERLAY,
                );
            }
        }
    }
    if rows.is_empty() {
        let msg = if picker.root_listed() {
            "   (no match)"
        } else {
            "   (no access)"
        };
        put(buf, x, y, w, msg, SUBTEXT);
    }
    sel_line
}

/// A directory row's name plus its repository meta — `●`/`○` then
/// `branch · age`/`no commits` — in a fixed column while the width
/// allows: the column drops first, the name truncates second.
fn row_line(picker: &mut Picker, buf: &mut Buffer, at: Rect, name: &str, path: &Path, name_x: u16) {
    let (x, ly, w) = (at.x, at.y, at.width);
    let meta = match picker.repo(path) {
        Repo::Commit { branch, age_s } => Some(format!("{branch} · {}", age_text(age_s))),
        Repo::Empty => Some("no commits".to_string()),
        Repo::None => None,
    };
    let meta_x = x + w.saturating_sub(META_OFF);
    let show_meta = w >= META_MIN_W && meta.is_some() && name_x + 4 < meta_x - x;
    let name_w = match show_meta {
        true => meta_x - x - name_x - 1,
        false => w.saturating_sub(name_x + 1),
    };
    let name = fit(name, name_w);
    put(buf, x + name_x, ly, name_w, &name, TEXT);
    if let Some(text) = meta.filter(|_| show_meta) {
        let (mark, mark_fg) = match picker.repo(path) {
            Repo::Commit { .. } => ("●", ACCENT),
            _ => ("○", SUBTEXT),
        };
        put(buf, meta_x, ly, 1, mark, mark_fg);
        put(
            buf,
            meta_x + 2,
            ly,
            w.saturating_sub(meta_x - x + 2),
            &text,
            SUBTEXT,
        );
    }
}

/// The status row: the picker's own message while there is one,
/// else the `●` legend.
fn status_line(picker: &Picker, x: u16, y: u16, w: u16, buf: &mut Buffer) {
    match picker.status() {
        Some(s) => put(buf, x, y, w, &format!(" {s}"), SUBTEXT),
        None => {
            put(buf, x + 1, y, w.saturating_sub(1), "●", ACCENT);
            put(buf, x + 3, y, w.saturating_sub(3), LEGEND, SUBTEXT);
        }
    }
}

/// A row's displayed name — a `Used` row spells its full path with
/// `~`, a tree row its directory name.
fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// `s` clipped to `w` display columns, ending with `…` when cut.
fn fit(s: &str, w: u16) -> String {
    let w = w as usize;
    if UnicodeWidthStr::width(s) <= w {
        return s.to_string();
    }
    if w == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for g in s.chars() {
        let gw = UnicodeWidthStr::width(g.to_string().as_str());
        if used + gw > w.saturating_sub(1) {
            break;
        }
        out.push(g);
        used += gw;
    }
    format!("{out}…")
}

fn put(buf: &mut Buffer, x: u16, y: u16, w: u16, s: &str, fg: Color) {
    let area = buf.area();
    if x >= area.right() || y >= area.bottom() {
        return;
    }
    buf.set_stringn(x, y, s, w as usize, Style::default().fg(fg));
}
