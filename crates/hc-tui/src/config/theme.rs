// Derived from herdr src/config/theme.rs (Apache-2.0).

use serde::Deserialize;

/// Theme configuration: pick a built-in or override individual tokens.
///
/// ```toml
/// [theme]
/// name = "tokyo-night"
///
/// [theme.custom]
/// accent = "#f5c2e7"
/// ```
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    /// Built-in theme name. Default: "catppuccin".
    pub name: Option<String>,
    /// Follow host terminal light/dark appearance and switch between theme names.
    pub auto_switch: bool,
    /// Theme name used when `auto_switch` selects a dark appearance.
    pub dark_name: Option<String>,
    /// Theme name used when `auto_switch` selects a light appearance.
    pub light_name: Option<String>,
    /// Custom overrides — applied on top of the selected base theme.
    pub custom: Option<CustomThemeColors>,
}

impl ThemeConfig {
    /// The reference `ThemeConfig::diagnostics`: unknown theme names fall
    /// back per field and report the valid list.
    pub(crate) fn diagnostics(&self) -> Vec<String> {
        let valid = super::THEME_NAMES.join(", ");
        [
            ("theme.name", self.name.as_deref(), "catppuccin"),
            ("theme.dark_name", self.dark_name.as_deref(), "catppuccin"),
            (
                "theme.light_name",
                self.light_name.as_deref(),
                "catppuccin-latte",
            ),
        ]
        .into_iter()
        .filter_map(|(field, value, fallback)| {
            let value = value?;
            super::canonical_theme_name(value)
                .is_none()
                .then(|| {
                    format!(
                        "unknown theme name {field} = {value:?}; using {fallback:?}; valid themes: {valid}"
                    )
                })
        })
        .collect()
    }
}

/// Per-token color overrides. All fields optional — only set what you want to change.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CustomThemeColors {
    pub accent: Option<String>,
    pub panel_bg: Option<String>,
    pub sidebar_bg: Option<String>,
    pub active_row_bg: Option<String>,
    pub selection_bg: Option<String>,
    pub surface0: Option<String>,
    pub surface1: Option<String>,
    pub surface_dim: Option<String>,
    pub overlay0: Option<String>,
    pub overlay1: Option<String>,
    pub text: Option<String>,
    pub subtext0: Option<String>,
    pub mauve: Option<String>,
    pub green: Option<String>,
    pub yellow: Option<String>,
    pub red: Option<String>,
    pub blue: Option<String>,
    pub teal: Option<String>,
    pub peach: Option<String>,
    /// Overrides applied when `auto_switch` selects a light appearance.
    pub light: Option<ModeThemeColors>,
    /// Overrides applied when `auto_switch` selects a dark appearance.
    pub dark: Option<ModeThemeColors>,
}

/// Per-token color overrides for one auto-switch appearance.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ModeThemeColors {
    pub accent: Option<String>,
    pub panel_bg: Option<String>,
    pub sidebar_bg: Option<String>,
    pub active_row_bg: Option<String>,
    pub selection_bg: Option<String>,
    pub surface0: Option<String>,
    pub surface1: Option<String>,
    pub surface_dim: Option<String>,
    pub overlay0: Option<String>,
    pub overlay1: Option<String>,
    pub text: Option<String>,
    pub subtext0: Option<String>,
    pub mauve: Option<String>,
    pub green: Option<String>,
    pub yellow: Option<String>,
    pub red: Option<String>,
    pub blue: Option<String>,
    pub teal: Option<String>,
    pub peach: Option<String>,
}
