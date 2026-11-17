//! The band's height ladder: which rows draw and how tall the frames
//! are when the screen is short. The blank rows go first, then the
//! hints, then the headline, then the cards, then the stage line; the
//! frames and the sprockets are the last to go, and the input line
//! never does — nothing here can panic, down to a one-row frame.

use super::frames;
use super::rows::Row;

/// One fitted plan of the expanded band.
#[derive(Debug, Default)]
pub(crate) struct Plan {
    pub top_sprocket: bool,
    pub title: bool,
    pub blank_1: bool,
    pub frames_h: u16,
    pub blank_2: bool,
    /// The band rows to draw, as indices into `band_rows`.
    pub band: Vec<usize>,
    pub hints: bool,
    pub bottom_sprocket: bool,
}

impl Plan {
    /// The plan's full height in rows.
    pub fn height(&self) -> u16 {
        u16::from(self.top_sprocket)
            + u16::from(self.title)
            + u16::from(self.blank_1)
            + self.frames_h
            + u16::from(self.blank_2)
            + self.band.len() as u16
            + u16::from(self.hints)
            + u16::from(self.bottom_sprocket)
    }
}

/// Fit `plan` into `height`, dropping rows in the ladder's order.
/// Every branch frees at least one row or shrinks the frames, so the
/// loop terminates.
pub(crate) fn fit(plan: &mut Plan, band: &[Row<'_>], height: u16, mobile: bool) {
    while plan.height() > height {
        if plan.blank_1 {
            plan.blank_1 = false;
        } else if plan.blank_2 {
            plan.blank_2 = false;
        } else if plan.hints {
            plan.hints = false;
        } else if let Some(pos) = plan
            .band
            .iter()
            .position(|&i| matches!(band[i], Row::Headline(_)))
        {
            plan.band.remove(pos);
        } else if let Some(pos) = plan
            .band
            .iter()
            .rposition(|&i| matches!(band[i], Row::Card(_)))
        {
            plan.band.remove(pos);
        } else if let Some(pos) = plan
            .band
            .iter()
            .position(|&i| matches!(band[i], Row::StageDown))
        {
            plan.band.remove(pos);
        } else if plan.bottom_sprocket {
            plan.bottom_sprocket = false;
        } else if plan.top_sprocket {
            plan.top_sprocket = false;
        } else if plan.frames_h > if mobile { frames::FRAME_H } else { 0 } {
            plan.frames_h = plan
                .frames_h
                .saturating_sub(if mobile { frames::FRAME_H } else { 1 });
        } else if plan.frames_h > 0 {
            plan.frames_h = 0;
        } else if plan.title {
            plan.title = false;
        } else {
            break;
        }
    }
}
