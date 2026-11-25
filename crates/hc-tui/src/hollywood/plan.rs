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
use super::{Confirm, Hollywood, LayerOpts, band, frames};
use crate::panel::data::MachineSummary;

/// The expanded band's height cap while floating: half the frame.
const EXPANDED_FRAC: u16 = 2;
/// The most rows the newest Hollywood line wraps to on a wide band.
pub(crate) const WRAP_TALK_ROWS: u16 = 4;
/// The cap on a narrow/mobile band.
pub(crate) const WRAP_TALK_ROWS_MOBILE: u16 = 2;

/// The band's height while expanded — the dock's seam (A5.2) reads
/// the same number before it picks its own `y`.
pub(crate) fn expanded_height(
    frame: Rect,
    machines: &[MachineSummary],
    status: &Status<'_>,
    hollywood: &Hollywood,
    opts: LayerOpts,
) -> u16 {
    let (waits, band_rows) = rows::band_rows(machines, status, opts.agent);
    plan(frame, &waits, &band_rows, hollywood, opts).height()
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
    // The selected waiting item keeps its detail through the ladder.
    let selected_wait = hollywood
        .selected
        .as_ref()
        .and_then(|t| waits.iter().position(|w| &w.target() == t));
    // The armed `finish?` confirm borrows its row's detail line —
    // keep it in the plan even though a ready row is not detailable.
    let confirming = hollywood.confirm.as_ref().map(|c| match c {
        Confirm::Finish(key) => Target::Ready(key.clone()),
    });
    let talk_lines: Vec<&backlot_model::summary::Exchange> = band_rows
        .iter()
        .filter_map(|r| match r {
            Row::Talk(e) => Some(*e),
            _ => None,
        })
        .collect();
    let wrap_seq = talk_lines.iter().rposition(|e| e.who == "hollywood");
    let mut plan = Plan {
        top_sprocket: true,
        title: true,
        blank_1: true,
        frames_h: frames::FRAME_H,
        blank_2: true,
        headline: band_rows.iter().any(|r| matches!(r, Row::Headline(_))),
        details: waits
            .iter()
            .map(|w| w.detailable() || confirming.as_ref() == Some(&w.target()))
            .collect(),
        shown: waits.len(),
        talk: band_rows
            .iter()
            .filter(|r| matches!(r, Row::Talk(_) | Row::Fold { .. }))
            .count() as u16,
        wrap: 0,
        wrap_keep: wrap_seq.map(|p| (talk_lines.len() - p) as u16).unwrap_or(0),
        wrap_seq: wrap_seq.map(|p| talk_lines[p].seq),
        live: band_rows.iter().any(|r| matches!(r, Row::Live(_))),
        tail_rows: if waits.is_empty() { 2 } else { 1 }, // calm|+ tell
        hints: true,
        bottom_sprocket: true,
    };
    // The newest Hollywood line wraps to a few rows at the band's own
    // width — never more than the mobile or wide cap, and never past
    // the band's height.
    if let Some(line) = plan.wrap_seq.and_then(|seq| {
        band_rows.iter().find_map(|r| match r {
            Row::Talk(e) if e.seq == seq => Some(*e),
            _ => None,
        })
    }) {
        let text_w = band::talk_text_width(frame.width, &line.who);
        let cap = if opts.mobile {
            WRAP_TALK_ROWS_MOBILE
        } else {
            WRAP_TALK_ROWS
        };
        let rows = crate::hollywood::reader_wrap::wrap_plain(&line.text, text_w, cap as usize);
        plan.wrap = (rows.len() as u16).saturating_sub(1);
    }
    fit(&mut plan, cap, frame.height, selected_wait);
    plan
}

/// One fitted plan of the expanded band.
#[derive(Debug, Default)]
pub(crate) struct Plan {
    pub top_sprocket: bool,
    pub title: bool,
    pub blank_1: bool,
    /// `7` draws the film frames, `1` the chip strip, `0` nothing.
    pub frames_h: u16,
    pub blank_2: bool,
    /// The headline row — a flag now, not a band entry, so the
    /// ladder drops it before the hints.
    pub headline: bool,
    /// Per waiting item: `true` draws its detail line.
    pub details: Vec<bool>,
    /// How many waiting rows the scroll window shows — never fewer
    /// than `1` while items exist; the rest scroll (`i/N`).
    pub shown: usize,
    /// How many exchange lines the dialog shows above the input —
    /// the oldest lines leave first when the band runs short.
    pub talk: u16,
    /// The seq of the newest Hollywood line that wraps, if any.
    pub wrap_seq: Option<u64>,
    /// How many talk rows from the wrapped line to the newest — the
    /// wrap only draws while that many talk rows survive the ladder.
    pub wrap_keep: u16,
    /// The extra rows that line's wrap adds beyond its first row.
    pub wrap: u16,
    /// Hollywood's live progress row under the newest user line.
    pub live: bool,
    /// The tell/setup/calm tail rows — always drawn, `calm` or
    /// `setup` standing in for the waiting list.
    pub tail_rows: u16,
    pub hints: bool,
    pub bottom_sprocket: bool,
}

impl Plan {
    /// The wrapped line's extra rows, counted only while the line is
    /// still drawn (the ladder has not dropped `talk` past it).
    pub(crate) fn wrap_rows(&self) -> u16 {
        if self.wrap_keep > 0 && self.talk >= self.wrap_keep {
            self.wrap
        } else {
            0
        }
    }

    /// The plan's full height in rows. `shown` waiting rows each
    /// take one line plus their detail when kept; the wrapped newest
    /// Hollywood line adds its extra rows while it is still drawn.
    pub fn height(&self) -> u16 {
        let waits: u16 = self
            .details
            .iter()
            .take(self.shown)
            .map(|d| 1 + u16::from(*d))
            .sum();
        let talk = self.talk + self.wrap_rows();
        u16::from(self.top_sprocket)
            + u16::from(self.title)
            + u16::from(self.blank_1)
            + self.frames_h
            + u16::from(self.blank_2)
            + u16::from(self.headline)
            + waits
            + talk
            + u16::from(self.live)
            + self.tail_rows
            + u16::from(self.hints)
            + u16::from(self.bottom_sprocket)
    }
}

/// Fit `plan` into `height`, compressing in the spec's order: the
/// blanks, the unselected details (`keep` is the selected item's
/// index — its detail stays), the frames to chips, the headline,
/// the hints; under ten rows the top sprocket goes too, and the
/// scroll window shrinks before the bottom sprocket ever leaves.
/// Every branch frees at least one row, so the loop terminates.
pub(crate) fn fit(plan: &mut Plan, height: u16, rows: u16, keep: Option<usize>) {
    while plan.height() > height {
        if plan.blank_1 {
            plan.blank_1 = false;
        } else if plan.blank_2 {
            plan.blank_2 = false;
        } else if let Some(i) = plan
            .details
            .iter()
            .enumerate()
            .rfind(|(i, d)| **d && Some(*i) != keep)
            .map(|(i, _)| i)
        {
            plan.details[i] = false;
        } else if plan.frames_h > 1 {
            plan.frames_h = 1;
        } else if plan.headline {
            plan.headline = false;
        } else if plan.hints {
            plan.hints = false;
        } else if plan.wrap_rows() > 0 {
            // The wrap gives its extra rows back, down to the one
            // line, before a talk line or any waiting row is dropped.
            plan.wrap -= 1;
        } else if plan.talk > 0 {
            plan.talk -= 1;
        } else if plan.live {
            plan.live = false;
        } else if rows < 10 && plan.top_sprocket {
            plan.top_sprocket = false;
        } else if plan.shown > usize::from(!plan.details.is_empty()) {
            plan.shown -= 1;
        } else if plan.bottom_sprocket {
            plan.bottom_sprocket = false;
        } else if plan.top_sprocket {
            plan.top_sprocket = false;
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
