// Derived from herdr-core schema/hc.openrpc.json (Apache-2.0).

//! Terminal records and the methods around them: `terminal.list/get/run/read/
//! kill` and `terminal.attach`. String vocabularies (`state`, `reason`,
//! `how`, ...) stay plain strings so values a newer core adds parse instead
//! of failing, and object types keep fields beyond the schema in `extra` so
//! a patched core's extensions survive a round trip.

mod api;
mod facts;

pub use api::*;
pub use facts::{Activity, AheadBehind, Checkout, RecordClock, RepositoryFacts};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// One OSC 7501 record, kept exactly as the program reported it.
///
/// Display state comes only from these fields: `extra` is retained verbatim
/// for a patched core's extensions (for example `source`), never interpreted.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Status {
    /// The program's own word: `idle`, `working`, `done`, `blocked`, `error`,
    /// or anything newer.
    pub state: String,
    /// `permission`, `question` or `auth` (blocked only, when reported).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// 0..100 (working or blocked, when reported).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<u8>,
    /// The reporting program's name.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub app: String,
    /// Reported title.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// Reported message.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub msg: String,
    /// Fields beyond the schema, retained verbatim.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// What runs in a terminal right now: the foreground process group's leader
/// and its folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Foreground {
    /// The process id.
    pub pid: i32,
    /// Its name as the OS has it (`node`, `vim`, `sh`).
    pub name: String,
    /// Its argv.
    pub argv: Vec<String>,
    /// Its working directory now (resolved by the OS).
    pub cwd: String,
}

/// A template first message that was not sent, or its delivery report.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FirstMessage {
    /// Whether it went in.
    pub sent: bool,
    /// Why it was not sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Whether the agent started working on it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<bool>,
    /// The turn when it went in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<u64>,
}

/// A terminal as the API shows it: its public facts, without the core's
/// bookkeeping. Required fields are what generation 1 guarantees; the rest
/// default, so records from older cores still parse. Fields beyond the
/// schema are kept in `extra` — retained, never interpreted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Terminal {
    /// `t7`: this terminal forever on this core.
    pub address: String,
    /// Its unique-among-live name, when named.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Client-visible labels, e.g. `herdr.space`.
    pub labels: BTreeMap<String, String>,
    /// Program and arguments.
    pub argv: Vec<String>,
    /// Working directory at start.
    pub cwd: String,
    /// Replace a lost program with a shell in place.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep: bool,
    /// `live` or `finished`; kept as a string for newer states.
    pub state: String,
    /// The program's pid, while it runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// Exit code when it ended with one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Signal when it died by one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<i32>,
    /// Why its program ended or is pending: `exited`, `killed`, `server_stop`,
    /// `host_shutdown`, `signaled`, `holder_lost`, `stopped_by_restart`,
    /// `spawn_pending`, or anything newer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// When it was created, ms since the epoch.
    pub created_ms: u64,
    /// When it finished, ms since the epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_ms: Option<u64>,
    /// `exact`, or `inexact` after a core crash.
    pub screen: String,
    /// Grid size.
    pub cols: u16,
    /// Grid size.
    pub rows: u16,
    /// Which terminal the program believes it runs in (TERM_PROGRAM).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub identity: String,
    /// The program's own word for what it's doing (7501 root record).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    /// When that word last changed, ms since the epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_ms: Option<u64>,
    /// Other 7501 records by id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub records: BTreeMap<String, Status>,
    /// Goes up each time the status leaves `working`.
    #[serde(default)]
    pub turn: u64,
    /// Whether the program ever sent a 7501 report.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reported: bool,
    /// An incoming message is held for a person's unsent draft.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub waiting_for_person: bool,
    /// Its title.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// The terminal that started it, set once at start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Its parent gets notes about it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub notify: bool,
    /// Children end with their parent unless `outlive_parent`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub die_with_parent: bool,
    /// Removed right after it exits.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rm: bool,
    /// The template that started it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    /// A template first message that was not sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_message: Option<FirstMessage>,
    /// Template overrides: submit key, stop key, paste delay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submit: Option<String>,
    /// The template's stop key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<String>,
    /// Pause between paste and submit, in ms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paste_gap_ms: Option<u64>,
    /// What runs in it right now and where (null once finished).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreground: Option<Foreground>,
    /// Origin repository observation, absent on unsupported paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<RepositoryFacts>,
    /// Origin record clocks. No cross-origin total order is implied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<Activity>,
    /// Fields beyond the schema, retained verbatim.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// How a `terminal.wait` (or a waited `run`/`send`) ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaitResult {
    /// The terminal waited on.
    pub terminal: String,
    /// False on timeout: the terminal is still running.
    pub ended: bool,
    /// `status`, `exit`, `output`, `timeout`, `not_started`, `children`,
    /// `message`, `running`, or anything newer.
    pub reason: String,
    /// The turn the wait ended on.
    pub turn: u64,
    /// With `children`: one result per child it reports on.
    #[serde(default)]
    pub children: Vec<WaitResult>,
    /// Exit code when it ended with one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Signal when it died by one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<i32>,
    /// The exact status word reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// With a message: the sender's address; null when a person sent it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

impl Terminal {
    /// A live process carrying core-reported status, without status inference.
    pub fn is_live_reporter(&self) -> bool {
        self.state == "live"
            && self.pid.is_some()
            && (self.reported || self.status.is_some() || !self.records.is_empty())
    }
}

/// A size in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitialSize {
    /// Columns.
    pub cols: u16,
    /// Rows.
    pub rows: u16,
}

/// What a viewer's own terminal looks like, reported to the core so it can
/// answer the program's display queries.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Display {
    /// Default background as `#rrggbb`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg: Option<String>,
    /// Cell size in pixels `[width, height]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell: Option<[u32; 2]>,
    /// Cursor color as `#rrggbb`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Default foreground as `#rrggbb`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fg: Option<String>,
}
