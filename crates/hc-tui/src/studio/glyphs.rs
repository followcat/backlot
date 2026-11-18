//! The shared glyph and colour vocabulary (interaction spec §1): one
//! glyph and one palette token per state on every surface — the lot
//! pill, the Hollywood frames, the studio dock and the sidebar cast.
//! Only single-width codepoints; only `working` animates.

use backlot_model::event::{PERMISSION_PROMPT, Priority};
use backlot_model::state::TaskStatus;
use backlot_model::summary::{RoleSummary, Work};
use ratatui::style::{Color, Modifier, Style};

use crate::chrome::palette::Palette;

/// Spinner frames for a `working` mark (~8.3 fps at the 120 ms
/// tick). Every other state is static.
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// The lot pill's own mark — the studio header's pill uses it.
#[allow(dead_code)]
pub(crate) const LOT: &str = "✦";
/// The pill's all-clear dot and a live health mark.
#[allow(dead_code)]
pub(crate) const LIVE: &str = "●";
/// The selected row's left-edge bar.
pub(crate) const SELECTION_BAR: &str = "▌";
/// The mark the studio header and the dock put before the studio
/// whose terminal is currently focused (A7.2).
pub(crate) const FOCUSED_STUDIO: &str = "▸";

/// A person's state as the whole interaction layer shows it — the
/// same role reads the same on every surface (A11.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Person {
    /// Actively producing — the only animated glyph (`⠋`..`⠏`,
    /// `◐` in static contexts).
    Working,
    /// Live and quiet — `○` `overlay1`, never green.
    Idle,
    /// Wanted but held for a runner slot — `◌` `peach`.
    Slot,
    /// A prompt only the user may answer — `×` `red` `BOLD`.
    UserPrompt,
    /// A prompt the director answers — `×` `peach`.
    DirPrompt,
    /// Screen stalled or a step overdue — `!` `peach`.
    Stalled,
    /// A recorded `StopFailure` — `!` `red` `BOLD`.
    Failed,
    /// The terminal vanished — `!` `red` `BOLD`.
    Lost,
    /// Closed with a resumable session — `✓` `teal`.
    Resumable,
    /// Closed, nothing to reopen — `✓` `overlay0`.
    Closed,
}

impl Person {
    /// A role row → its glyph state. `closed`/`lost`/held override the
    /// sampled screen; an open attention then a recorded failure then
    /// the sample decide the rest.
    pub(crate) fn of(role: &RoleSummary) -> Person {
        match role.state.as_str() {
            "closed" => {
                return if role.resumable {
                    Person::Resumable
                } else {
                    Person::Closed
                };
            }
            "lost" => return Person::Lost,
            "waiting" => return Person::Slot,
            _ => {}
        }
        if let Some(attention) = role.attention.as_deref() {
            return if role.attention_user || attention.ends_with("needs the user") {
                Person::UserPrompt
            } else if attention.starts_with(PERMISSION_PROMPT) {
                Person::DirPrompt
            } else {
                // `stalled`, `cannot resume` — something owes a nudge.
                Person::Stalled
            };
        }
        if role.failed.is_some() {
            return Person::Failed;
        }
        match role.work {
            Some(Work::Working) => Person::Working,
            // A blocked screen with no attention row: the stage's own
            // rule — a non-actor's prompt is the user's (D9).
            Some(Work::Blocked) => {
                if role.task.is_some() || role.role.starts_with("actor-") {
                    Person::DirPrompt
                } else {
                    Person::UserPrompt
                }
            }
            _ => Person::Idle,
        }
    }

    /// The mark's symbol for spinner frame `spin`.
    pub(crate) fn glyph(self, spin: usize) -> &'static str {
        match self {
            Person::Working => SPINNER[spin % SPINNER.len()],
            Person::Idle => "○",
            Person::Slot => "◌",
            Person::UserPrompt | Person::DirPrompt => "×",
            Person::Stalled | Person::Failed | Person::Lost => "!",
            Person::Resumable | Person::Closed => "✓",
        }
    }

    /// The glyph for a surface that never animates — `working` reads
    /// `◐` (the spec's static/reduced-motion form).
    pub(crate) fn still(self) -> &'static str {
        match self {
            Person::Working => "◐",
            _ => self.glyph(0),
        }
    }

    /// The mark's palette token.
    pub(crate) fn color(self, palette: &Palette) -> Color {
        match self {
            Person::Working => palette.yellow,
            Person::Idle => palette.overlay1,
            Person::Slot | Person::DirPrompt | Person::Stalled => palette.peach,
            Person::UserPrompt | Person::Failed | Person::Lost => palette.red,
            Person::Resumable => palette.teal,
            Person::Closed => palette.overlay0,
        }
    }

    /// The mark's full style — `red` marks of a prompt or a failure
    /// are bold.
    pub(crate) fn style(self, palette: &Palette) -> Style {
        let style = Style::default().fg(self.color(palette));
        match self {
            Person::UserPrompt | Person::Failed | Person::Lost => {
                style.add_modifier(Modifier::BOLD)
            }
            _ => style,
        }
    }

    /// The state word shown at a row's right edge.
    pub(crate) fn word(self) -> &'static str {
        match self {
            Person::Working => "working",
            Person::Idle => "idle",
            Person::Slot => "slot",
            Person::UserPrompt => "USER",
            Person::DirPrompt => "dir",
            Person::Stalled => "stalled",
            Person::Failed => "failed",
            Person::Lost => "lost",
            Person::Resumable | Person::Closed => "closed",
        }
    }
}

/// A task's glyph — defined and tested now, drawn from S2 on.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Task {
    /// `created` or waiting on an open dependency — `·` `overlay0`.
    Open,
    /// `running` — `◐` `yellow`.
    Running,
    /// `delivered`, awaiting check/review — `◈` `blue`.
    Review,
    /// `accepted` clean — `✓` `green`.
    Accepted,
    /// `partially_accepted` — `✓` `teal`.
    Partial,
    /// `paused` — `=` `overlay1`.
    Paused,
    /// `blocked` — `×` `red`.
    Blocked,
    /// `at_cap` — `×` `peach`.
    AtCap,
    /// `abandoned`/`taken_over` — `-` `overlay0`.
    Abandoned,
}

#[allow(dead_code)]
impl Task {
    /// The ledger status → its glyph state.
    pub(crate) fn of(status: TaskStatus) -> Task {
        match status {
            TaskStatus::Created => Task::Open,
            TaskStatus::Running => Task::Running,
            TaskStatus::Delivered => Task::Review,
            TaskStatus::Accepted => Task::Accepted,
            TaskStatus::PartiallyAccepted => Task::Partial,
            TaskStatus::Paused => Task::Paused,
            TaskStatus::Blocked => Task::Blocked,
            TaskStatus::AtCap => Task::AtCap,
            TaskStatus::Abandoned | TaskStatus::TakenOver => Task::Abandoned,
        }
    }

    /// The task's symbol — never animated.
    pub(crate) fn glyph(self) -> &'static str {
        match self {
            Task::Open => "·",
            Task::Running => "◐",
            Task::Review => "◈",
            Task::Accepted | Task::Partial => "✓",
            Task::Paused => "=",
            Task::Blocked | Task::AtCap => "×",
            Task::Abandoned => "-",
        }
    }

    /// The task's palette token.
    pub(crate) fn color(self, palette: &Palette) -> Color {
        match self {
            Task::Open | Task::Abandoned => palette.overlay0,
            Task::Running => palette.yellow,
            Task::Review => palette.blue,
            Task::Accepted => palette.green,
            Task::Partial => palette.teal,
            Task::Paused => palette.overlay1,
            Task::Blocked => palette.red,
            Task::AtCap => palette.peach,
        }
    }
}

/// A task's priority flag — `↑`/`↓` after its id, `normal` draws
/// nothing (D11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PriorityMark {
    High,
    Low,
}

impl PriorityMark {
    /// The priority's mark, or `None` for `normal`.
    pub(crate) fn of(priority: Priority) -> Option<PriorityMark> {
        match priority {
            Priority::High => Some(PriorityMark::High),
            Priority::Low => Some(PriorityMark::Low),
            Priority::Normal => None,
        }
    }

    /// The flag's symbol.
    pub(crate) fn glyph(self) -> &'static str {
        match self {
            PriorityMark::High => "↑",
            PriorityMark::Low => "↓",
        }
    }

    /// The flag's palette token.
    pub(crate) fn color(self, palette: &Palette) -> Color {
        match self {
            PriorityMark::High => palette.peach,
            PriorityMark::Low => palette.overlay0,
        }
    }
}

/// A machine + `backlot-stage` status mark (the health dot a frame or
/// header shows) — defined and tested now, drawn from S1/S2 on.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Health {
    /// Stage answering — `●` `green`.
    Live,
    /// The machine answers but the stage is down — `◌` `peach`.
    StageDown,
    /// No answer at all — `×` `red`.
    Offline,
}

#[allow(dead_code)]
impl Health {
    /// The mark's symbol.
    pub(crate) fn glyph(self) -> &'static str {
        match self {
            Health::Live => "●",
            Health::StageDown => "◌",
            Health::Offline => "×",
        }
    }

    /// The mark's palette token.
    pub(crate) fn color(self, palette: &Palette) -> Color {
        match self {
            Health::Live => palette.green,
            Health::StageDown => palette.peach,
            Health::Offline => palette.red,
        }
    }
}

/// The contrast foreground for text on `panel_bg` / the urgent pill —
/// the reference's `panel_contrast_fg`.
#[allow(dead_code)]
pub(crate) fn contrast_fg(palette: &Palette) -> Color {
    match palette.panel_bg {
        Color::Reset => palette.surface_dim,
        color => color,
    }
}

/// The selected-row style: `selection_bg` + `text` `BOLD`, or
/// `REVERSED` when the theme leaves `selection_bg` unset (terminal
/// theme).
pub(crate) fn selection(palette: &Palette) -> Style {
    if palette.selection_bg == Color::Reset {
        Style::default()
            .fg(palette.text)
            .add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        Style::default()
            .bg(palette.selection_bg)
            .fg(palette.text)
            .add_modifier(Modifier::BOLD)
    }
}

#[cfg(test)]
#[path = "glyphs_tests.rs"]
mod tests;
