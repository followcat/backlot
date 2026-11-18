//! The band's height ladder: which rows draw and how tall the frames
//! are when the screen is short. The blanks go first, then the
//! unselected waiting rows' detail lines, then the frames fold into
//! the one-row chip strip, then the headline, then the hints; the
//! top sprocket only leaves under ten rows, and a waiting row is
//! never dropped — when they outgrow the room the window scrolls and
//! the `i/N` marker says so. The `tell` row never leaves either.

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
    /// The tell/setup/calm tail rows — always drawn, `calm` or
    /// `setup` standing in for the waiting list.
    pub tail_rows: u16,
    pub hints: bool,
    pub bottom_sprocket: bool,
}

impl Plan {
    /// The plan's full height in rows. `shown` waiting rows each
    /// take one line plus their detail when kept.
    pub fn height(&self) -> u16 {
        let waits: u16 = self
            .details
            .iter()
            .take(self.shown)
            .map(|d| 1 + u16::from(*d))
            .sum();
        u16::from(self.top_sprocket)
            + u16::from(self.title)
            + u16::from(self.blank_1)
            + self.frames_h
            + u16::from(self.blank_2)
            + u16::from(self.headline)
            + waits
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
