//! The stage's live facts beside the ledgers: each role's work state and
//! `doing` text as the last watch pass read them, and Hollywood's own.
//! The summary document carries the same picture to the client, but a
//! role terminal always carries `BACKLOT_STATE_DIR` and never connects
//! to the core — so `backlot status` reads this file instead, and
//! Hollywood sees the role's current state rather than only its ledger.
//! The stage is the only writer; a missing or unreadable file reads
//! empty, so an older state dir never fails a read.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use backlot_model::summary::{Summary, Work};
use serde::{Deserialize, Serialize};

/// The file name under the state base.
pub const FILE: &str = "live.json";

/// One role's live facts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveRole {
    pub studio: String,
    pub role: String,
    /// The watch pass's work classification, while the role is live and
    /// the screen has been classified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work: Option<Work>,
    /// What the role reports it is doing, capped like the summary's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doing: Option<String>,
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
            if role.work.is_none() && role.doing.is_none() {
                continue;
            }
            roles.push(LiveRole {
                studio: studio.id.clone(),
                role: role.role.clone(),
                work: role.work,
                doing: role.doing.clone(),
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
            doing: h.doing.clone(),
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

/// The facts under `base`; a missing or unreadable file is empty.
pub fn read(base: &Path) -> Live {
    std::fs::read(path(base))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// `(studio, role)` → work, for the callers that only classify.
pub fn work_map(live: &Live) -> BTreeMap<(String, String), Work> {
    live.roles
        .iter()
        .filter_map(|r| r.work.map(|w| ((r.studio.clone(), r.role.clone()), w)))
        .collect()
}
