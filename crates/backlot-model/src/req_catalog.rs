//! Catalog wire types for `backlot-cat` (`schema: "backlot-catalog/1"`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::event::{ActorIdentity, Candidate, StudioMode};

pub const CAT_DOC_NAME: &str = "backlot-cat";
pub const CAT_SCHEMA: &str = "backlot-catalog/1";

/// Stage-published static/catalog document (`backlot-cat`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogDoc {
    pub schema: String,
    pub server_id: String,
    pub ts_ms: u64,
    #[serde(default)]
    pub runners: Vec<RunnerCatalogEntry>,
    pub hollywood_cfg: HollywoodCfgCatalog,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_cast: Option<DefaultCastCatalog>,
    #[serde(default)]
    pub archived: Vec<ArchivedCatalogEntry>,
    pub home: String,
}

impl CatalogDoc {
    /// Whether two catalog documents differ in any semantic field other than `ts_ms`.
    pub fn content_eq(&self, other: &Self) -> bool {
        self.schema == other.schema
            && self.server_id == other.server_id
            && self.runners == other.runners
            && self.hollywood_cfg == other.hollywood_cfg
            && self.default_cast == other.default_cast
            && self.archived == other.archived
            && self.home == other.home
    }
}

/// One installed/supported runner row in `backlot-cat` and `backlot runners --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunnerCatalogEntry {
    pub runner: String,
    pub status: String,
    pub version: Option<String>,
    pub path: Option<String>,
    pub needs_path: bool,
    pub models: Vec<String>,
    pub efforts: Vec<String>,
}

/// Public view of `[hollywood]` configuration (never includes secrets).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HollywoodCfgCatalog {
    pub configured: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_cast: Option<String>,
}

/// Cast structure `studio new` would start from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefaultCastCatalog {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<StudioMode>,
    pub producer: ActorIdentity,
    pub director: ActorIdentity,
    pub writer: ActorIdentity,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<Candidate>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub limits: BTreeMap<String, u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rework_cap: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stall_limit_s: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nudge_after_s: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escalate_after_s: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_budget_s: Option<u32>,
}

/// One archived studio entry matching `studio list --archived --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchivedCatalogEntry {
    pub name: String,
    pub stamp: String,
    pub studio: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requirement: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_ms: Option<u64>,
}
