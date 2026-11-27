//! `[studio]` parsing: backlot's own switches for the studio layer (the
//! reference has no studios to watch). `notify_asks` sends a desktop
//! notification through the outer terminal when a studio starts waiting
//! on the person — a role's `--ask` or a card.

use serde::Deserialize;

/// `[studio]` — backlot-only settings for the studio layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct StudioConfig {
    /// Raise a desktop notification when a **new** item appears in what
    /// the Call Sheet shows as waiting on the person: a role's `--ask`
    /// or a card waiting for the person. Default: true. One ping per
    /// item; items already open at start do not ping, and answering or
    /// acknowledging never pings.
    pub notify_asks: bool,
}

impl Default for StudioConfig {
    fn default() -> Self {
        Self { notify_asks: true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_on() {
        assert!(StudioConfig::default().notify_asks);
    }

    #[test]
    fn parses_and_defaults_on() {
        let config: crate::config::Config = toml::from_str(
            r#"
[studio]
notify_asks = false
"#,
        )
        .unwrap();
        assert!(!config.studio.notify_asks);
        let config: crate::config::Config = toml::from_str("").unwrap();
        assert!(config.studio.notify_asks, "absent means on");
    }
}
