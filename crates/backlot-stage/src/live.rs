//! The stage's live facts beside the ledgers: each role's work state and
//! `doing` text as the last watch pass read them, and Hollywood's own.
//! The summary document carries the same picture to the client, but a
//! role terminal always carries `BACKLOT_STATE_DIR` and never connects
//! to the core — so `backlot status` reads this file instead, and
//! Hollywood sees the role's current state rather than only its ledger.
//! The stage is the only writer; a missing or unreadable file reads
//! empty, so an older state dir never fails a read.
//!
//! A work word is only as good as its age. The stage writes the file on
//! a change and a heartbeat, and a reader treats a file older than
//! [`STALE_MS`] as absent — a stopped stage must not keep printing its
//! last `· working` as if it were current, which is the same
//! stale-snapshot mistake this task removes from Hollywood's inputs.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use backlot_model::summary::{RoleDoing, Summary, Work};
use serde::{Deserialize, Serialize};

/// The file name under the state base.
pub const FILE: &str = "live.json";

/// A written file older than this is treated as absent: the stage has
/// stopped (or died), and its last work word is a stale snapshot.
pub const STALE_MS: u64 = 15_000;

/// While the roles' facts do not change, the file is still refreshed
/// this often, so its age stays a liveness signal without a write every
/// tick.
pub const HEARTBEAT_MS: u64 = 5_000;

/// One role's live facts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveRole {
    pub studio: String,
    pub role: String,
    /// The watch pass's work classification, while the role is live and
    /// the screen has been classified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work: Option<Work>,
    /// Deepest descendant command line and since when, published while `Working`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doing: Option<RoleDoing>,
    /// What the terminal status reports (legacy activity string), capped like the summary's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<String>,
}

/// Every role's live facts, plus the wall clock they were read at.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Live {
    #[serde(default)]
    pub updated_ms: u64,
    #[serde(default)]
    pub roles: Vec<LiveRole>,
}

/// The file under `base`.
pub fn path(base: &Path) -> PathBuf {
    base.join(FILE)
}

/// The live facts a summary document implies — the same roles, work and
/// `doing` the panel draws, plus Hollywood's own row under
/// `hollywood/hollywood`.
pub fn from_summary(summary: &Summary, updated_ms: u64) -> Live {
    let mut roles = Vec::new();
    for studio in &summary.studios {
        for role in &studio.roles {
            if role.work.is_none() && role.doing.is_none() && role.activity.is_none() {
                continue;
            }
            roles.push(LiveRole {
                studio: studio.id.clone(),
                role: role.role.clone(),
                work: role.work,
                doing: role.doing.clone(),
                activity: role.activity.clone(),
            });
        }
    }
    if let Some(h) = &summary.hollywood
        && (h.work.is_some() || h.doing.is_some())
    {
        roles.push(LiveRole {
            studio: backlot_model::hollywood::HOLLYWOOD.into(),
            role: backlot_model::hollywood::HOLLYWOOD.into(),
            work: h.work,
            doing: None,
            activity: h.doing.clone(),
        });
    }
    Live { updated_ms, roles }
}

/// Write the facts atomically: a temp sibling renamed into place, so a
/// reader never sees a half-written file.
pub fn write(base: &Path, live: &Live) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(live).map_err(std::io::Error::other)?;
    let dest = path(base);
    let tmp = base.join(format!(".{FILE}.tmp-{}", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, dest)
}

/// What the stage's next tick should do with `live.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Write {
    /// Write it now: the facts changed, or the heartbeat is due.
    Now,
    /// Leave the file as it is: the facts are unchanged and the
    /// heartbeat is not yet due.
    Skip,
}

/// Whether to write this tick. The file is written whenever the roles'
/// facts change, and otherwise on a [`HEARTBEAT_MS`] cadence so a
/// stopped stage's file ages past [`STALE_MS`] and readers know it is
/// stale — never on every tick, which would make the age meaningless.
/// `written` is the last file's `updated_ms`, or `None` before the first.
pub fn due(facts: &Live, written: Option<&Live>, now_ms: u64) -> Write {
    let changed = written.is_none_or(|last| last.roles != facts.roles);
    let heartbeat =
        written.is_none_or(|last| now_ms.saturating_sub(last.updated_ms) >= HEARTBEAT_MS);
    if changed || heartbeat {
        Write::Now
    } else {
        Write::Skip
    }
}

/// The facts under `base` as of `now_ms`, when the file is fresh. A
/// missing, unreadable or stale file (older than [`STALE_MS`]) is
/// `None`: a stopped stage's last work word is not current state.
pub fn read_fresh(base: &Path, now_ms: u64) -> Option<Live> {
    let live: Live = std::fs::read(path(base))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())?;
    (live.updated_ms != 0 && now_ms.saturating_sub(live.updated_ms) <= STALE_MS).then_some(live)
}

/// The facts under `base` as of `now_ms`; stale reads as empty.
#[cfg(test)]
pub fn read(base: &Path, now_ms: u64) -> Live {
    read_fresh(base, now_ms).unwrap_or_default()
}

/// `(studio, role)` → work, for the callers that only classify.
pub fn work_map(live: &Live) -> BTreeMap<(String, String), Work> {
    live.roles
        .iter()
        .filter_map(|r| r.work.map(|w| ((r.studio.clone(), r.role.clone()), w)))
        .collect()
}

/// `(studio, role)` → deepest descendant command while `Working`.
pub fn doing_map(live: &Live) -> BTreeMap<(String, String), RoleDoing> {
    live.roles
        .iter()
        .filter(|r| r.work == Some(Work::Working))
        .filter_map(|r| {
            r.doing
                .clone()
                .map(|d| ((r.studio.clone(), r.role.clone()), d))
        })
        .collect()
}

/// What `backlot status` reads from `live.json`: the work samples,
/// active `doing` commands, and whether a running stage supplied them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LiveView {
    /// The work sample per `(studio, role)`, from a fresh file only.
    pub work: BTreeMap<(String, String), Work>,
    /// The deepest descendant command per `(studio, role)` while `Working`.
    pub doing: BTreeMap<(String, String), RoleDoing>,
    /// A fresh file exists — the stage is running.
    pub running: bool,
}

impl LiveView {
    /// Read `live.json` under `base` as of `now_ms`.
    pub fn read(base: &Path, now_ms: u64) -> LiveView {
        match read_fresh(base, now_ms) {
            Some(live) => LiveView {
                work: work_map(&live),
                doing: doing_map(&live),
                running: true,
            },
            None => LiveView::default(),
        }
    }
}

/// The wall-clock milliseconds since the epoch.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
