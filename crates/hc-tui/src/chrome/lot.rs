//! The Lot Pill and the studio header (S0 — spec §3, User Decision
//! 5). The pill is a reserved, clickable segment at the right end of
//! the tab bar — or left of `☰` in the mobile header — replacing the
//! floating collapsed strip. In a studio workspace the tab bar
//! becomes a one-row studio header instead: `acme-shop › actor-a t1
//! ◐ ↑` on the left, the pill on the right. Both surfaces read the
//! same counts (`U` user-owned waits, `C` open cards + ready
//! studios) and the same tier ladder.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use super::palette::Palette;
use super::render::{ShellHitMap, display_width, put_text};
use super::status::panel_contrast_fg;
use crate::hollywood::HollywoodActivity;
use crate::studio::StudioKey;
use crate::studio::glyphs::{Person, PriorityMark};

/// What the pill counts: `S` studios, `U` the one needs-you list
/// (prompts, roles that need the user, blocked Hollywood, new
/// studios, stage issues, cards and ready studios — so `C`, its old
/// ready-work count, stays 0 now that the list covers it), `unread`
/// new Hollywood replies while collapsed, and `activity` for spinner.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LotCounts {
    pub studios: usize,
    pub urgent: usize,
    pub cards: usize,
    pub unread: usize,
    pub activity: HollywoodActivity,
}

/// Which text ladder a terminal width gets — the spec §3 rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PillTier {
    /// `160`-class: ` ✦ 2 studios · !1 you · 1 card ` (31 cols).
    Full,
    /// `120`-class: ` ✦ 2 · !1 you · 1c ` (19 cols).
    Wide,
    /// `80`-class tab bar: ` ✦2 !1 c1 ` (11 cols).
    Compact,
    /// `60`-class mobile header: the compact text (11 cols).
    Mobile,
    /// `40`-class mobile header: ` ✦!1 ` (5 cols).
    Tiny,
}

impl PillTier {
    /// The tier's reserved cap from spec §3 — a pill never takes
    /// more than this from the tab bar.
    fn cap(self) -> u16 {
        match self {
            PillTier::Full => 31,
            PillTier::Wide => 19,
            PillTier::Compact | PillTier::Mobile => 11,
            PillTier::Tiny => 5,
        }
    }

    /// The next-narrower form when the text overflows the cap.
    fn demote(self) -> Option<PillTier> {
        match self {
            PillTier::Full => Some(PillTier::Wide),
            PillTier::Wide => Some(PillTier::Compact),
            PillTier::Compact | PillTier::Mobile => Some(PillTier::Tiny),
            PillTier::Tiny => None,
        }
    }
}

/// The tier for `cols` of terminal: desktop widths pick by column
/// band; under `mobile_threshold` the mobile rows apply, with the
/// `<= 40` cut for the tiny form.
pub(crate) fn pill_tier(cols: u16, mobile_threshold: u16) -> PillTier {
    if cols >= 140 {
        PillTier::Full
    } else if cols >= 96 {
        PillTier::Wide
    } else if cols > mobile_threshold {
        PillTier::Compact
    } else if cols > 40 {
        PillTier::Mobile
    } else {
        PillTier::Tiny
    }
}

/// A segment's role — the pill styles each part by kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Seg {
    /// `✦` (or spinner while active) — the lot mark, accent.
    Spark,
    /// Counts and connective text.
    Body,
    /// The card/ready segment — peach under the card style.
    Cards,
    /// Unread Hollywood replies — bold accent.
    Unread,
    /// The all-clear `●`/`live` word — green under the calm style.
    Live,
}

fn count(n: usize) -> String {
    if n > 99 {
        "9+".to_string()
    } else {
        n.to_string()
    }
}

/// The pill's styled text segments for `tier` — the `hollywood`
/// flag is the 0-studios empty form (` ✦ hollywood · idle `).
fn segments(
    counts: &LotCounts,
    tier: PillTier,
    hollywood: bool,
    spin: usize,
) -> Vec<(String, Seg)> {
    let (s, u, c, n) = (counts.studios, counts.urgent, counts.cards, counts.unread);
    let spark = if counts.activity.is_active() {
        Person::Working.glyph(spin)
    } else {
        "✦"
    };
    if s == 0 && hollywood {
        let (tail, tail_seg) = if n > 0 {
            (format!(" · +{} new ", count(n)), Seg::Unread)
        } else {
            match counts.activity {
                HollywoodActivity::Working => (" · thinking ".into(), Seg::Spark),
                HollywoodActivity::Queued => (" · queued ".into(), Seg::Cards),
                HollywoodActivity::Idle => (" · idle ".into(), Seg::Live),
            }
        };
        return match tier {
            PillTier::Full => vec![
                (format!(" {spark} hollywood"), Seg::Spark),
                (tail, tail_seg),
            ],
            PillTier::Wide => vec![(format!(" {spark} 0"), Seg::Spark), (tail, tail_seg)],
            PillTier::Compact | PillTier::Mobile => {
                let tail = if n > 0 {
                    (format!("+{} ", count(n)), Seg::Unread)
                } else {
                    ("● ".into(), Seg::Live)
                };
                vec![(format!(" {spark}0 "), Seg::Spark), tail]
            }
            PillTier::Tiny => {
                if n > 0 {
                    vec![
                        (format!(" {spark}"), Seg::Spark),
                        (format!("+{} ", count(n)), Seg::Unread),
                    ]
                } else {
                    vec![(format!(" {spark}0 "), Seg::Spark)]
                }
            }
        };
    }
    match tier {
        PillTier::Full => {
            let mut out = vec![
                (format!(" {spark} "), Seg::Spark),
                (format!("{s} studios"), Seg::Body),
            ];
            if u > 0 {
                out.push((format!(" · !{} you", count(u)), Seg::Body));
            }
            if c > 0 {
                out.push((
                    if n > 0 {
                        format!(" · {}c", count(c))
                    } else {
                        format!(" · {} card{}", count(c), if c == 1 { "" } else { "s" })
                    },
                    Seg::Cards,
                ));
            }
            if n > 0 {
                out.push((format!(" · +{} new", count(n)), Seg::Unread));
            }
            if u == 0 && c == 0 && n == 0 {
                match counts.activity {
                    HollywoodActivity::Working => out.push((" · thinking".into(), Seg::Spark)),
                    HollywoodActivity::Queued => out.push((" · queued".into(), Seg::Cards)),
                    HollywoodActivity::Idle => out.push((" · live".into(), Seg::Live)),
                }
            }
            out.push((" ".into(), Seg::Body));
            out
        }
        PillTier::Wide => {
            let mut out = vec![
                (format!(" {spark} "), Seg::Spark),
                (format!("{s}"), Seg::Body),
            ];
            if u > 0 {
                out.push((format!(" · !{} you", count(u)), Seg::Body));
            }
            if c > 0 {
                out.push((
                    if u > 0 || n > 0 {
                        format!(" · {}c", count(c))
                    } else {
                        format!(" · {} card{}", count(c), if c == 1 { "" } else { "s" })
                    },
                    Seg::Cards,
                ));
            }
            if n > 0 {
                out.push((
                    if u == 0 && c == 0 {
                        format!(" · +{} new", count(n))
                    } else {
                        format!(" · +{}", count(n))
                    },
                    Seg::Unread,
                ));
            }
            if u == 0 && c == 0 && n == 0 {
                match counts.activity {
                    HollywoodActivity::Working => out.push((" · work".into(), Seg::Spark)),
                    HollywoodActivity::Queued => out.push((" · queue".into(), Seg::Cards)),
                    HollywoodActivity::Idle => out.push((" · live".into(), Seg::Live)),
                }
            }
            out.push((" ".into(), Seg::Body));
            out
        }
        PillTier::Compact | PillTier::Mobile => {
            let mut out = vec![
                (format!(" {spark}"), Seg::Spark),
                (format!("{s}"), Seg::Body),
            ];
            let dense = u > 0 && c > 0 && n > 0;
            if u > 0 {
                out.push((format!(" !{}", count(u)), Seg::Body));
            }
            if c > 0 {
                out.push((
                    if dense {
                        format!("c{}", count(c))
                    } else {
                        format!(" c{}", count(c))
                    },
                    Seg::Cards,
                ));
            }
            if n > 0 {
                out.push((
                    if dense {
                        format!("+{}", count(n))
                    } else {
                        format!(" +{}", count(n))
                    },
                    Seg::Unread,
                ));
            }
            if u == 0 && c == 0 && n == 0 {
                out.push((" ●".into(), Seg::Live));
            }
            out.push((" ".into(), Seg::Body));
            out
        }
        PillTier::Tiny => {
            if u > 0 {
                vec![
                    (format!(" {spark}"), Seg::Spark),
                    (format!("!{} ", count(u)), Seg::Body),
                ]
            } else if n > 0 {
                vec![
                    (format!(" {spark}"), Seg::Spark),
                    (format!("+{} ", count(n)), Seg::Unread),
                ]
            } else if c > 0 {
                vec![
                    (format!(" {spark}"), Seg::Spark),
                    (format!("c{} ", count(c)), Seg::Cards),
                ]
            } else {
                vec![
                    (format!(" {spark}"), Seg::Spark),
                    (format!("{s} "), Seg::Body),
                ]
            }
        }
    }
}

/// A segment's style under the pill's colour rules (§3):
/// urgent paints the whole pill `red`, while non-urgent pills (cards,
/// unread replies, or calm) sit on `elevation.raised_row`.
fn seg_style(seg: Seg, counts: &LotCounts, palette: &Palette) -> Style {
    if counts.urgent > 0 {
        return Style::default()
            .fg(panel_contrast_fg(palette))
            .bg(palette.red)
            .add_modifier(Modifier::BOLD);
    }
    let base = Style::default().bg(palette.elevation().raised_row);
    if counts.cards > 0 || counts.unread > 0 {
        match seg {
            Seg::Spark => base.fg(palette.accent),
            Seg::Body => base.fg(palette.text),
            Seg::Cards => base.fg(palette.peach).add_modifier(Modifier::BOLD),
            Seg::Unread => base.fg(palette.accent).add_modifier(Modifier::BOLD),
            Seg::Live => base.fg(palette.green),
        }
    } else {
        match seg {
            Seg::Spark => base.fg(palette.accent),
            Seg::Body => base.fg(palette.subtext0),
            Seg::Cards => base.fg(palette.peach).add_modifier(Modifier::BOLD),
            Seg::Unread => base.fg(palette.accent).add_modifier(Modifier::BOLD),
            Seg::Live => base.fg(palette.green),
        }
    }
}

/// The pill's drawn width at `tier` — demoting until the text fits
/// the tier's cap. Returns the segments and the tier used.
fn pill_fit(
    counts: &LotCounts,
    tier: PillTier,
    hollywood: bool,
    spin: usize,
) -> (Vec<(String, Seg)>, PillTier) {
    let mut tier = tier;
    loop {
        let segs = segments(counts, tier, hollywood, spin);
        let width: u16 = segs.iter().map(|(t, _)| display_width(t)).sum();
        if width <= tier.cap() {
            return (segs, tier);
        }
        match tier.demote() {
            Some(next) => tier = next,
            None => return (segs, tier),
        }
    }
}

/// The width the pill would claim at `tier` — the tab bar reserves
/// exactly this at its right end.
pub(crate) fn pill_width(counts: &LotCounts, tier: PillTier, hollywood: bool) -> u16 {
    let (segs, _) = pill_fit(counts, tier, hollywood, 0);
    segs.iter().map(|(t, _)| display_width(t)).sum()
}

/// Draw the pill into `rect` (already sized to `pill_width`) and
/// return its hit rect.
pub(crate) fn render_pill(
    buf: &mut Buffer,
    rect: Rect,
    counts: &LotCounts,
    tier: PillTier,
    hollywood: bool,
    spin: usize,
    palette: &Palette,
) -> Rect {
    if rect.width == 0 || rect.height == 0 {
        return Rect::default();
    }
    let (segs, used) = pill_fit(counts, tier, hollywood, spin);
    let mut x = rect.x;
    let right = rect.right();
    for (text, seg) in segs {
        let _ = used;
        let room = right.saturating_sub(x);
        if room == 0 {
            break;
        }
        let width = display_width(&text).min(room);
        put_text(
            buf,
            x,
            rect.y,
            width,
            &text,
            seg_style(seg, counts, palette),
        );
        x = x.saturating_add(width);
    }
    // Any slack keeps the pill's own background, never stale cells.
    if x < right {
        let base = seg_style(Seg::Body, counts, palette);
        buf.set_style(Rect::new(x, rect.y, right - x, 1), base);
    }
    rect
}

/// The app-side lot latch — rebuilt every `sync_studio`, read by
/// `render_shell` and the layout. In plain mode it stays all-empty:
/// no pill, no header, `chrome_layout` sees nothing to force.
#[derive(Debug, Default)]
pub(crate) struct Lot {
    /// U/C/N for the pill.
    pub counts: LotCounts,
    /// The focused workspace's studio header — `Some` only while a
    /// studio role, studio shell or the Hollywood terminal is
    /// focused.
    pub header: Option<StudioHeader>,
    /// The pill reserves its segment when this holds — studios, or a
    /// configured Hollywood.
    pub pill: bool,
    /// The focused workspace is a studio role or shell — where the
    /// role keys (`alt+↑/↓` by default) win over herdr's direct bindings.
    pub studio_workspace: bool,
    /// U > 0 on the last publish — `when_waiting` reads the edge.
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
    /// The full terminal width — the pill tier and the header's
    /// truncation read it.
    pub cols: u16,
    pub spin: usize,
}

/// What the header shows under the focused terminal (Decision 5):
/// a role with its S0 glyph, task and priority; a studio shell; the
/// Hollywood terminal; or the bare studio.
#[derive(Debug, Clone)]
pub(crate) enum HeaderKind {
    Role {
        name: String,
        task: Option<String>,
        /// `None` while the role is not in the summary — the name
        /// still reads; the glyph simply does not invent a state.
        mark: Option<Person>,
        priority: Option<PriorityMark>,
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
    /// `true` while `StudioFocus::Studio(key)` points at this studio.
    pub studio_focused: bool,
    pub kind: HeaderKind,
}

/// The header's left text — `acme-shop › actor-a t1`, also the
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
        HeaderKind::Role { name, task, .. } => match task {
            Some(task) => format!("{id} › {name} {task}"),
            None => format!("{id} › {name}"),
        },
    }
}

/// The studio id's own width, leading space included (` acme-shop`) —
/// what a narrow mobile header must keep whole before the update hint
/// may claim cells. The `· machine` qualifier is the first thing a
/// tight row may clip.
pub(crate) fn header_name_width(header: &StudioHeader) -> u16 {
    display_width(&format!(" {}", header.studio))
}

/// The studio header's styled left pieces: ` id · machine › part`
/// with the person glyph and priority mark in their colours.
fn header_spans(header: &StudioHeader, palette: &Palette) -> Vec<(String, Style)> {
    let header_bg = palette.elevation().surface;
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
    let mut spans = vec![
        (
            format!(" {}", header.studio),
            if header.studio_focused {
                Style::default()
                    .fg(palette.accent)
                    .bg(header_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                text.add_modifier(Modifier::BOLD)
            },
        ),
        (
            header
                .machine
                .as_ref()
                .map(|m| format!(" · {m}"))
                .unwrap_or_default(),
            dim,
        ),
        (" › ".into(), dim),
    ];
    match &header.kind {
        HeaderKind::Bare => {
            spans.pop();
        }
        HeaderKind::Shell(n) => {
            spans.push((format!("shell {n}"), text));
        }
        HeaderKind::Role {
            name,
            task,
            mark,
            priority,
        } => {
            spans.push((name.clone(), text));
            if let Some(task) = task {
                spans.push((
                    format!(" {task}"),
                    Style::default().fg(palette.accent).bg(header_bg),
                ));
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
/// in `hits.lot_pill`.
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
    buf.set_style(area, Style::default().bg(palette.elevation().surface));
    let pill_w = pill_width(&lot.counts, tier, lot.agent);
    let name_rows = area.width.saturating_sub(pill_w.saturating_add(1));
    let mut x = area.x;
    let right = area.x.saturating_add(name_rows);
    let name_end = x.saturating_add(display_width(&header.studio) + 1);
    for (text, style) in header_spans(header, palette) {
        let room = right.saturating_sub(x);
        if room == 0 {
            break;
        }
        let clipped = crate::chrome::text::truncate_end(&text, room as usize);
        let width = display_width(&clipped).min(room);
        put_text(buf, x, area.y, width, &clipped, style);
        x = x.saturating_add(width);
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
    hits.lot_pill = render_pill(
        buf,
        Rect::new(area.right().saturating_sub(pill_w), area.y, pill_w, 1),
        &lot.counts,
        tier,
        lot.agent,
        lot.spin,
        palette,
    );
}
