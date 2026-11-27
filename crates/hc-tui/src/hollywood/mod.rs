//! The Hollywood layer (H4): a client-side floating view of the
//! `backlot` summary document's `hollywood` object — hidden, the lot
//! pill's collapsed face (the chrome draws it), or a band that drops
//! down from the top. It never reserves columns or rows: the pane
//! layout is never recomputed for it and no program is resized. All
//! the data it shows comes from the document; everything it changes
//! goes through `backlot` commands spawned off the event loop.
//!
//! `--plain` keeps the layer `Hidden`: no cells, no consumed key, no
//! timer — the frame is byte-identical to herdr (D15, D6).

mod architrave;
mod band;
mod band_detail;
mod band_lower;
mod band_wait;
mod chips;
mod compose_chip;
pub(crate) mod decision;
mod draw;
mod draw_chips;
mod draw_lower;
mod draw_sections;
mod frames;
mod frames_draw;
mod handoff_ui;
mod hints;
mod items;
mod keys;
mod nav_rows;
mod new_frame;
mod plan;
mod reader_banner;
mod reader_chips;
mod reader_chrome;
mod reader_compose;
mod reader_design;
mod reader_frame;
mod reader_md;
mod reader_meta;
mod reader_wrap;
mod rows;
mod sheet;
mod talk;
mod title;
mod view;
mod zone;

pub(crate) use draw::{HollywoodRender, render};
pub(crate) use frames::reveal_frame_scroll;
pub(crate) use handoff_ui::{
    PillSwitchState, handoff_phase, handoff_target_short, is_handoff_active, is_quota_or_error,
    pill_switch_state, rollback_banner,
};
#[cfg(test)]
pub(crate) use hints::for_row;
pub(crate) use items::{WaitBucket, chips_of, default_chip_index};
pub(crate) use keys::{CallSheetKeys, resolve_toggle};
pub(crate) use nav_rows::nav_band_targets;
pub(crate) use plan::expanded_extent;
#[cfg(test)]
pub(crate) use plan::expanded_height;
pub(crate) use reader_compose::{InlineTellState, ReaderComposeOpts, reader_tell_target};
pub(crate) use reader_design::version_from_file;
#[cfg(test)]
pub(crate) use reader_md::{DesignSection, parse_design_sections};
#[cfg(test)]
pub(crate) use reader_meta::age_label;
pub(crate) use reader_meta::{
    ReaderKind, ReaderMeta, ReaderOAction, ReaderOrigin, find_studio_role, hollywood_state_word,
    role_state_word,
};
pub(crate) use reader_wrap::{render_body_lines_with_sections, wrap_plain};
#[cfg(test)]
pub(crate) use rows::band_rows;
pub(crate) use rows::{
    HollywoodActivity, MAX_TALK_ENTRIES, Target, band_rows_with_echoes, frames, hollywood_activity,
    hollywood_asking, hollywood_reply_max_seq, hollywood_unread_count, local_summary, on_set,
};
pub(crate) use sheet::{Conn, Wait, conn_of, machine_label, status_of, waiting_items};
pub(crate) use view::{CardView, ReaderRenderOpts, render_view_with_opts};
#[cfg(test)]
pub(crate) use view::{render_view, render_view_at, render_view_with_compose};

/// Outer horizontal gutter keeping vertical rails (`col 0`, `col width - 1`)
/// one air column outside zone content across the whole Call Sheet band.
pub(crate) const MARGIN: u16 = 2;

use crate::chrome::text_editor::TextEditor;

/// How the layer sits on screen. `Hidden` draws nothing — the
/// default, and the only state when `[hollywood]` is absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum LayerMode {
    /// Not drawn; no key is consumed; nothing is armed.
    #[default]
    Hidden,
    /// One row at the right end of the top row, sized to its content.
    Collapsed,
    /// The drop-down panel — the scrollable view with the input line.
    Expanded,
}

/// The three keyboard-navigation and focus zones inside the expanded
/// Call Sheet, in top-to-bottom screen order (`Tab` cycles them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Section {
    /// Top zone: items waiting on the user.
    Waiting,
    /// Middle zone: Hollywood headline, exchange, live row, and compose.
    #[default]
    Hollywood,
    /// Bottom zone: studio strip or expanded film strip + `+ new`.
    Studios,
}

impl Section {
    /// Which focus zone owns `target`.
    pub(crate) fn of(target: &Target) -> Section {
        match target {
            Target::Prompt(..)
            | Target::Role(..)
            | Target::Tell(_)
            | Target::Upgrade(_)
            | Target::Halted(_)
            | Target::Decision(_)
            | Target::Stage(..)
            | Target::Card(_)
            | Target::OutwardCard(..)
            | Target::Ready(_)
            | Target::Hollywood => Section::Waiting,
            Target::Headline
            | Target::Talk(_)
            | Target::HollywoodLive
            | Target::Input
            | Target::Setup
            | Target::Calm => Section::Hollywood,
            Target::Title | Target::Studio(_) | Target::NewStudio => Section::Studios,
        }
    }

    /// Focus zone expanded when `target` is selected (`Title` has no zone expansion).
    pub(crate) fn focus_of(target: &Target) -> Option<Section> {
        match target {
            Target::Title | Target::Calm => None,
            other => Some(Self::of(other)),
        }
    }
}

/// What the renderer needs from the app: whether a `[hollywood]` table
/// configures the agent, and the narrow-width cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct LayerOpts {
    /// An agent is configured: the headline, the cards, the `tell
    /// hollywood` line and the animated role state belong to it.
    /// Without one the bar still exists, in its no-agent form (U7).
    pub agent: bool,
    /// The narrow-width full-screen cut.
    pub mobile: bool,
    /// Suppress active selection highlights, `▌` bars, active `›`
    /// `active_row_bg`, and edit cursors while a modal covers it (`R3-M2`).
    pub suppress_highlights: bool,
    /// Disable the external 1-row bottom drop shadow (`[ui] popup_shadow = false`).
    pub no_shadow: bool,
}

/// Which waiting moment armed Compose's reply target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReplyTargetKind {
    Ask,
    Design,
}

/// The whole client-local layer state (D21): which of the three
/// states it is in, the selection, the frame window, the card scroll
/// and the input line's text. Everything on screen is read from the
/// document.
pub(crate) struct Hollywood {
    pub mode: LayerMode,
    /// The selected frame or row's stable identity — frames and rows
    /// rebuild each frame, so selection survives document updates by
    /// target, not index.
    pub selected: Option<Target>,
    /// Active keyboard-navigation section (`Upper` frames vs `Lower` band).
    pub section: Section,
    /// Last focused frame in `Upper`, restored when switching back.
    pub last_upper: Option<Target>,
    /// Last focused row in `Lower`, restored when switching back.
    pub last_lower: Option<Target>,
    /// Focused action chip within the selected `Wait` row (`None` uses
    /// `items::default_chip_index` so `allow y` / `show ↵` starts focused).
    pub chip_index: Option<usize>,
    /// The first frame drawn: the frames scroll sideways when more
    /// exist than fit.
    pub frame_scroll: usize,
    /// First card row drawn.
    pub scroll: usize,
    /// The input line's editor — `editing` is the latch that sends it
    /// keys (`i` or a click opens it, `Esc` leaves it).
    pub input: TextEditor,
    pub editing: bool,
    /// `Some((studio, role))` while the compose line is an in-place
    /// reply to that role's question instead of a line to Hollywood.
    pub reply_to: Option<(crate::studio::StudioKey, String)>,
    /// The waiting moment that armed `reply_to` (`Ask` rows and design
    /// sign-offs invalidate differently).
    pub reply_kind: Option<ReplyTargetKind>,
    /// Client-local echo rows for replies sent from this sheet. They
    /// use high synthetic seqs so ledger rows never share their target.
    pub reply_echoes: Vec<backlot_model::summary::Exchange>,
    pub next_reply_echo_seq: u64,
    pub history: crate::chrome::history::ComposeHistory,
    /// A card's "Show me" read — `card show <id>` output in a
    /// read-only box over the layer.
    pub view: Option<CardView>,
    /// The armed one-key confirm — S1's inline `finish` ask.
    pub confirm: Option<Confirm>,
    /// The running action's row label — the row draws `…` until the
    /// result lands.
    pub pending: Option<String>,
    /// Spinner frame counter synced from `panel.spin`.
    pub spin: usize,
    /// Highest Hollywood reply `seq` viewed by the user (`None` until
    /// seeded from the first received summary document).
    pub seen_hollywood_seq: Option<u64>,
    /// One-line start command when `backlot-cat` is present while the server
    /// terminal is gone.
    pub server_stopped_cmd: Option<String>,
    /// Currently active main-pane studio (renders `▸` prefix on unselected frame).
    pub active_studio: Option<crate::studio::StudioKey>,
    /// Optional clock override (`ms` since epoch) used by tests for exchange age labels.
    pub now_ms: Option<u64>,
}

/// The inline `y/n` confirm S1 arms on a ready row: which studio
/// `f` would finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Confirm {
    /// `backlot studio finish` on this studio — `y` runs it.
    Finish(crate::studio::StudioKey),
}

impl Hollywood {
    pub fn new() -> Hollywood {
        Hollywood {
            mode: LayerMode::Hidden,
            selected: None,
            section: Section::default(),
            last_upper: None,
            last_lower: None,
            chip_index: None,
            frame_scroll: 0,
            scroll: 0,
            input: TextEditor::new("", false),
            editing: false,
            reply_to: None,
            reply_kind: None,
            reply_echoes: Vec::new(),
            next_reply_echo_seq: u64::MAX - 1_000_000,
            history: Default::default(),
            view: None,
            confirm: None,
            pending: None,
            spin: 0,
            seen_hollywood_seq: None,
            server_stopped_cmd: None,
            active_studio: None,
            now_ms: None,
        }
    }

    /// Drop the one-moment reply target and its source together.
    pub(crate) fn clear_reply(&mut self) {
        self.reply_to = None;
        self.reply_kind = None;
    }

    /// Drawn at all — the pill face counts.
    pub fn visible(&self) -> bool {
        self.mode != LayerMode::Hidden
    }

    /// The expanded panel owns its keys like the studio panel does.
    pub fn expanded(&self) -> bool {
        self.mode == LayerMode::Expanded
    }
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
