// Derived from herdr-core schema/hc.openrpc.json (Apache-2.0).

//! Params and results for the terminal methods hc-tui uses:
//! `terminal.list/get/run/read/kill` and `terminal.attach`.

use crate::machine::MachineView;
use crate::terminal::{Display, InitialSize, Terminal, WaitResult};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Params for `terminal.list`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ListParams {
    /// Include finished terminals.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all: bool,
    /// Only terminals carrying all of these labels.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
}

/// The `terminal.list` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListResult {
    /// This machine's terminals, then each connected machine's (`machine/t7`).
    pub terminals: Vec<Terminal>,
    /// Finished terminals left out (see `all`).
    pub hidden_finished: u64,
    /// Machines added here and their lines. Absent when empty.
    #[serde(default)]
    pub machines: Vec<MachineView>,
}

/// Params selecting one terminal plus the optional caller id: `terminal.get`
/// and `terminal.kill` share this shape (kill answers with a `Terminal`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerminalParams {
    /// The terminal: `t7`, a name, or `machine/t7`.
    pub terminal: String,
    /// Who is calling (`HC_TERMINAL_ID` inside a terminal).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller: Option<String>,
}

/// Params for `terminal.read`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadParams {
    /// The terminal: `t7`, a name, or `machine/t7`.
    pub terminal: String,
    /// Lines of history above the screen.
    #[serde(default)]
    pub lines: u64,
    /// Who is calling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller: Option<String>,
}

/// The `terminal.read` result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadResult {
    /// The screen plus requested history, as text.
    pub text: String,
    /// False when part of what was asked for may be missing.
    pub complete: bool,
    /// `screen`, `history`, or `stitched` (a full-screen app, read by
    /// scrolling it); kept as a string for newer values.
    pub how: String,
    /// Why only the screen: `working`, `blocked` or `error`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// Params for `terminal.run`: start a program in a new terminal. With neither
/// `argv` nor `line` the machine's own shell starts.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RunParams {
    /// Program and arguments, or a template name and its task.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub argv: Vec<String>,
    /// Who is calling (`HC_TERMINAL_ID` inside a terminal).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller: Option<String>,
    /// Working directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Ignored: children end with their parent unless `outlive_parent`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub die_with_parent: bool,
    /// The caller's colors and cell size, answering display queries early.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<Display>,
    /// Set on top of the core's environment.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Replace lost programs with a shell in place.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep: bool,
    /// Labels, key=value.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    /// A shell line instead of argv, run by that machine's shell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    /// Name, unique among live terminals.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Don't type notes about it into its parent.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_notify: bool,
    /// No parent, even when started from a terminal.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_parent: bool,
    /// Run it on another machine; its address is `machine/t7`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<String>,
    /// Keep it running when its parent's program ends.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub outlive_parent: bool,
    /// Parent terminal (default: the terminal it is started from).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Remove it right after it exits.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rm: bool,
    /// Initial size before the program starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<InitialSize>,
    /// Seconds before a wait checks in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<f64>,
    /// Then wait until it is not working and return its output.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub wait: bool,
}

/// The `terminal.run` result: the new terminal's record, plus the optional
/// wait report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunResult {
    /// The created terminal's facts.
    #[serde(flatten)]
    pub terminal: Terminal,
    /// With `wait`: its text then, like `terminal.read`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// With `wait`: how the wait ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<WaitResult>,
}

/// Params for `terminal.attach` (framed connections only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttachParams {
    /// The terminal: `t7`, a name, or `machine/t7`.
    pub terminal: String,
    /// Watch only: never type or resize.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub observe: bool,
    /// Observer understands core-to-viewer RESIZE before replacement paints.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub resize_notifications: bool,
    /// Scrollback lines to show first (main screen only).
    #[serde(default)]
    pub history: u64,
    /// The viewer's size in cells.
    pub cols: u16,
    /// The viewer's size in cells.
    pub rows: u16,
    /// The viewer's own colors and cell size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<Display>,
    /// Back after a dropped line: sets the size only once active again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub resumed: bool,
    /// Who is calling (`HC_TERMINAL_ID` inside a terminal): never itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller: Option<String>,
}

/// The `terminal.attach` result: the stream's channel and the negotiated
/// capabilities. Capability fields default to false on older replies, so an
/// absent flag never enables a feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttachResult {
    /// Stream channel: first paint, then live bytes.
    pub channel: u32,
    /// The attached terminal's record.
    pub terminal: Terminal,
    /// True only when the entire path sends RESIZE before replacement paints.
    #[serde(default)]
    pub resize_notifications: bool,
    /// The entire attach path accepts AUTOMATIC_INPUT (kind 6).
    #[serde(default)]
    pub automatic_input: bool,
}
