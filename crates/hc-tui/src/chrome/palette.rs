// Derived from herdr src/app/state.rs and src/config/theme.rs (Apache-2.0).

//! Theme palettes, name resolution, and per-token overrides. The named
//! palette constructors live in `palette/themes.rs` so this file stays near
//! the size guideline.

use ratatui::style::Color;

mod themes;

use crate::config::{CustomThemeColors, ModeThemeColors};

/// All colors used by the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    /// Primary accent (highlight, active borders).
    pub accent: Color,
    /// Background for the tab bar, floating panels, overlays, and modals.
    pub panel_bg: Color,
    /// Optional sidebar background. Reset preserves the terminal background.
    pub sidebar_bg: Color,
    /// Background for the active workspace and focused agent rows.
    pub active_row_bg: Color,
    /// Background for the Navigate-mode cursor row in the sidebar.
    pub selection_bg: Color,
    /// Subtle surface background for selected/focused items.
    pub surface0: Color,
    /// Slightly lighter surface for hover/active states.
    pub surface1: Color,
    /// Very dim surface for separators.
    pub surface_dim: Color,
    /// Muted text (secondary info, numbers).
    pub overlay0: Color,
    /// Slightly brighter overlay text.
    pub overlay1: Color,
    /// Main text color.
    pub text: Color,
    /// Subdued text (workspace numbers, dim labels).
    pub subtext0: Color,
    /// Branch name / special label color.
    pub mauve: Color,
    /// Done / idle states.
    pub green: Color,
    /// Working / running states.
    pub yellow: Color,
    /// Needs attention / blocked states.
    pub red: Color,
    /// Unseen / done notification accent.
    pub blue: Color,
    /// Notification accent / unseen markers.
    pub teal: Color,
    /// Interrupted / warning states.
    pub peach: Color,
}

pub use crate::config::THEME_NAMES;
use crate::config::canonical_theme_name;

impl Palette {
    /// Resolve a theme by name. Returns None for unknown names.
    pub fn from_name(name: &str) -> Option<Self> {
        match canonical_theme_name(name)? {
            "catppuccin" => Some(Self::catppuccin()),
            "catppuccin-latte" => Some(Self::catppuccin_latte()),
            "terminal" => Some(Self::terminal()),
            "tokyo-night" => Some(Self::tokyo_night()),
            "tokyo-night-day" => Some(Self::tokyo_night_day()),
            "dracula" => Some(Self::dracula()),
            "nord" => Some(Self::nord()),
            "gruvbox" => Some(Self::gruvbox()),
            "gruvbox-light" => Some(Self::gruvbox_light()),
            "one-dark" => Some(Self::one_dark()),
            "one-light" => Some(Self::one_light()),
            "solarized" => Some(Self::solarized()),
            "solarized-light" => Some(Self::solarized_light()),
            "kanagawa" => Some(Self::kanagawa()),
            "kanagawa-lotus" => Some(Self::kanagawa_lotus()),
            "rose-pine" => Some(Self::rose_pine()),
            "rose-pine-dawn" => Some(Self::rose_pine_dawn()),
            "vesper" => Some(Self::vesper()),
            _ => None,
        }
    }

    /// Apply custom color overrides on top of this palette.
    pub fn with_overrides(mut self, custom: &CustomThemeColors) -> Self {
        apply_overrides(&mut self, &ModeThemeColors::from_custom(custom));
        self
    }

    pub fn with_mode_overrides(mut self, custom: &ModeThemeColors) -> Self {
        apply_overrides(&mut self, custom);
        self
    }
}

impl ModeThemeColors {
    fn from_custom(custom: &CustomThemeColors) -> Self {
        Self {
            accent: custom.accent.clone(),
            panel_bg: custom.panel_bg.clone(),
            sidebar_bg: custom.sidebar_bg.clone(),
            active_row_bg: custom.active_row_bg.clone(),
            selection_bg: custom.selection_bg.clone(),
            surface0: custom.surface0.clone(),
            surface1: custom.surface1.clone(),
            surface_dim: custom.surface_dim.clone(),
            overlay0: custom.overlay0.clone(),
            overlay1: custom.overlay1.clone(),
            text: custom.text.clone(),
            subtext0: custom.subtext0.clone(),
            mauve: custom.mauve.clone(),
            green: custom.green.clone(),
            yellow: custom.yellow.clone(),
            red: custom.red.clone(),
            blue: custom.blue.clone(),
            teal: custom.teal.clone(),
            peach: custom.peach.clone(),
        }
    }
}

fn apply_overrides(palette: &mut Palette, custom: &ModeThemeColors) {
    for (slot, value) in [
        (&mut palette.accent, &custom.accent),
        (&mut palette.panel_bg, &custom.panel_bg),
        (&mut palette.sidebar_bg, &custom.sidebar_bg),
        (&mut palette.active_row_bg, &custom.active_row_bg),
        (&mut palette.selection_bg, &custom.selection_bg),
        (&mut palette.surface0, &custom.surface0),
        (&mut palette.surface1, &custom.surface1),
        (&mut palette.surface_dim, &custom.surface_dim),
        (&mut palette.overlay0, &custom.overlay0),
        (&mut palette.overlay1, &custom.overlay1),
        (&mut palette.text, &custom.text),
        (&mut palette.subtext0, &custom.subtext0),
        (&mut palette.mauve, &custom.mauve),
        (&mut palette.green, &custom.green),
        (&mut palette.yellow, &custom.yellow),
        (&mut palette.red, &custom.red),
        (&mut palette.blue, &custom.blue),
        (&mut palette.teal, &custom.teal),
        (&mut palette.peach, &custom.peach),
    ] {
        if let Some(c) = value {
            *slot = parse_color(c);
        }
    }
}

/// Parse a color string into a ratatui Color.
/// Supports: hex (#rrggbb, #rgb), named colors, rgb(r,g,b), and reset aliases.
pub fn parse_color(s: &str) -> Color {
    let s = s.trim().to_lowercase();

    match s.as_str() {
        "reset" | "default" | "none" | "transparent" => return Color::Reset,
        _ => {}
    }

    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&hex[0..2], 16),
                u8::from_str_radix(&hex[2..4], 16),
                u8::from_str_radix(&hex[4..6], 16),
            ) {
                return Color::Rgb(r, g, b);
            }
        } else if hex.len() == 3 {
            let chars: Vec<u8> = hex
                .chars()
                .filter_map(|c| u8::from_str_radix(&c.to_string(), 16).ok())
                .collect();
            if chars.len() == 3 {
                return Color::Rgb(chars[0] * 17, chars[1] * 17, chars[2] * 17);
            }
        }
    }

    if let Some(inner) = s.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<&str> = inner.split(',').collect();
        if parts.len() == 3
            && let (Ok(r), Ok(g), Ok(b)) = (
                parts[0].trim().parse::<u8>(),
                parts[1].trim().parse::<u8>(),
                parts[2].trim().parse::<u8>(),
            )
        {
            return Color::Rgb(r, g, b);
        }
    }

    match s.as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" | "purple" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "gray" | "grey" => Color::Gray,
        "darkgray" | "darkgrey" => Color::DarkGray,
        "lightred" => Color::LightRed,
        "lightgreen" => Color::LightGreen,
        "lightyellow" => Color::LightYellow,
        "lightblue" => Color::LightBlue,
        "lightmagenta" => Color::LightMagenta,
        "lightcyan" => Color::LightCyan,
        _ => Color::Cyan,
    }
}

/// Theme resolution inputs ported from `app/mod.rs::theme_runtime_config`.
#[derive(Debug, Clone)]
pub struct ThemeRuntimeConfig {
    pub manual_name: String,
    pub dark_name: String,
    pub light_name: String,
    pub auto_switch: bool,
    pub custom: Option<CustomThemeColors>,
    pub legacy_accent: Option<String>,
}

fn sibling_theme_names(name: &str) -> (String, String) {
    match canonical_theme_name(name).unwrap_or(name) {
        "catppuccin" | "catppuccin-mocha" | "catppuccin-latte" | "latte" => {
            ("catppuccin".to_string(), "catppuccin-latte".to_string())
        }
        "tokyo-night" | "tokyonight" | "tokyo-night-day" | "tokyo-day" | "tokyonight-day" => {
            ("tokyo-night".to_string(), "tokyo-night-day".to_string())
        }
        "gruvbox" | "gruvbox-dark" | "gruvbox-light" => {
            ("gruvbox".to_string(), "gruvbox-light".to_string())
        }
        "one-dark" | "onedark" | "one-light" | "onelight" => {
            ("one-dark".to_string(), "one-light".to_string())
        }
        "solarized" | "solarized-dark" | "solarized-light" => {
            ("solarized".to_string(), "solarized-light".to_string())
        }
        "kanagawa" | "kanagawa-lotus" | "lotus" => {
            ("kanagawa".to_string(), "kanagawa-lotus".to_string())
        }
        "rose-pine" | "rosepine" | "rose-pine-dawn" | "rosepine-dawn" | "dawn" => {
            ("rose-pine".to_string(), "rose-pine-dawn".to_string())
        }
        _ => (name.to_string(), name.to_string()),
    }
}

/// Host terminal light/dark appearance for `auto_switch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostAppearance {
    Dark,
    Light,
}

impl ThemeRuntimeConfig {
    pub(crate) fn from_config(config: &crate::config::Config) -> Self {
        let manual_name = config
            .theme
            .name
            .clone()
            .unwrap_or_else(|| "catppuccin".to_string());
        let (default_dark, default_light) = sibling_theme_names(&manual_name);
        Self {
            manual_name,
            dark_name: config.theme.dark_name.clone().unwrap_or(default_dark),
            light_name: config.theme.light_name.clone().unwrap_or(default_light),
            auto_switch: config.theme.auto_switch,
            custom: config.theme.custom.clone(),
            legacy_accent: (config.ui.accent != "cyan"
                && config
                    .theme
                    .custom
                    .as_ref()
                    .and_then(|c| c.accent.as_ref())
                    .is_none())
            .then(|| config.ui.accent.clone()),
        }
    }
}

pub(crate) fn resolve_palette_for_theme_name(
    name: &str,
    fallback_name: &str,
    runtime: &ThemeRuntimeConfig,
    mode_custom: Option<&ModeThemeColors>,
) -> Palette {
    let mut palette = Palette::from_name(name)
        .unwrap_or_else(|| Palette::from_name(fallback_name).unwrap_or_else(Palette::catppuccin));

    if let Some(custom) = &runtime.custom {
        palette = palette.with_overrides(custom);
    }
    if let Some(accent) = &runtime.legacy_accent {
        palette.accent = parse_color(accent);
    }
    if let Some(custom) = mode_custom {
        palette = palette.with_mode_overrides(custom);
    }

    palette
}

/// Resolve the effective palette, honoring `auto_switch` host appearance.
pub fn resolve_effective_theme(
    runtime: &ThemeRuntimeConfig,
    appearance: Option<HostAppearance>,
) -> (Palette, String) {
    let (name, fallback, mode_custom) = if runtime.auto_switch {
        match appearance.unwrap_or(HostAppearance::Dark) {
            HostAppearance::Dark => (
                &runtime.dark_name,
                "catppuccin",
                runtime
                    .custom
                    .as_ref()
                    .and_then(|custom| custom.dark.as_ref()),
            ),
            HostAppearance::Light => (
                &runtime.light_name,
                "catppuccin-latte",
                runtime
                    .custom
                    .as_ref()
                    .and_then(|custom| custom.light.as_ref()),
            ),
        }
    } else {
        (&runtime.manual_name, "catppuccin", None)
    };

    (
        resolve_palette_for_theme_name(name, fallback, runtime, mode_custom),
        name.clone(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_is_catppuccin() {
        let runtime = ThemeRuntimeConfig::from_config(&crate::config::Config::default());
        let (palette, name) = resolve_effective_theme(&runtime, Some(HostAppearance::Dark));
        assert_eq!(name, "catppuccin");
        assert_eq!(palette.accent, Color::Rgb(137, 180, 250));
    }

    #[test]
    fn parse_color_accepts_hex_named_and_reset() {
        assert_eq!(parse_color("#ff0000"), Color::Rgb(255, 0, 0));
        assert_eq!(parse_color("#f00"), Color::Rgb(255, 0, 0));
        assert_eq!(parse_color("rgb(1, 2, 3)"), Color::Rgb(1, 2, 3));
        assert_eq!(parse_color("reset"), Color::Reset);
        assert_eq!(parse_color("darkgray"), Color::DarkGray);
        assert_eq!(parse_color("purple"), Color::Magenta);
    }
}
