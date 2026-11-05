//! Read-only structure mapping and hc-tui's separate versioned layout.
// Derived from herdr-app src/mirror.rs and src/layout.rs (Apache-2.0).

mod reader;
mod tree;

pub use reader::read;
pub use tree::Tree;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Space {
    pub id: String,
    pub name: String,
    pub custom_name: bool,
    pub folder: String,
    pub tabs: Vec<Tab>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tab {
    pub id: String,
    pub name: String,
    pub custom_name: bool,
    pub root: Tree,
}

/// Only overrides belong here: inventory and herdr-app remain authoritative
/// for membership. Unknown fields survive a read/modify/write round trip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub schema: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
    #[serde(default)]
    pub tabs: BTreeMap<String, TabLayout>,
    #[serde(default)]
    pub order: BTreeMap<String, Vec<String>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            schema: 1,
            focus: None,
            tabs: BTreeMap::new(),
            order: BTreeMap::new(),
            extra: BTreeMap::new(),
        }
    }
}

impl Document {
    /// Never replace a document using an unknown schema, including malformed
    /// current values. The caller can still render the independent structure.
    pub fn decode(value: Value) -> Result<Self, String> {
        if value.is_null() {
            return Ok(Self::default());
        }
        let document: Self = serde_json::from_value(value).map_err(|e| e.to_string())?;
        if document.schema != 1 {
            return Err("Unsupported hc-tui layout schema".into());
        }
        if document
            .tabs
            .values()
            .any(|tab| tab.root.as_ref().is_some_and(|r| !r.valid()))
        {
            return Err("Invalid hc-tui layout tree".into());
        }
        Ok(document)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TabLayout {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<Tree>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zoomed: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[cfg(test)]
mod tests;
