//! The Call Sheet's two-phase focus-aware height planner:
//!
//! 1. **Lay out every zone in its unfocused form first** (title, 1-line
//!    waiting items, 1 newest talk line + compose, 1-row studio chip
//!    strip, hints, marquee sill — no top perforation, no external shadow).
//!    If that exceeds `cap`, shed in order: unfocused `HOLLYWOOD` rule ->
//!    hints -> unfocused talk (never when HOLLYWOOD is focused) -> scroll
//!    waiting window down to `min(3, waits)` -> unfocused studio strip
//!    (never when STUDIOS is focused) -> title -> scroll waiting window
//!    to `1` -> marquee sill (only when `cap < 3`). Compose and the
//!    selected waiting row never drop while `cap >= 2`.
//! 2. **Expand only the focused zone into the remaining rows** (`cap - base`):
//!    - WAITING: selected item's detail (`why:`, `Q1..Q3`, `finish?`).
//!    - HOLLYWOOD: `╠══╡ … HOLLYWOOD … ╞══╣` rule + newest talk lines with
//!      wraps (up to 8); if no room for rule, keep talk with `N earlier`.
//!    - STUDIOS: sprocket header + 7-row frames -> 5 rows -> 3 rows ->
//!      focused 1-row strip.
//!
//!    Any leftover spare rows after focused expansion may fund the
//!    unfocused `╟── HOLLYWOOD ──╢` crossbar when `waits > 0`.
//!
//! Expansion never pushes another zone's unfocused line out.

use ratatui::layout::Rect;

use super::rows::{self, Row, Target};
use super::sheet::{Status, Wait};
use super::{Confirm, Hollywood, LayerOpts, Section, frames, talk};
use crate::panel::data::MachineSummary;

/// The expanded band's height cap while floating: half the frame.
const EXPANDED_FRAC: u16 = 2;
/// Every exchange message clamps to at most 2 wrapped lines.
pub(crate) const WRAP_TALK_ROWS: u16 = 2;
/// Maximum talk entries shown in the focused HOLLYWOOD zone.
const MAX_FOCUSED_TALK: usize = super::rows::MAX_TALK_ENTRIES;

/// The band's height while expanded — the dock's seam (A5.2) reads
/// the same number before it picks its own `y`.
#[cfg(test)]
pub(crate) fn expanded_height(
    frame: Rect,
    machines: &[MachineSummary],
    status: &Status<'_>,
    hollywood: &Hollywood,
    opts: LayerOpts,
) -> u16 {
    expanded_extent(frame, machines, status, hollywood, opts).0
}

/// Both the band's internal height (`bar_h`) and its external bottom
/// drop-shadow height (always `0` for the docked Call Sheet).
pub(crate) fn expanded_extent(
    frame: Rect,
    machines: &[MachineSummary],
    status: &Status<'_>,
    hollywood: &Hollywood,
    opts: LayerOpts,
) -> (u16, u16) {
    let (waits, band_rows) =
        rows::band_rows_with_echoes(machines, status, opts.agent, &hollywood.reply_echoes);
    let p = plan(frame, &waits, &band_rows, hollywood, opts);
    (p.height(), 0)
}

/// The height cap in rows for `frame`.
pub(crate) fn band_cap(frame: Rect, mobile: bool) -> u16 {
    if mobile {
        frame.height.min(16)
    } else {
        (frame.height / EXPANDED_FRAC).max(12).min(frame.height)
    }
}

/// The plan for one layout pass — shared by the height read and the
/// draw so the dock's seam and the band itself never disagree.
pub(crate) fn plan(
    frame: Rect,
    waits: &[Wait<'_>],
    band_rows: &[Row<'_>],
    hollywood: &Hollywood,
    opts: LayerOpts,
) -> Plan {
    let cap = band_cap(frame, opts.mobile);
    let focus = hollywood.selected.as_ref().and_then(Section::focus_of);
    let selected_wait = hollywood
        .selected
        .as_ref()
        .and_then(|t| waits.iter().position(|w| &w.target() == t));
    let confirming = hollywood.confirm.as_ref().and_then(|c| match c {
        Confirm::Finish(key) => {
            let target = Target::Ready(key.clone());
            waits.iter().position(|w| w.target() == target)
        }
    });
    let talk_entries: Vec<&backlot_model::summary::Exchange> = band_rows
        .iter()
        .filter_map(|r| match r {
            Row::Talk(e) => Some(*e),
            _ => None,
        })
        .collect();
    let total_talk = talk_entries.len();
    let talk_want_wraps: Vec<u16> = talk_entries
        .iter()
        .map(|e| {
            let text_w = talk::talk_text_width(frame.width, &e.who);
            let rows = crate::hollywood::reader_wrap::wrap_plain(
                &talk::talk_body(e),
                text_w,
                WRAP_TALK_ROWS as usize,
            );
            (rows.len() as u16).saturating_sub(1)
        })
        .collect();
    let has_headline = band_rows.iter().any(|r| matches!(r, Row::Headline(_)));
    let has_live = band_rows.iter().any(|r| matches!(r, Row::Live(_)));
    let want_detail_extra: Vec<u16> = waits
        .iter()
        .map(|w| match w {
            Wait::Decision(item) => {
                (super::decision::detail_lines(item).len() as u16).saturating_sub(1)
            }
            _ => 0,
        })
        .collect();

    let mut plan = Plan {
        title: true,
        shown: waits.len(),
        details: vec![false; waits.len()],
        detail_extra: vec![0; waits.len()],
        hollywood_rule: false,
        headline: opts.agent && total_talk == 0 && has_headline,
        talk: u16::from(opts.agent && total_talk > 0),
        talk_wraps: vec![0; total_talk],
        live: false,
        tail_rows: 1,
        studios_perf: false,
        frames_h: 1,
        hints: true,
        bottom_edge: true,
    };

    fit_base(&mut plan, cap, focus, waits.len());
    expand_focused(
        &mut plan,
        cap,
        focus,
        waits,
        selected_wait,
        confirming,
        &want_detail_extra,
        opts.agent,
        has_headline,
        has_live,
        &talk_want_wraps,
    );
    plan
}

/// One fitted plan of the expanded docked Call Sheet.
#[derive(Debug, Default)]
pub(crate) struct Plan {
    pub title: bool,
    /// How many waiting rows the scroll window shows (`>= 1` while waits exist).
    pub shown: usize,
    /// Per waiting item: `true` draws its detail line.
    pub details: Vec<bool>,
    /// Additional detail lines beyond the first (used by multi-question design decisions).
    pub detail_extra: Vec<u16>,
    /// Crossbar above the HOLLYWOOD zone (`╟── HOLLYWOOD ──╢` or `╠══╡ … ╞══╣`).
    pub hollywood_rule: bool,
    /// Hollywood's headline row when shown inside the HOLLYWOOD zone.
    pub headline: bool,
    /// How many `Row::Talk` entries draw above compose.
    pub talk: u16,
    /// Extra wrapped row (`0` or `1`) per entry in `talk_entries`.
    pub talk_wraps: Vec<u16>,
    /// Hollywood's live progress row (focused HOLLYWOOD only).
    pub live: bool,
    /// The compose (`›`) / setup row — never drops when height >= 1.
    pub tail_rows: u16,
    /// Focused STUDIOS top perforation row `▄▄ ▄▄ … STUDIOS N …`.
    pub studios_perf: bool,
    /// `7`/`5`/`3` draws frames, `1` draws the 1-row chip strip, `0` none.
    pub frames_h: u16,
    pub hints: bool,
    /// Bottom marquee sill `╚══▪══…╡ ✦ ╞…╝` (no external shadow row).
    pub bottom_edge: bool,
}

impl Plan {
    /// Total extra wrap rows among the visible tail of `talk_wraps`.
    pub(crate) fn wrap_rows(&self) -> u16 {
        let len = self.talk_wraps.len();
        let skip = len.saturating_sub(self.talk as usize);
        self.talk_wraps[skip..].iter().sum()
    }

    /// Extra wrap row (`0` or `1`) for `talk_index` in `0..self.talk_wraps.len()`.
    pub(crate) fn talk_wrap_at(&self, talk_index: usize) -> u16 {
        let len = self.talk_wraps.len();
        let skip = len.saturating_sub(self.talk as usize);
        if talk_index < skip {
            0
        } else {
            self.talk_wraps.get(talk_index).copied().unwrap_or(0)
        }
    }

    /// Total band height in screen rows (external shadow is always 0).
    pub fn height(&self) -> u16 {
        let waits: u16 = self
            .details
            .iter()
            .enumerate()
            .take(self.shown)
            .map(|(i, d)| {
                1 + if *d {
                    1 + self.detail_extra.get(i).copied().unwrap_or(0)
                } else {
                    0
                }
            })
            .sum();
        // When the kept waiting item scrolls past `0..self.shown`, `draw_waiting`
        // slides the window to include it and drops an unselected item.
        let kept_outside = if self.shown > 0 && !self.details.iter().take(self.shown).any(|d| *d) {
            self.details
                .iter()
                .enumerate()
                .skip(self.shown)
                .find_map(|(i, d)| d.then(|| 1 + self.detail_extra.get(i).copied().unwrap_or(0)))
                .unwrap_or(0)
        } else {
            0
        };
        let talk = self.talk + self.wrap_rows();
        u16::from(self.title)
            + waits
            + kept_outside
            + u16::from(self.hollywood_rule)
            + u16::from(self.headline)
            + talk
            + u16::from(self.live)
            + self.tail_rows
            + u16::from(self.studios_perf)
            + self.frames_h
            + u16::from(self.hints)
            + u16::from(self.bottom_edge)
    }
}

/// Phase 1: shed base unfocused rows into `cap`. Compose (`tail_rows`)
/// and the selected waiting row (`shown >= 1`) never drop while `cap >= 2`,
/// and the bottom marquee sill (`bottom_edge`) sheds only when `cap < 3`.
pub(crate) fn fit_base(plan: &mut Plan, cap: u16, focus: Option<Section>, total_waits: usize) {
    let min_three = total_waits.min(3);
    let min_shown = usize::from(total_waits > 0);
    while plan.height() > cap {
        if focus != Some(Section::Hollywood) && plan.hollywood_rule {
            plan.hollywood_rule = false;
        } else if plan.hints {
            plan.hints = false;
        } else if focus != Some(Section::Hollywood) && plan.headline {
            plan.headline = false;
        } else if focus != Some(Section::Hollywood) && plan.talk > 0 {
            plan.talk = 0;
        } else if plan.shown > min_three {
            plan.shown -= 1;
        } else if focus != Some(Section::Studios) && plan.frames_h > 0 {
            plan.frames_h = 0;
        } else if plan.title {
            plan.title = false;
        } else if plan.shown > min_shown {
            plan.shown -= 1;
        } else if plan.headline {
            plan.headline = false;
        } else if plan.talk > 0 {
            plan.talk = 0;
        } else if plan.frames_h > 0 {
            plan.frames_h = 0;
        } else if plan.bottom_edge && cap < 3 {
            plan.bottom_edge = false;
        } else if plan.shown > 0 && cap < 2 {
            plan.shown = 0;
        } else if plan.bottom_edge {
            plan.bottom_edge = false;
        } else if plan.tail_rows > 0 && cap == 0 {
            plan.tail_rows = 0;
        } else {
            break;
        }
    }
}

/// Phase 2: expand only the focused zone into rows left under `cap`.
#[allow(clippy::too_many_arguments)]
fn expand_focused(
    plan: &mut Plan,
    cap: u16,
    focus: Option<Section>,
    waits: &[Wait<'_>],
    selected_wait: Option<usize>,
    confirming: Option<usize>,
    want_detail_extra: &[u16],
    agent: bool,
    has_headline: bool,
    has_live: bool,
    talk_want_wraps: &[u16],
) {
    let mut spare = cap.saturating_sub(plan.height());
    // An armed inline `finish studio? y/n` on a Ready item (even if
    // armed from STUDIOS) needs its 1-line ask visible.
    if let Some(ci) = confirming
        && Some(ci) != selected_wait
        && spare > 0
    {
        plan.details[ci] = true;
        spare -= 1;
    }

    match focus {
        Some(Section::Waiting) => {
            if let Some(sel) = selected_wait
                && spare > 0
                && (waits[sel].detailable() || confirming == Some(sel))
            {
                let want = 1 + want_detail_extra.get(sel).copied().unwrap_or(0);
                let give = spare.min(want);
                plan.details[sel] = true;
                plan.detail_extra[sel] = give - 1;
            }
        }
        Some(Section::Hollywood) if agent => {
            let mut rem = spare;
            let total_talk = talk_want_wraps.len();
            if has_live && rem > 0 {
                plan.live = true;
                rem -= 1;
            }
            if rem > 0 {
                plan.hollywood_rule = true;
                rem -= 1;
            }
            if total_talk > 0 && rem > 0 {
                let last = total_talk - 1;
                let w = talk_want_wraps[last].min(rem);
                plan.talk_wraps[last] = w;
                rem -= w;

                let oldest = total_talk.saturating_sub(MAX_FOCUSED_TALK);
                for idx in (oldest..last).rev() {
                    if rem == 0 {
                        break;
                    }
                    let extra = talk_want_wraps[idx];
                    if rem > extra {
                        plan.talk += 1;
                        plan.talk_wraps[idx] = extra;
                        rem -= 1 + extra;
                    } else {
                        plan.talk += 1;
                        rem -= 1;
                    }
                }
            }
            if has_headline && !plan.headline && rem > 0 {
                plan.headline = true;
            }
        }
        Some(Section::Studios) => {
            // `plan.frames_h` starts at `1` unless cap was < 2.
            if plan.frames_h == 1 {
                let avail = 1 + spare;
                if avail > frames::FRAME_H {
                    plan.studios_perf = true;
                    plan.frames_h = frames::FRAME_H;
                } else if avail > frames::FRAME_H_MID {
                    plan.studios_perf = true;
                    plan.frames_h = frames::FRAME_H_MID;
                } else if avail > frames::FRAME_H_MIN {
                    plan.studios_perf = true;
                    plan.frames_h = frames::FRAME_H_MIN;
                }
            }
        }
        _ => {}
    }

    // Unfocused `╟── HOLLYWOOD ──╢` rule appears only from leftover spare
    // rows when at least one waiting item sits above it.
    if focus != Some(Section::Hollywood)
        && agent
        && !waits.is_empty()
        && !plan.hollywood_rule
        && (plan.headline || plan.talk > 0 || plan.tail_rows > 0)
        && cap.saturating_sub(plan.height()) > 0
    {
        plan.hollywood_rule = true;
    }
}
