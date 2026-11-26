//! The Lot Pill model and painter (`S3 §4.1`): bucketed counts
//! (`studios`, `blocking`, `asks`, `cards`, `ready`, `unread`, `activity`),
//! width tier ladder, per-segment hit rects (`PillAction`), and colors
//! where `blocking > 0` paints red and non-blocking pills sit on
//! `elevation.raised_row`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::chrome::palette::Palette;
use crate::chrome::render::{ShellHitMap, display_width, put_text};
use crate::chrome::status::panel_contrast_fg;
use crate::hollywood::HollywoodActivity;
use crate::studio::glyphs::Person;

/// Which bucket a clicked pill segment opens in the Call Sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PillAction {
    Default,
    Blocking,
    Asks,
    Cards,
    Ready,
}

/// Bucketed counts shown by the Lot Pill (`S3 §4.1`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LotCounts {
    pub studios: usize,
    pub blocking: usize,
    pub asks: usize,
    pub cards: usize,
    pub ready: usize,
    pub unread: usize,
    pub activity: HollywoodActivity,
}

impl LotCounts {
    /// Total actionable items across all four Call Sheet buckets.
    pub(crate) fn any_waiting(&self) -> usize {
        self.blocking + self.asks + self.cards + self.ready
    }
}

/// Width tier for the Lot Pill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PillTier {
    /// `140+` cols: ` ✦ N · !B · ?A · C cards · ✓R ready · alt+y `.
    Full,
    /// `96..140` cols: ` ✦ N · !B · ?A · Cc · ✓R `.
    Wide,
    /// `41..96` cols desktop tab bar: ` ✦N !B ?A cC ✓R `.
    Compact,
    /// `41..=mobile_threshold` mobile header: compact text.
    Mobile,
    /// `<= 40` cols: ` ✦!<top> ` prioritized `blocking > asks > cards > unread > ready`.
    Tiny,
}

impl PillTier {
    fn cap(self) -> u16 {
        match self {
            PillTier::Full => 46,
            PillTier::Wide => 28,
            PillTier::Compact | PillTier::Mobile => 16,
            PillTier::Tiny => 6,
        }
    }

    fn demote(self) -> Option<PillTier> {
        match self {
            PillTier::Full => Some(PillTier::Wide),
            PillTier::Wide => Some(PillTier::Compact),
            PillTier::Compact | PillTier::Mobile => Some(PillTier::Tiny),
            PillTier::Tiny => None,
        }
    }
}

/// Select the starting pill tier from total terminal width and mobile cut-off.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Seg {
    Spark,
    Body,
    Blocking,
    Asks,
    Cards,
    Ready,
    Unread,
    Live,
    Hint,
}

impl Seg {
    fn action(self) -> PillAction {
        match self {
            Seg::Blocking => PillAction::Blocking,
            Seg::Asks => PillAction::Asks,
            Seg::Cards => PillAction::Cards,
            Seg::Ready => PillAction::Ready,
            Seg::Spark | Seg::Body | Seg::Unread | Seg::Live | Seg::Hint => PillAction::Default,
        }
    }
}

fn count(n: usize) -> String {
    if n > 99 {
        "9+".to_string()
    } else {
        n.to_string()
    }
}

fn push_bucket(out: &mut Vec<(String, Seg)>, sep: Option<&str>, text: String, seg: Seg, gap: bool) {
    if let Some(s) = sep {
        out.push((s.to_string(), Seg::Body));
    } else if gap {
        out.push((" ".to_string(), Seg::Body));
    }
    out.push((text, seg));
}

fn segments(
    counts: &LotCounts,
    tier: PillTier,
    hollywood: bool,
    spin: usize,
) -> Vec<(String, Seg)> {
    let (s, b, a, c, r, n) = (
        counts.studios,
        counts.blocking,
        counts.asks,
        counts.cards,
        counts.ready,
        counts.unread,
    );
    let spark = if counts.activity.is_active() {
        Person::Working.glyph(spin)
    } else {
        "✦"
    };
    if s == 0 && hollywood {
        let (tail, tail_seg) = if n > 0 {
            (format!(" · +{} new", count(n)), Seg::Unread)
        } else {
            match counts.activity {
                HollywoodActivity::Working => (" · thinking".into(), Seg::Spark),
                HollywoodActivity::Queued => (" · queued".into(), Seg::Cards),
                HollywoodActivity::Idle => (" · idle".into(), Seg::Live),
            }
        };
        return match tier {
            PillTier::Full => vec![
                (format!(" {spark} hollywood"), Seg::Spark),
                (tail, tail_seg),
                (" · alt+y".into(), Seg::Hint),
                (" ".into(), Seg::Body),
            ],
            PillTier::Wide => vec![
                (format!(" {spark} 0"), Seg::Spark),
                (tail, tail_seg),
                (" ".into(), Seg::Body),
            ],
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

    let alerts = b + a + c + r + n;
    match tier {
        PillTier::Full => {
            let mut out = vec![
                (format!(" {spark} "), Seg::Spark),
                (format!("{s}"), Seg::Body),
            ];
            if b > 0 {
                push_bucket(
                    &mut out,
                    Some(" · "),
                    format!("!{}", count(b)),
                    Seg::Blocking,
                    false,
                );
            }
            if a > 0 {
                push_bucket(
                    &mut out,
                    Some(" · "),
                    format!("?{}", count(a)),
                    Seg::Asks,
                    false,
                );
            }
            if c > 0 {
                let label = if b + a + r + n > 0 {
                    format!("{}c", count(c))
                } else {
                    format!("{} card{}", count(c), if c == 1 { "" } else { "s" })
                };
                push_bucket(&mut out, Some(" · "), label, Seg::Cards, false);
            }
            if r > 0 {
                push_bucket(
                    &mut out,
                    Some(" · "),
                    format!("✓{} ready", count(r)),
                    Seg::Ready,
                    false,
                );
            }
            if n > 0 {
                push_bucket(
                    &mut out,
                    Some(" · "),
                    format!("+{} new", count(n)),
                    Seg::Unread,
                    false,
                );
            }
            if alerts == 0 {
                match counts.activity {
                    HollywoodActivity::Working => out.push((" · thinking".into(), Seg::Spark)),
                    HollywoodActivity::Queued => out.push((" · queued".into(), Seg::Cards)),
                    HollywoodActivity::Idle => out.push((" · live".into(), Seg::Live)),
                }
            }
            out.push((" · alt+y".into(), Seg::Hint));
            out.push((" ".into(), Seg::Body));
            out
        }
        PillTier::Wide => {
            let mut out = vec![
                (format!(" {spark} "), Seg::Spark),
                (format!("{s}"), Seg::Body),
            ];
            if b > 0 {
                push_bucket(
                    &mut out,
                    Some(" · "),
                    format!("!{}", count(b)),
                    Seg::Blocking,
                    false,
                );
            }
            if a > 0 {
                push_bucket(
                    &mut out,
                    Some(" · "),
                    format!("?{}", count(a)),
                    Seg::Asks,
                    false,
                );
            }
            if c > 0 {
                let label = if b + a + r + n > 0 {
                    format!("{}c", count(c))
                } else {
                    format!("{} card{}", count(c), if c == 1 { "" } else { "s" })
                };
                push_bucket(&mut out, Some(" · "), label, Seg::Cards, false);
            }
            if r > 0 {
                push_bucket(
                    &mut out,
                    Some(" · "),
                    format!("✓{}", count(r)),
                    Seg::Ready,
                    false,
                );
            }
            if n > 0 {
                let label = if b + a + c + r == 0 {
                    format!("+{} new", count(n))
                } else {
                    format!("+{}", count(n))
                };
                push_bucket(&mut out, Some(" · "), label, Seg::Unread, false);
            }
            if alerts == 0 {
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
            let active_buckets = usize::from(b > 0)
                + usize::from(a > 0)
                + usize::from(c > 0)
                + usize::from(r > 0)
                + usize::from(n > 0);
            let dense = active_buckets >= 3;
            let mut first = true;
            for (val, text, seg) in [
                (b, format!("!{}", count(b)), Seg::Blocking),
                (a, format!("?{}", count(a)), Seg::Asks),
                (c, format!("c{}", count(c)), Seg::Cards),
                (r, format!("✓{}", count(r)), Seg::Ready),
                (n, format!("+{}", count(n)), Seg::Unread),
            ] {
                if val > 0 {
                    push_bucket(&mut out, None, text, seg, first || !dense);
                    first = false;
                }
            }
            if alerts == 0 {
                out.push((" ●".into(), Seg::Live));
            }
            out.push((" ".into(), Seg::Body));
            out
        }
        PillTier::Tiny => {
            let (tail, seg) = if b > 0 {
                (format!("!{} ", count(b)), Seg::Blocking)
            } else if a > 0 {
                (format!("?{} ", count(a)), Seg::Asks)
            } else if c > 0 {
                (format!("c{} ", count(c)), Seg::Cards)
            } else if n > 0 {
                (format!("+{} ", count(n)), Seg::Unread)
            } else if r > 0 {
                (format!("✓{} ", count(r)), Seg::Ready)
            } else {
                (format!("{s} "), Seg::Body)
            };
            vec![(format!(" {spark}"), Seg::Spark), (tail, seg)]
        }
    }
}

fn seg_style(seg: Seg, counts: &LotCounts, palette: &Palette) -> Style {
    if counts.blocking > 0 {
        let base = Style::default()
            .fg(panel_contrast_fg(palette))
            .bg(palette.red);
        return if seg == Seg::Hint {
            base
        } else {
            base.add_modifier(Modifier::BOLD)
        };
    }
    let base = Style::default().bg(palette.elevation().raised_row);
    let body_fg = if counts.asks > 0 || counts.cards > 0 || counts.ready > 0 || counts.unread > 0 {
        palette.text
    } else {
        palette.subtext0
    };
    match seg {
        Seg::Spark => base.fg(palette.accent),
        Seg::Body | Seg::Blocking => base.fg(body_fg),
        Seg::Asks | Seg::Cards => base.fg(palette.peach).add_modifier(Modifier::BOLD),
        Seg::Ready => base.fg(palette.teal).add_modifier(Modifier::BOLD),
        Seg::Unread => base.fg(palette.accent).add_modifier(Modifier::BOLD),
        Seg::Live => base.fg(palette.green),
        Seg::Hint => base.fg(palette.overlay0),
    }
}

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

/// The width the pill would claim at `tier`.
pub(crate) fn pill_width(counts: &LotCounts, tier: PillTier, hollywood: bool) -> u16 {
    let (segs, _) = pill_fit(counts, tier, hollywood, 0);
    segs.iter().map(|(t, _)| display_width(t)).sum()
}

/// Clickable bucket sub-rects inside `rect` matching the rendered pill segments.
pub(crate) fn pill_segments(
    rect: Rect,
    counts: &LotCounts,
    tier: PillTier,
    hollywood: bool,
    spin: usize,
) -> Vec<(Rect, PillAction)> {
    if rect.width == 0 || rect.height == 0 {
        return Vec::new();
    }
    let (segs, _) = pill_fit(counts, tier, hollywood, spin);
    let mut out = Vec::new();
    let mut x = rect.x;
    let right = rect.right();
    for (text, seg) in segs {
        let room = right.saturating_sub(x);
        if room == 0 {
            break;
        }
        let width = display_width(&text).min(room);
        let action = seg.action();
        if action != PillAction::Default && width > 0 {
            out.push((Rect::new(x, rect.y, width, 1), action));
        }
        x = x.saturating_add(width);
    }
    out
}

/// Draw the pill into `rect` and return its overall hit rect.
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
    let (segs, _) = pill_fit(counts, tier, hollywood, spin);
    let mut x = rect.x;
    let right = rect.right();
    for (text, seg) in segs {
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
    if x < right {
        let base = seg_style(Seg::Body, counts, palette);
        buf.set_style(Rect::new(x, rect.y, right - x, 1), base);
    }
    rect
}

/// Draw the pill into `rect` and record both `hits.lot_pill` and `hits.lot_pill_segments`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_pill_recorded(
    buf: &mut Buffer,
    rect: Rect,
    counts: &LotCounts,
    tier: PillTier,
    hollywood: bool,
    spin: usize,
    palette: &Palette,
    hits: &mut ShellHitMap,
) {
    hits.lot_pill = render_pill(buf, rect, counts, tier, hollywood, spin, palette);
    hits.lot_pill_segments = pill_segments(hits.lot_pill, counts, tier, hollywood, spin);
}
