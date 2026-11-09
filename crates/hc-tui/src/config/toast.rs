// Derived from herdr src/config/model.rs (Apache-2.0).
//! `[ui.toast]` parsing: delivery channel, settle delay and the in-app
//! toast position. The legacy `enabled = true/false` form still maps to
//! `herdr`/`off` when `delivery` is absent, like the reference.

use serde::{Deserialize, Deserializer, de};

/// The reference `MAX_TOAST_DELAY_SECONDS` — the settle window cap.
pub const MAX_TOAST_DELAY_SECONDS: u64 = 3600;

/// Where toast notifications are delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ToastDelivery {
    /// Notifications are synthesized but never shown or forwarded.
    #[default]
    Off,
    /// The in-app toast card / mobile banner.
    Herdr,
    /// An OSC 9/99 notification on the outer terminal.
    Terminal,
    /// A desktop notification; hc-tui falls back to the terminal path.
    System,
}

/// Corner the in-app toast card anchors to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ToastHerdrPosition {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
}

/// The reference `ToastClipboardPosition` — six anchor spots for the
/// copied-to-clipboard card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ToastClipboardPosition {
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    #[default]
    BottomCenter,
    BottomRight,
}

/// `[ui.toast.clipboard]` — the reference `ClipboardToastConfig`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ClipboardToastConfig {
    pub enabled: bool,
    pub position: ToastClipboardPosition,
}

impl Default for ClipboardToastConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            position: ToastClipboardPosition::BottomCenter,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct HerdrToastConfig {
    pub position: ToastHerdrPosition,
}

impl Default for HerdrToastConfig {
    fn default() -> Self {
        Self {
            position: ToastHerdrPosition::BottomRight,
        }
    }
}

/// `[ui.toast]` — the reference's `ToastConfig`.
#[derive(Debug, Clone)]
pub struct ToastConfig {
    pub delivery: ToastDelivery,
    /// Seconds a synthesized alert settles before validation/delivery.
    pub delay_seconds: u64,
    pub herdr: HerdrToastConfig,
    pub clipboard: ClipboardToastConfig,
}

impl Default for ToastConfig {
    fn default() -> Self {
        Self {
            delivery: ToastDelivery::Off,
            delay_seconds: 1,
            herdr: HerdrToastConfig::default(),
            clipboard: ClipboardToastConfig::default(),
        }
    }
}

impl<'de> Deserialize<'de> for ToastConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct RawToastConfig {
            delivery: Option<ToastDelivery>,
            enabled: Option<bool>,
            delay_seconds: Option<u64>,
            herdr: HerdrToastConfig,
            clipboard: ClipboardToastConfig,
        }

        let raw = RawToastConfig::deserialize(deserializer)?;
        let legacy_delivery = match raw.enabled {
            Some(true) => ToastDelivery::Herdr,
            Some(false) | None => ToastDelivery::Off,
        };
        let delivery = raw.delivery.unwrap_or(legacy_delivery);
        let default = Self::default();
        let delay_seconds = raw.delay_seconds.unwrap_or(default.delay_seconds);
        if delay_seconds > MAX_TOAST_DELAY_SECONDS {
            return Err(de::Error::custom(format!(
                "ui.toast.delay_seconds must be between 0 and {MAX_TOAST_DELAY_SECONDS}"
            )));
        }
        Ok(Self {
            delivery,
            delay_seconds,
            herdr: raw.herdr,
            clipboard: raw.clipboard,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_reference() {
        let toast = ToastConfig::default();
        assert_eq!(toast.delivery, ToastDelivery::Off);
        assert_eq!(toast.delay_seconds, 1);
        assert_eq!(toast.herdr.position, ToastHerdrPosition::BottomRight);
    }

    #[test]
    fn delivery_and_delay_parse() {
        let config: crate::config::Config = toml::from_str(
            r#"
[ui.toast]
delivery = "terminal"
delay_seconds = 3
"#,
        )
        .unwrap();
        assert_eq!(config.ui.toast.delivery, ToastDelivery::Terminal);
        assert_eq!(config.ui.toast.delay_seconds, 3);
    }

    #[test]
    fn legacy_enabled_maps_to_herdr_and_delivery_wins() {
        let config: crate::config::Config = toml::from_str(
            r#"
[ui.toast]
enabled = true
"#,
        )
        .unwrap();
        assert_eq!(config.ui.toast.delivery, ToastDelivery::Herdr);
        let config: crate::config::Config = toml::from_str(
            r#"
[ui.toast]
enabled = false
delivery = "system"
"#,
        )
        .unwrap();
        assert_eq!(config.ui.toast.delivery, ToastDelivery::System);
    }

    #[test]
    fn herdr_position_parses_kebab_case() {
        let config: crate::config::Config = toml::from_str(
            r#"
[ui.toast.herdr]
position = "top-left"
"#,
        )
        .unwrap();
        assert_eq!(config.ui.toast.herdr.position, ToastHerdrPosition::TopLeft);
    }

    #[test]
    fn delay_over_max_is_a_config_error() {
        let result: Result<crate::config::Config, _> = toml::from_str(
            r#"
[ui.toast]
delay_seconds = 3601
"#,
        );
        let error = result.expect_err("delay above the cap is rejected");
        assert!(error.to_string().contains("ui.toast.delay_seconds"));
    }
}
