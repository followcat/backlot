// Derived from herdr-core schema/hc.openrpc.json and schema/README.md (Apache-2.0).

//! The `hello` handshake: generation negotiation, core identity, and the
//! method/feature lists a client checks before enabling features.

use crate::PROTOCOL_GENERATION;
use serde::{Deserialize, Serialize};

/// Feature strings a core advertises in `hello` replies (and confirms in
/// `terminal.attach` results where the whole path must support them).
pub mod feature {
    /// The path accepts AUTOMATIC_INPUT (kind 6) frames end to end; without
    /// it, automatic query replies go as ordinary DATA.
    pub const AUTOMATIC_INPUT: &str = "automatic_input";
    /// Observers may opt in to core-to-viewer RESIZE before replacement
    /// paints.
    pub const RESIZE_NOTIFICATIONS: &str = "resize_notifications";
}

/// Who a core is: a permanent random id and a display name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoreInfo {
    /// Permanent random id, reset only when state is wiped.
    pub id: String,
    /// Display name, the hostname by default.
    pub name: String,
}

/// Params for `hello`. A plain client sends `generation` and, when it wants
/// attach streams or framed events, `framed: true`. The `core`, `calls_you`
/// and `features` fields describe a *core* dialing in; a viewer client leaves
/// them out.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HelloParams {
    /// Protocol generation the caller speaks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<u32>,
    /// Switch this connection to framed mode after the reply.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub framed: bool,
    /// Another core dialing in: who it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core: Option<CoreInfo>,
    /// What that core calls this one, so news reaches it as `machine/t7`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calls_you: Option<String>,
    /// Features supported by a core dialing in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
}

impl HelloParams {
    /// A viewer client's handshake for this crate's generation.
    pub fn new() -> HelloParams {
        HelloParams {
            generation: Some(PROTOCOL_GENERATION),
            ..HelloParams::default()
        }
    }

    /// The same handshake asking for framed mode after the reply.
    pub fn framed() -> HelloParams {
        HelloParams {
            framed: true,
            ..HelloParams::new()
        }
    }
}

/// The `hello` result: what the core is and what it can do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HelloResult {
    /// Protocol generation the core speaks.
    pub generation: u32,
    /// Its `hc` version.
    pub version: String,
    /// Its identity.
    pub core: CoreInfo,
    /// Whether this connection switched to framed mode.
    pub framed: bool,
    /// Every method it offers. Older cores may not send the list.
    #[serde(default)]
    pub methods: Vec<String>,
    /// Feature strings it advertises. Absent on older cores: every optional
    /// capability then counts as unsupported.
    #[serde(default)]
    pub features: Vec<String>,
}

/// `hello` answered with a generation this client does not speak.
#[derive(Debug, Clone, PartialEq)]
pub struct UnsupportedGeneration {
    /// The generation the core reported.
    pub found: u32,
}

impl std::fmt::Display for UnsupportedGeneration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "core speaks protocol generation {}, we speak {PROTOCOL_GENERATION}",
            self.found
        )
    }
}

impl std::error::Error for UnsupportedGeneration {}

/// A checked view of a [`HelloResult`]: generation validated, method and
/// feature lookups with explanations for what is missing.
#[derive(Debug)]
pub struct Capabilities<'a> {
    hello: &'a HelloResult,
}

impl<'a> Capabilities<'a> {
    /// Validates the generation. A mismatch is the only hard failure; missing
    /// methods or features degrade instead.
    pub fn new(hello: &'a HelloResult) -> Result<Capabilities<'a>, UnsupportedGeneration> {
        if crate::is_supported_generation(hello.generation) {
            Ok(Capabilities { hello })
        } else {
            Err(UnsupportedGeneration {
                found: hello.generation,
            })
        }
    }

    /// The validated handshake.
    pub fn hello(&self) -> &'a HelloResult {
        self.hello
    }

    /// Whether the core offers `method`.
    pub fn has_method(&self, method: &str) -> bool {
        self.hello.methods.iter().any(|m| m == method)
    }

    /// Whether the core advertises `feature`.
    pub fn has_feature(&self, feature: &str) -> bool {
        self.hello.features.iter().any(|f| f == feature)
    }

    /// Which of `needed` methods the core does not offer.
    pub fn missing_methods<'m>(&self, needed: &[&'m str]) -> Vec<&'m str> {
        needed
            .iter()
            .copied()
            .filter(|m| !self.has_method(m))
            .collect()
    }

    /// Why a method-gated feature is unavailable, for showing to the user:
    /// `None` when the core offers the method.
    pub fn explain_unavailable(&self, method: &str) -> Option<String> {
        if self.has_method(method) {
            None
        } else {
            Some(format!(
                "core {} does not offer `{method}` (protocol generation {})",
                self.hello.core.name, self.hello.generation
            ))
        }
    }
}
