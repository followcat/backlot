//! The sidebar's public row vocabulary — what `model::build` emits and
//! the renderer draws. Every `String` here is already sanitized, so a
//! `Row` can go straight to the host terminal.

/// A row's stable identity — what selection tracks across rebuilds.
#[derive(Debug, Clone, PartialEq)]
pub enum RowId {
    /// A machine heading, by machine name (`local` for this core).
    Machine(String),
    /// A space heading under a machine, by `(machine, space key)`.
    Space { machine: String, key: String },
    /// A terminal row, by its localized address (`t7`, `host-b/t7`).
    Terminal(String),
}

/// Semantic color hint; the renderer maps it to styles. Colors are
/// decoration only — the words carry the meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Headings and neutral text.
    Heading,
    /// A machine heading whose machine is not connected.
    Offline,
    /// Reported `blocked` — the person's turn.
    Blocked,
    /// Reported `error`.
    Error,
    /// Program lost (died without finishing).
    Lost,
    /// Reported `working`.
    Working,
    /// Reported `done` — ready.
    Done,
    /// `idle`, unknown words, pending, unreported live terminals.
    Quiet,
    /// Finished programs and fully dimmed offline rows.
    Muted,
}

/// One rendered row: label, secondary note and status badge.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// Stable identity for selection.
    pub id: RowId,
    /// Indent level (0 machine, 1 space, 2 terminal).
    pub depth: u8,
    /// Primary label, sanitized.
    pub text: String,
    /// Secondary detail (`worker of t2`, machine `why`), sanitized.
    pub note: String,
    /// Status badge, sanitized.
    pub badge: String,
    /// Semantic tone.
    pub tone: Tone,
    /// Render dimmed: the owning machine is not connected.
    pub dim: bool,
}

/// The three headline counters over agents only (docs/sidebar.md).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    /// Live agents whose effective word is `blocked`.
    pub your_turn: usize,
    /// Live agents whose effective word is `working`.
    pub working: usize,
    /// Live agents whose effective word is `done`.
    pub ready: usize,
}

/// Sidebar-wide summary for the compact line.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Summary {
    /// Machines currently shown, including offline ones.
    pub machines: usize,
    /// Machines not connected right now.
    pub offline: usize,
}

/// The sidebar's mode: which list the rows belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// Machines → spaces → terminals.
    Spaces,
    /// Every agent, most urgent first.
    Agents,
}
