//! The band's height ladder: which rows draw and how tall the frames
//! are when the screen is short. The blanks go first, then the
//! unselected waiting rows' detail lines, then the frames fold into
//! the one-row chip strip, then the headline, then the hints; the
//! top sprocket only leaves under ten rows, and a waiting row is
//! never dropped — when they outgrow the room the window scrolls and
//! the `i/N` marker says so. The `tell` row never leaves either.
//!
//! `expanded_height` and `plan` live here too — the dock's seam and
//! the band's draw both read the same ladder.

use ratatui::layout::Rect;

use super::rows::{self, Row, Target};
use super::sheet::{Status, Wait};
use super::{Confirm, Hollywood, LayerOpts, frames, talk};
use crate::panel::data::MachineSummary;

/// The expanded band's height cap while floating: half the frame.
const EXPANDED_FRAC: u16 = 2;
/// Every exchange message clamps to at most 2 wrapped lines.
pub(crate) const WRAP_TALK_ROWS: u16 = 2;

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
/// drop-shadow height (`0` or `1`), so a beside Lot dock clears the shadow.
pub(crate) fn expanded_extent(
    frame: Rect,
    machines: &[MachineSummary],
    status: &Status<'_>,
    hollywood: &Hollywood,
    opts: LayerOpts,
) -> (u16, u16) {
    let (waits, band_rows) = rows::band_rows(machines, status, opts.agent);
    let p = plan(frame, &waits, &band_rows, hollywood, opts);
    (p.height(), u16::from(p.shadow))
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
    let cap = if opts.mobile {
        frame.height.min(16)
    } else {
        (frame.height / EXPANDED_FRAC).max(12).min(frame.height)
    };
    let selected_wait = hollywood
        .selected
        .as_ref()
        .and_then(|t| waits.iter().position(|w| &w.target() == t));
    let confirming = hollywood.confirm.as_ref().map(|c| match c {
        Confirm::Finish(key) => Target::Ready(key.clone()),
    });
    let talk_wraps: Vec<u16> = band_rows
        .iter()
        .filter_map(|r| match r {
            Row::Talk(e) => {
                let text_w = talk::talk_text_width(frame.width, &e.who);
                let rows = crate::hollywood::reader_wrap::wrap_plain(
                    &e.text,
                    text_w,
                    WRAP_TALK_ROWS as usize,
                );
                Some((rows.len() as u16).saturating_sub(1))
            }
            _ => None,
        })
        .collect();
    let total_talk = talk_wraps.len() as u16;
    let breathing = frame.height >= 50;
    let mut plan = Plan {
        top_sprocket: true,
        title: true,
        blank_1: breathing,
        studios_rule: true,
        frames_h: frames::FRAME_H,
        blank_2: breathing || waits.is_empty(),
        waiting_rule: !waits.is_empty(),
        headline: band_rows.iter().any(|r| matches!(r, Row::Headline(_))),
        details: waits
            .iter()
            .map(|w| w.detailable() || confirming.as_ref() == Some(&w.target()))
            .collect(),
        detail_extra: waits
            .iter()
            .map(|w| match w {
                Wait::Decision(item) => {
                    (super::decision::detail_lines(item).len() as u16).saturating_sub(1)
                }
                _ => 0,
            })
            .collect(),
        shown: waits.len(),
        hollywood_rule: opts.agent,
        setup_hairline: !opts.agent && breathing,
        talk: total_talk,
        talk_wraps,
        live: band_rows.iter().any(|r| matches!(r, Row::Live(_))),
        tail_rows: if waits.is_empty() { 2 } else { 1 }, // calm|+ tell
        hints: true,
        bottom_sprocket: true,
        shadow: !opts.no_shadow && frame.height >= 16,
    };
    fit(&mut plan, cap, frame.height, selected_wait);
    if opts.no_shadow || frame.height < 16 || plan.height() >= frame.height {
        plan.shadow = false;
    }
    plan
}

/// One fitted plan of the expanded band.
#[derive(Debug, Default)]
pub(crate) struct Plan {
    pub top_sprocket: bool,
    pub title: bool,
    pub blank_1: bool,
    pub studios_rule: bool,
    /// `7` draws the film frames, `1` the chip strip, `0` nothing.
    pub frames_h: u16,
    pub blank_2: bool,
    pub waiting_rule: bool,
    /// The headline row — dropped after zone rules and old talk.
    pub headline: bool,
    /// Per waiting item: `true` draws its detail line.
    pub details: Vec<bool>,
    /// Additional detail lines beyond the first (used by multi-question design decisions).
    pub detail_extra: Vec<u16>,
    /// How many waiting rows the scroll window shows — never fewer
    /// than `1` while items exist.
    pub shown: usize,
    pub hollywood_rule: bool,
    pub setup_hairline: bool,
    /// How many `Row::Talk` lines draw above the compose field.
    pub talk: u16,
    /// Extra wrapped line (`0` or `1`) per visible `Row::Talk`, oldest first.
    pub talk_wraps: Vec<u16>,
    /// Hollywood's live progress row under the newest user line.
    pub live: bool,
    /// The tell/setup/calm tail rows — never drops below `1`.
    pub tail_rows: u16,
    pub hints: bool,
    pub bottom_sprocket: bool,
    /// External 1-row bottom shadow drawn at `y = area.bottom()`.
    pub shadow: bool,
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

    /// The plan's internal height in rows (excluding the external bottom shadow).
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
        // When the selected item scrolls past `0..self.shown`, `draw_waiting`
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
        u16::from(self.top_sprocket)
            + u16::from(self.title)
            + u16::from(self.blank_1)
            + u16::from(self.studios_rule)
            + self.frames_h
            + u16::from(self.blank_2)
            + u16::from(self.waiting_rule)
            + u16::from(self.headline)
            + waits
            + kept_outside
            + u16::from(self.hollywood_rule)
            + u16::from(self.setup_hairline)
            + talk
            + u16::from(self.live)
            + self.tail_rows
            + u16::from(self.hints)
            + u16::from(self.bottom_sprocket)
    }
}

/// 10-step compression ladder into `height`:
/// 1. blanks / optional hairlines
/// 2. exchange 2-line wrap -> 1-line (oldest wrapped message first)
/// 3. unselected waiting details (`keep` keeps its detail)
/// 4. oldest visible `Row::Talk` lines (bumps `earlier` count on `HOLLYWOOD` rule)
/// 5. zone label rules (`HOLLYWOOD` -> `WAITING ON YOU` -> `STUDIOS`)
/// 6. non-duplicate `Row::Headline`
/// 7. 7-row frames -> 1-row chip strip, then `Row::Live`
/// 8. hint row
/// 9. waiting scroll window down to `1`
/// 10. external shadow -> top sprocket -> bottom sprocket -> chip strip (`0`) -> title
pub(crate) fn fit(plan: &mut Plan, height: u16, rows: u16, keep: Option<usize>) {
    while plan.height() > height {
        let skip = plan.talk_wraps.len().saturating_sub(plan.talk as usize);
        if plan.blank_1 {
            plan.blank_1 = false;
        } else if plan.blank_2 {
            plan.blank_2 = false;
        } else if plan.setup_hairline {
            plan.setup_hairline = false;
        } else if let Some(idx) = plan
            .talk_wraps
            .iter()
            .enumerate()
            .skip(skip)
            .find(|(_, w)| **w > 0)
            .map(|(i, _)| i)
        {
            plan.talk_wraps[idx] -= 1;
        } else if let Some(i) = plan
            .detail_extra
            .iter()
            .enumerate()
            .rfind(|(i, extra)| {
                **extra > 0 && plan.details.get(*i).copied().unwrap_or(false) && Some(*i) != keep
            })
            .map(|(i, _)| i)
        {
            plan.detail_extra[i] -= 1;
        } else if let Some(i) = plan
            .details
            .iter()
            .enumerate()
            .rfind(|(i, d)| **d && Some(*i) != keep)
            .map(|(i, _)| i)
        {
            plan.details[i] = false;
        } else if plan.talk > 0 {
            plan.talk -= 1;
        } else if plan.hollywood_rule {
            plan.hollywood_rule = false;
        } else if plan.waiting_rule {
            plan.waiting_rule = false;
        } else if plan.studios_rule {
            plan.studios_rule = false;
        } else if plan.headline {
            plan.headline = false;
        } else if plan.frames_h > 1 {
            plan.frames_h = 1;
        } else if plan.live {
            plan.live = false;
        } else if plan.hints {
            plan.hints = false;
        } else if let Some(i) = keep.filter(|&k| {
            plan.details.get(k).copied().unwrap_or(false)
                && plan.detail_extra.get(k).copied().unwrap_or(0) > 0
        }) {
            plan.detail_extra[i] -= 1;
        } else if plan.shown > usize::from(!plan.details.is_empty()) {
            plan.shown -= 1;
        } else {
            // Step 10 drops the external shadow row first, then tight chrome.
            plan.shadow = false;
            if plan.top_sprocket && (rows < 10 || plan.height() > height) {
                plan.top_sprocket = false;
            } else if plan.bottom_sprocket {
                plan.bottom_sprocket = false;
            } else if plan.frames_h > 0 {
                plan.frames_h = 0;
            } else if plan.title {
                plan.title = false;
            } else if plan.tail_rows > 1 {
                plan.tail_rows -= 1;
            } else {
                break;
            }
        }
    }
}
