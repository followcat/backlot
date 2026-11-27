//! The Lot Pill and the studio header (S0 — spec §3, User Decision 5).
//! The pill lives in `lot_pill.rs`; this module owns the studio
//! workspace header (`id · machine › Ⓟ ○  runner · model (effort)`)
//! and the `Lot` / `LotFrame` latch structs.

#[path = "lot_pill.rs"]
mod pill;

pub(crate) use pill::{
    LotCounts, PillAction, PillTier, pill_tier, pill_width, render_pill_recorded,
};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::palette::Palette;
use super::render::{ShellHitMap, display_width, put_text};
use crate::studio::StudioKey;
use crate::studio::glyphs::{Person, PriorityMark};
use crate::studio::role_row::RoleBadge;

/// The app-side lot latch — rebuilt every `sync_studio`, read by
/// `render_shell` and the layout. In plain mode it stays all-empty:
/// no pill, no header, `chrome_layout` sees nothing to force.
#[derive(Debug, Default)]
pub(crate) struct Lot {
    /// Bucketed counts for the pill.
    pub counts: LotCounts,
    /// The focused workspace's studio header — `Some` only while a
    /// studio role, studio shell or the Hollywood terminal is focused.
    pub header: Option<StudioHeader>,
    /// The pill reserves its segment when this holds — studios, or a
    /// configured Hollywood.
    pub pill: bool,
    /// The focused workspace is a studio role or shell — where the
    /// role keys (`alt+↑/↓` by default) win over herdr's direct bindings.
    pub studio_workspace: bool,
    /// Waiting items > 0 on the last publish — `when_waiting` reads the edge.
    pub was_urgent: bool,
    /// Apply `[hollywood].start` on the first publish.
    pub start_pending: bool,
    /// The start policy already ran once — a config reload never
    /// re-applies it over the user's own mode.
    pub start_applied: bool,
}

/// What `render_shell` needs for one lot draw — the latch plus the
/// two layout facts it does not know: the full row width and whether
/// a Hollywood agent is configured.
pub(crate) struct LotFrame<'a> {
    pub counts: LotCounts,
    pub agent: bool,
    pub pill: bool,
    pub header: Option<&'a StudioHeader>,
    /// Show `[unfocused · Esc returns]` at the right end of the studio top bar
    /// (before the pill) when focus leaves a borderless single pane.
    pub unfocused_cue: bool,
    /// The full terminal width — the pill tier and the header's
    /// truncation read it.
    pub cols: u16,
    pub spin: usize,
    pub studio_chrome: bool,
}

/// What the header shows under the focused terminal (Decision 5):
/// a role with its badge, S0 glyph, priority and runner/model/effort;
/// a studio shell; the Hollywood terminal; or the bare studio.
#[derive(Debug, Clone)]
pub(crate) enum HeaderKind {
    Role {
        role_key: String,
        name: String,
        task: Option<String>,
        /// `None` while the role is not in the summary — the name
        /// still reads; the glyph simply does not invent a state.
        mark: Option<Person>,
        priority: Option<PriorityMark>,
        runner: Option<String>,
        model: Option<String>,
        effort: Option<String>,
    },
    Shell(u32),
    Hollywood,
    Bare,
}

/// A studio workspace's one-row header model — the app builds it,
/// `chrome::lot` styles and draws it.
#[derive(Debug, Clone)]
pub(crate) struct StudioHeader {
    /// The studio the header's name click addresses — `None` for the
    /// `hollywood` form, which owns no studio.
    pub key: Option<StudioKey>,
    /// The studio id — the clickable name.
    pub studio: String,
    /// `· host-b` when the aggregate spans machines.
    pub machine: Option<String>,
    pub kind: HeaderKind,
}

fn role_badge_and_tail(
    role_key: &str,
    name: &str,
    task: Option<&str>,
) -> (Option<RoleBadge>, Option<String>) {
    if let Some(badge) = RoleBadge::of_role(role_key) {
        let tail = if badge == RoleBadge::Actor {
            Some(task.unwrap_or(name).to_string())
        } else {
            None
        };
        return (Some(badge), tail);
    }
    let text = match task {
        Some(task) => format!("{name} {task}"),
        None => name.to_string(),
    };
    (None, Some(text))
}

/// Fit role runtime metadata (`  runner · model (effort)`) into `room`,
/// degrading `effort` -> `model` -> `runner` -> `None`.
fn fit_role_meta(
    runner: Option<&str>,
    model: Option<&str>,
    effort: Option<&str>,
    room: usize,
) -> Option<String> {
    let runner = runner.map(str::trim).filter(|s| !s.is_empty());
    let model = model.map(str::trim).filter(|s| !s.is_empty());
    let effort = effort
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "auto");
    let mut cands = Vec::with_capacity(3);
    let fmt = |r: Option<&str>, m: Option<&str>, e: Option<&str>| -> Option<String> {
        let body = match (r, m) {
            (Some(r), Some(m)) => format!("{r} · {m}"),
            (Some(r), None) => r.to_string(),
            (None, Some(m)) => m.to_string(),
            (None, None) => return None,
        };
        Some(match e {
            Some(e) => format!("  {body} ({e})"),
            None => format!("  {body}"),
        })
    };
    if let Some(full) = fmt(runner, model, effort) {
        cands.push(full);
    }
    if let Some(no_eff) = fmt(runner, model, None)
        && !cands.contains(&no_eff)
    {
        cands.push(no_eff);
    }
    if let Some(runner_only) = fmt(runner, None, None)
        && !cands.contains(&runner_only)
    {
        cands.push(runner_only);
    }
    cands
        .into_iter()
        .find(|cand| crate::chrome::text::display_width(cand) <= room)
}

/// The header's left text — `acme-shop › Ⓟ ○`, also the
/// mobile header's tab-label replacement.
pub(crate) fn header_text(header: &StudioHeader) -> String {
    let id = match &header.machine {
        Some(machine) => format!("{} · {machine}", header.studio),
        None => header.studio.clone(),
    };
    match &header.kind {
        HeaderKind::Hollywood => "hollywood".to_string(),
        HeaderKind::Bare => id,
        HeaderKind::Shell(n) => format!("{id} › shell {n}"),
        HeaderKind::Role {
            role_key,
            name,
            task,
            mark,
            ..
        } => {
            let role_part = match role_badge_and_tail(role_key, name, task.as_deref()) {
                (Some(badge), Some(tail)) => format!("{} {tail}", badge.glyph()),
                (Some(badge), None) => badge.glyph().to_string(),
                (None, Some(text)) => text,
                (None, None) => String::new(),
            };
            match mark {
                Some(mark) => format!("{id} › {role_part} {}", mark.still()),
                None => format!("{id} › {role_part}"),
            }
        }
    }
}

/// The studio id's own capsule width, leading and trailing space included
/// (` acme-shop `) — what a narrow mobile header must keep whole before the
/// update hint may claim cells. The `· machine` qualifier is the first thing
/// a tight row may clip.
pub(crate) fn header_name_width(header: &StudioHeader) -> u16 {
    display_width(&format!(" {} ", header.studio))
}

/// The studio header's styled left pieces: ` id · machine › part`
/// with the current-studio capsule, role badge, person glyph and
/// priority mark in their colours.
fn header_spans(
    header: &StudioHeader,
    palette: &Palette,
    studio_chrome: bool,
) -> Vec<(String, Style)> {
    let header_bg = pill::deco::header_bg(palette, studio_chrome);
    let capsule = crate::studio::glyphs::studio_capsule_style(palette);
    let text = Style::default().fg(palette.text).bg(header_bg);
    let dim = Style::default().fg(palette.overlay0).bg(header_bg);
    if let HeaderKind::Hollywood = header.kind {
        return vec![
            (
                " hollywood".into(),
                Style::default()
                    .fg(palette.accent)
                    .bg(header_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            (" ".into(), text),
        ];
    }
    let sep = if header.machine.is_some() {
        " › "
    } else {
        "› "
    };
    let mut spans = vec![
        (format!(" {} ", header.studio), capsule),
        (
            header
                .machine
                .as_ref()
                .map(|m| format!("· {m}"))
                .unwrap_or_default(),
            dim,
        ),
        (sep.into(), dim),
    ];
    match &header.kind {
        HeaderKind::Bare => {
            spans.pop();
        }
        HeaderKind::Shell(n) => {
            spans.push((format!("shell {n}"), text));
        }
        HeaderKind::Role {
            role_key,
            name,
            task,
            mark,
            priority,
            ..
        } => {
            match role_badge_and_tail(role_key, name, task.as_deref()) {
                (Some(badge), tail) => {
                    spans.push((
                        badge.glyph().to_string(),
                        Style::default().fg(badge.color(palette)).bg(header_bg),
                    ));
                    if let Some(tail) = tail {
                        spans.push((format!(" {tail}"), text));
                    }
                }
                (None, _) => {
                    spans.push((name.clone(), text));
                    if let Some(task) = task {
                        spans.push((
                            format!(" {task}"),
                            Style::default().fg(palette.accent).bg(header_bg),
                        ));
                    }
                }
            }
            if let Some(mark) = mark {
                spans.push((
                    format!(" {}", mark.still()),
                    mark.style(palette).bg(header_bg),
                ));
            }
            if let Some(priority) = priority {
                spans.push((
                    format!(" {}", priority.glyph()),
                    Style::default().fg(priority.color(palette)).bg(header_bg),
                ));
            }
        }
        HeaderKind::Hollywood => unreachable!("hollywood returns above"),
    }
    spans
}

/// Draw the studio header into `area` — the workspace's own row
/// where the tab bar was — plus the pill at its right end. Returns
/// the studio-name hit rect in `hits.studio_header` and the pill's
/// in `hits.lot_pill` / `hits.lot_pill_segments`.
pub(crate) fn render_studio_header(
    buf: &mut Buffer,
    area: Rect,
    lot: &LotFrame<'_>,
    tier: PillTier,
    palette: &Palette,
    hits: &mut ShellHitMap,
) {
    let Some(header) = lot.header else {
        return;
    };
    if area.is_empty() {
        return;
    }
    let header_bg = pill::deco::header_bg(palette, lot.studio_chrome);
    buf.set_style(area, Style::default().bg(header_bg));
    let pill_w = pill_width(&lot.counts, tier, lot.agent, lot.studio_chrome);
    let name_rows = area.width.saturating_sub(pill_w.saturating_add(1));
    let mut x = area.x;
    let right = area.x.saturating_add(name_rows);
    let name_end = x.saturating_add(display_width(&header.studio) + 2);
    for (text, style) in header_spans(header, palette, lot.studio_chrome) {
        let room = right.saturating_sub(x);
        if room == 0 {
            break;
        }
        let clipped = crate::chrome::text::truncate_end(&text, room as usize);
        let width = display_width(&clipped).min(room);
        put_text(buf, x, area.y, width, &clipped, style);
        x = x.saturating_add(width);
    }
    if let HeaderKind::Role {
        runner,
        model,
        effort,
        ..
    } = &header.kind
    {
        let room = right.saturating_sub(x) as usize;
        if let Some(meta) =
            fit_role_meta(runner.as_deref(), model.as_deref(), effort.as_deref(), room)
        {
            let meta_w = display_width(&meta).min(right.saturating_sub(x));
            put_text(
                buf,
                x,
                area.y,
                meta_w,
                &meta,
                Style::default().fg(palette.subtext0).bg(header_bg),
            );
            x = x.saturating_add(meta_w);
        }
    }
    if lot.unfocused_cue {
        let cue = "[unfocused · Esc returns]";
        let cue_w = display_width(cue);
        if right.saturating_sub(x) >= cue_w + 2 {
            put_text(
                buf,
                right.saturating_sub(cue_w + 1),
                area.y,
                cue_w,
                cue,
                Style::default().fg(palette.overlay0).bg(header_bg),
            );
        }
    }
    if let Some(key) = &header.key {
        hits.studio_header = Rect::new(
            area.x,
            area.y,
            name_end.min(right).saturating_sub(area.x),
            1,
        );
        hits.studio_key = Some(key.clone());
    }
    render_pill_recorded(
        buf,
        Rect::new(area.right().saturating_sub(pill_w), area.y, pill_w, 1),
        &lot.counts,
        tier,
        lot.agent,
        lot.spin,
        lot.studio_chrome,
        palette,
        hits,
    );
}

#[cfg(test)]
#[path = "lot_tests.rs"]
mod tests;
