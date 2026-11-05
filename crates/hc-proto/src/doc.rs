// Derived from herdr-core schema/hc.openrpc.json (Apache-2.0).

//! `doc.get` and versioned `doc.set`: opaque JSON documents stored by name. The value is
//! never interpreted by the core, and not by this crate either.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Params for `doc.get`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocGetParams {
    /// Document name, using the terminal name rules.
    pub name: String,
    /// Another machine; omitted for this core.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<String>,
}

/// The `doc.get` result. An unwritten name returns `value: null` and
/// `version: 0`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocGetResult {
    /// The stored JSON, or null. Opaque by design.
    pub value: Value,
    /// The document version for versioned `doc.set` writes. Zero when never
    /// written; deletions retain their version.
    pub version: u64,
}

/// Versioned compare-and-set. A conflict requires a fresh read before retrying
/// the intended operation; callers must not resend a stale full document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocSetParams {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<String>,
    pub value: Value,
    pub version: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocSetResult {
    pub version: u64,
}
