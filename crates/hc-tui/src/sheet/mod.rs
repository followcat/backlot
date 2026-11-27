//! The Task & Delivery Sheet (S2 §3): the `v` modal over the dock —
//! one task's overview/casts, its deliveries and reviews, and the
//! studio's actor records. The summary's task projection shows at
//! once; `backlot status --json` and `backlot records --json` fill
//! the ledger detail in behind it, fetched off the event loop on the
//! studio's own machine — the same runner the New Studio dialog uses.
//! All state is client-local: nothing here writes to a document.

mod draw;
mod lines;
mod lines_ledger;

pub(crate) use draw::{SheetRender, render_with_shadow};
pub(crate) use lines::lines_for;

use backlot_model::state::StudioState;

use crate::studio::StudioKey;

/// The sheet's tab ids — `Tab` cycles, `1`–`3` jump (S2 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SheetTab {
    /// `1 Overview & Casts`: title, status, acceptance, the
    /// dependency chain and every cast entry.
    Overview,
    /// `2 Deliveries & Reviews`: every delivery with its commit
    /// range, the actor-reported and stage-run checks, and every
    /// review with verdict, conclusion and findings.
    Deliveries,
    /// `3 Actor Records`: the `records --json` read for this studio.
    Records,
}

impl SheetTab {
    /// The tab bar's labels, in order.
    pub(crate) const NAMES: [&'static str; 3] = [
        "1 Overview & Casts",
        "2 Deliveries & Reviews",
        "3 Actor Records",
    ];

    /// `Tab` steps forward, wrapping.
    pub(crate) fn next(self) -> SheetTab {
        match self {
            SheetTab::Overview => SheetTab::Deliveries,
            SheetTab::Deliveries => SheetTab::Records,
            SheetTab::Records => SheetTab::Overview,
        }
    }

    /// `Shift+Tab` steps backward, wrapping.
    pub(crate) fn prev(self) -> SheetTab {
        match self {
            SheetTab::Overview => SheetTab::Records,
            SheetTab::Deliveries => SheetTab::Overview,
            SheetTab::Records => SheetTab::Deliveries,
        }
    }

    /// `1`–`3` jump straight to a tab.
    pub(crate) fn of(digit: char) -> Option<SheetTab> {
        match digit {
            '1' => Some(SheetTab::Overview),
            '2' => Some(SheetTab::Deliveries),
            '3' => Some(SheetTab::Records),
            _ => None,
        }
    }
}

/// A report the sheet opened with `o` — the file's lines, scrolled.
/// The delivery report and each stage-check log read through
/// `backlot report show` (G3), locally or on the studio's machine.
#[derive(Debug, Default)]
pub(crate) struct SheetView {
    /// The footer/title text — `reports/d1.md` and friends.
    pub title: String,
    pub lines: Vec<String>,
    pub raw_body: String,
    pub scroll: usize,
    pub meta: crate::hollywood::ReaderMeta,
    pub tell: crate::hollywood::InlineTellState,
}

/// The whole client-local sheet state.
pub(crate) struct Sheet {
    /// The studio the sheet reads — a local `None` machine or the
    /// machine-qualified key for a remote one.
    pub studio: StudioKey,
    /// The task the sheet opened on; `None` shows the studio's own
    /// overview (the owed step and every cast).
    pub task: Option<String>,
    pub tab: SheetTab,
    /// Selected content line — a file-carrying row on the Deliveries
    /// tab is the one `o` reads.
    pub sel: usize,
    /// First line drawn below the tab bar.
    pub scroll: usize,
    /// The open generation: opening again or switching studios bumps
    /// it, so a late reply lands dead (S2 §3).
    pub epoch: u64,
    /// `status --json` once it answered — the ledger detail the
    /// sheet fills in.
    pub status: Option<Result<StudioState, String>>,
    /// `records --json` once it answered.
    pub records: Option<Result<Vec<serde_json::Value>, String>>,
    /// The `o`-opened report — drawn over the tab body.
    pub view: Option<SheetView>,
    /// The fetch generation for an `o` read in flight — a second `o`
    /// or a close retires the earlier one.
    pub view_epoch: u64,
}

impl Sheet {
    pub(crate) fn new(studio: StudioKey, task: Option<String>, epoch: u64) -> Sheet {
        Sheet {
            studio,
            task,
            tab: SheetTab::Overview,
            sel: 0,
            scroll: 0,
            epoch,
            status: None,
            records: None,
            view: None,
            view_epoch: 0,
        }
    }

    /// Whether any ledger detail is still in flight — the body's
    /// `loading ledger details…` line shows until both reads answer.
    pub(crate) fn loading(&self) -> bool {
        self.status.is_none() || self.records.is_none()
    }
}
