//! Role badges, unified inline status vocabulary, and the shared 1-line
//! role row renderer used identically by both the left Cast and right Lot.

use backlot_model::event::age_short;
use backlot_model::summary::{IDLE_SHOWN_MS, RoleSummary};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::chrome::palette::Palette;
use crate::chrome::render::{put_role_badged_text, put_text};
use crate::chrome::text::{display_width, truncate_end};

use super::glyphs::{Person, PriorityMark, contrast_fg};

/// A role's one-cell circled badge. The glyph is the same everywhere
/// the TUI labels a role; the colour is the role's fixed theme token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RoleBadge {
    Producer,
    Director,
    Writer,
    Actor,
}

impl RoleBadge {
    /// The ledger role key's badge — `producer`, `director`, `writer`
    /// and anything under `actor`.
    pub(crate) fn of_role(role: &str) -> Option<RoleBadge> {
        match role {
            "producer" => Some(RoleBadge::Producer),
            "director" => Some(RoleBadge::Director),
            "writer" => Some(RoleBadge::Writer),
            _ if role.starts_with("actor") => Some(RoleBadge::Actor),
            _ => None,
        }
    }

    /// The badge a glyph stands for — the drawing helper reads a label
    /// back to the role it colours.
    pub(crate) fn of_glyph(glyph: char) -> Option<RoleBadge> {
        match glyph {
            'Ⓟ' => Some(RoleBadge::Producer),
            'Ⓓ' => Some(RoleBadge::Director),
            'Ⓦ' => Some(RoleBadge::Writer),
            'Ⓐ' => Some(RoleBadge::Actor),
            _ => None,
        }
    }

    pub(crate) fn glyph(self) -> &'static str {
        match self {
            RoleBadge::Producer => "Ⓟ",
            RoleBadge::Director => "Ⓓ",
            RoleBadge::Writer => "Ⓦ",
            RoleBadge::Actor => "Ⓐ",
        }
    }

    /// The role's fixed theme token: producer `mauve`, director `blue`,
    /// writer `teal`, actor `peach`.
    pub(crate) fn color(self, palette: &Palette) -> Color {
        match self {
            RoleBadge::Producer => palette.mauve,
            RoleBadge::Director => palette.blue,
            RoleBadge::Writer => palette.teal,
            RoleBadge::Actor => palette.peach,
        }
    }
}

/// A role's compact label: `Ⓟ`/`Ⓓ`/`Ⓦ` for fixed roles, `Ⓐ <task>`
/// (or `Ⓐ <name>` when uncast/closed without task) for actors, and the
/// role's own name otherwise.
pub(crate) fn role_label(role: &RoleSummary) -> String {
    let Some(badge) = RoleBadge::of_role(&role.role) else {
        return role.name.clone();
    };
    match (badge, role.task.as_deref()) {
        (RoleBadge::Actor, Some(task)) => format!("{} {task}", badge.glyph()),
        (RoleBadge::Actor, None) => format!("{} {}", badge.glyph(), role.name),
        _ => badge.glyph().to_string(),
    }
}

/// The single resolved inline status for any role row: highest-priority
/// semantic mark, unified state word, optional elapsed age, and optional
/// one-line detail tail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InlineState {
    pub(crate) mark: Person,
    pub(crate) word: &'static str,
    pub(crate) age: Option<String>,
    pub(crate) detail: Option<String>,
}

fn clean_single_line(raw: Option<&str>) -> Option<String> {
    let text = raw?.trim();
    if text.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut last_space = false;
    for ch in text.chars() {
        if matches!(ch, '\r' | '\n' | '\t') {
            if !last_space && !out.is_empty() {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(ch);
            last_space = ch == ' ';
        }
    }
    let trimmed = out.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn elapsed_age(since_ms: u64, now_ms: u64) -> Option<String> {
    (since_ms > 0).then(|| age_short(now_ms.saturating_sub(since_ms)))
}

impl Person {
    /// Single authoritative inline state resolver shared across Cast and Lot.
    pub(crate) fn inline_state(role: &RoleSummary, now_ms: u64) -> InlineState {
        let mark = Person::of(role);
        let attention = clean_single_line(
            role.attention_detail
                .as_deref()
                .or(role.attention.as_deref()),
        );
        let (age, detail) = match mark {
            Person::UserPrompt | Person::NeedsYou | Person::Stuck => {
                (elapsed_age(role.since_ms, now_ms), attention)
            }
            Person::Failed => (
                elapsed_age(role.since_ms, now_ms),
                clean_single_line(role.failed.as_deref()),
            ),
            Person::Lost => (
                elapsed_age(role.since_ms, now_ms),
                attention.or_else(|| Some("terminal gone".into())),
            ),
            Person::Asks => (
                elapsed_age(role.since_ms, now_ms),
                clean_single_line(role.asks.as_deref()),
            ),
            Person::DirPrompt => (
                elapsed_age(role.since_ms, now_ms),
                attention.or_else(|| Some("director reviewing".into())),
            ),
            Person::Working => (
                elapsed_age(role.since_ms, now_ms),
                clean_single_line(
                    role.progress
                        .as_ref()
                        .map(|p| p.text.as_str())
                        .or_else(|| role.running.as_ref().map(|r| r.cmd.as_str()))
                        .or_else(|| role.doing.as_deref().filter(|d| *d != "idle")),
                ),
            ),
            Person::Slot => (
                elapsed_age(role.since_ms, now_ms),
                clean_single_line(
                    role.doing
                        .as_deref()
                        .filter(|d| !d.is_empty() && *d != "waiting"),
                )
                .or_else(|| Some("waiting for a slot".into())),
            ),
            Person::Idle | Person::Ready => {
                let age = role
                    .idle_since_ms
                    .filter(|&since| since > 0 && now_ms.saturating_sub(since) >= IDLE_SHOWN_MS)
                    .map(|since| age_short(now_ms - since));
                let detail = clean_single_line(
                    role.doing
                        .as_deref()
                        .filter(|d| !d.is_empty() && *d != "idle"),
                );
                (age, detail)
            }
            Person::Standby | Person::Resumable | Person::Closed => (None, None),
        };
        InlineState {
            mark,
            word: mark.word(),
            age,
            detail,
        }
    }
}

/// Semantic style of the inline state word (`USER` red pill, `dir` peach
/// on `surface0`, otherwise `mark.style`).
pub(super) fn inline_word_style(mark: Person, palette: &Palette) -> Style {
    match mark {
        Person::UserPrompt => Style::default()
            .fg(contrast_fg(palette))
            .bg(palette.red)
            .add_modifier(Modifier::BOLD),
        Person::DirPrompt => Style::default().fg(palette.peach).bg(palette.surface0),
        _ => mark.style(palette),
    }
}

/// Paint a strict 1-line role row:
/// `[focus] <glyph> <role_label>[ <priority>]  <state>[ <age>][ · <detail…>]`
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_role_row(
    buf: &mut Buffer,
    row: Rect,
    glyph_x: u16,
    content_x: u16,
    right_limit: u16,
    role: &RoleSummary,
    spin: usize,
    now_ms: u64,
    fallback_detail: Option<&str>,
    palette: &Palette,
) {
    if row.height == 0 {
        return;
    }
    let inline = Person::inline_state(role, now_ms);
    let mark = inline.mark;
    put_text(
        buf,
        glyph_x,
        row.y,
        1,
        mark.glyph(spin),
        mark.style(palette),
    );
    let content_w = right_limit.saturating_sub(content_x) as usize;
    if content_w == 0 {
        return;
    }

    let dim = matches!(mark, Person::Standby | Person::Closed | Person::Resumable);
    let label = role_label(role);
    let flag = role.priority.and_then(PriorityMark::of);
    let detail = inline.detail.clone().or_else(|| {
        matches!(mark, Person::Idle | Person::Ready)
            .then(|| clean_single_line(fallback_detail.filter(|d| *d != "idle")))?
    });

    // Narrow rows switch `working` to `work` (< 24 cols total row).
    let mut word = if mark == Person::Working && row.width < 24 {
        "work"
    } else {
        inline.word
    };
    let mut age_part = inline.age.as_deref().map(|a| format!(" {a}"));
    let mut show_flag = flag.is_some();
    let base_gap = if content_w >= 28 && !show_flag {
        2usize
    } else {
        1usize
    };

    let state_len = |w: &str, a: Option<&str>| display_width(w) + a.map_or(0, display_width);
    if state_len(word, age_part.as_deref()) >= content_w {
        if mark == Person::Working {
            word = "work";
        }
        if state_len(word, age_part.as_deref()) >= content_w {
            age_part = None;
        }
    }

    let mut gap_w = base_gap;
    let label_w = display_width(&label);
    let flag_w = |show: bool| {
        if show {
            flag.map_or(0, |f| 1 + display_width(f.glyph()))
        } else {
            0
        }
    };
    let mut st_w = state_len(word, age_part.as_deref());
    let mut need = label_w + flag_w(show_flag) + gap_w + st_w;
    if need > content_w && mark == Person::Working && word != "work" {
        word = "work";
        st_w = state_len(word, age_part.as_deref());
        need = label_w + flag_w(show_flag) + gap_w + st_w;
    }
    if need > content_w && age_part.is_some() {
        age_part = None;
        st_w = state_len(word, None);
        need = label_w + flag_w(show_flag) + gap_w + st_w;
    }
    if need > content_w && gap_w > 1 {
        gap_w = 1;
        need = label_w + flag_w(show_flag) + gap_w + st_w;
    }
    if need > content_w && show_flag {
        show_flag = false;
    }

    let fw = flag_w(show_flag);
    let max_label = content_w.saturating_sub(fw + gap_w + st_w);
    let shown_label = truncate_end(&label, max_label);
    let shown_label_w = display_width(&shown_label);
    let name_fg = if dim { palette.overlay0 } else { palette.text };

    let mut x = content_x;
    let drawn = put_role_badged_text(
        buf,
        x,
        row.y,
        (shown_label_w as u16).min(right_limit.saturating_sub(x)),
        &shown_label,
        Style::default().fg(name_fg),
        palette,
    );
    x += drawn;

    if show_flag && let Some(flag) = flag {
        let fg_w = display_width(flag.glyph()) as u16;
        if x + 1 + fg_w <= right_limit {
            put_text(
                buf,
                x + 1,
                row.y,
                fg_w,
                flag.glyph(),
                Style::default().fg(flag.color(palette)),
            );
            x += 1 + fg_w;
        }
    }

    x = (x + gap_w as u16).min(right_limit);
    let word_w = (display_width(word) as u16).min(right_limit.saturating_sub(x));
    let shown_word = truncate_end(word, word_w as usize);
    put_text(
        buf,
        x,
        row.y,
        display_width(&shown_word) as u16,
        &shown_word,
        inline_word_style(mark, palette),
    );
    x += display_width(&shown_word) as u16;

    if let Some(age) = &age_part {
        let age_w = display_width(age) as u16;
        if x + age_w <= right_limit {
            let age_style = if mark == Person::DirPrompt {
                Style::default().fg(palette.peach)
            } else {
                mark.style(palette)
            };
            put_text(buf, x, row.y, age_w, age, age_style);
            x += age_w;
        }
    }

    let rem = right_limit.saturating_sub(x) as usize;
    if let Some(detail_text) = detail
        && rem >= 5
    {
        let cut = truncate_end(&detail_text, rem - 3);
        if !cut.is_empty() && (cut != "…" || detail_text.starts_with('…')) {
            let tail_fg = if matches!(mark, Person::Idle | Person::Ready) {
                palette.overlay0
            } else {
                palette.subtext0
            };
            put_text(
                buf,
                x,
                row.y,
                3,
                " · ",
                Style::default().fg(palette.overlay0),
            );
            put_text(
                buf,
                x + 3,
                row.y,
                display_width(&cut) as u16,
                &cut,
                Style::default().fg(tail_fg),
            );
        }
    }
}
