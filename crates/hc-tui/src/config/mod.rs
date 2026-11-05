// Derived from herdr src/config/model.rs and src/config/io.rs (Apache-2.0).

//! Herdr-compatible config model. Parsing is tolerant: `serde(default)` fills
//! missing keys with herdr defaults, matching the reference loader. hc-tui
//! always resolves the stable `herdr` config directory — never `herdr-dev`,
//! which herdr's own debug builds select.

pub mod agent;
mod theme_names;
pub use theme_names::THEME_NAMES;
pub(crate) use theme_names::canonical_theme_name;

pub mod window_title;
pub use window_title::{WindowTitlePart, WindowTitleTemplate, WindowTitleToken};

pub mod tab_bar;
pub use tab_bar::TabBarRightEntryConfig;

mod io;
mod keybinds;
mod registry;
mod sidebar;
mod theme;

pub use io::{LoadedConfig, config_diagnostic_summary, load, load_live};

pub use keybinds::{
    ActionKeybinds, BindingConfig, BindingTrigger, CustomCommandAction, CustomCommandKeybind,
    IndexedKeybind, KeyCombo, Keybinds, LiveKeybindConfig, NavigateKeybinds, ResolvedBinding,
    TerminalKey, format_key_combo, format_prefix_combos, normalize_key_combo,
    terminal_key_matches_combo,
};
pub use sidebar::{
    AgentSidebarToken, AgentsSidebarConfig, SidebarConfig, SidebarTokenColor, SidebarTokenStyle,
    SpaceSidebarToken, SpacesSidebarConfig,
};
pub use theme::{CustomThemeColors, ModeThemeColors, ThemeConfig};

use std::num::NonZeroUsize;
use std::path::PathBuf;

use crossterm::event::KeyModifiers;
use serde::Deserialize;

pub const CONFIG_PATH_ENV_VAR: &str = "HERDR_CONFIG_PATH";
pub const DEFAULT_MOUSE_SCROLL_LINES: usize = 3;
pub const DEFAULT_MOBILE_WIDTH_THRESHOLD: u16 = 64;

/// hc-tui deliberately reads the stable `herdr` config directory in all
/// build profiles; herdr's own debug `app_dir_name` would say `herdr-dev`.
pub fn app_dir_name() -> &'static str {
    "herdr"
}

pub fn config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(dir).join(app_dir_name());
    }
    platform_config_dir()
}

fn platform_config_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(format!(".config/{}", app_dir_name()))
    } else {
        std::env::temp_dir().join(app_dir_name())
    }
}

pub fn config_path() -> PathBuf {
    if let Ok(path) = std::env::var(CONFIG_PATH_ENV_VAR) {
        return PathBuf::from(path);
    }
    config_dir().join("config.toml")
}

/// Validate `[ui]` sidebar bound configuration. Returns `Some((min, max))`
/// when `min <= max`, `None` otherwise.
pub fn validated_sidebar_bounds(min: u16, max: u16) -> Option<(u16, u16)> {
    if min <= max { Some((min, max)) } else { None }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub onboarding: Option<bool>,
    pub theme: ThemeConfig,
    pub update: UpdateConfig,
    pub keys: KeysConfig,
    pub ui: UiConfig,
    pub worktrees: WorktreesConfig,
    pub experimental: ExperimentalConfig,
}

impl Config {
    /// All configured prefix keys; falls back to `ctrl+b` when none parse.
    pub fn prefix_keys(&self) -> Vec<keybinds::KeyCombo> {
        self.validated_keybinds().1
    }

    /// Resolve every action binding through the ported registry.
    pub fn keybinds(&self) -> Keybinds {
        self.validated_keybinds().3
    }

    /// The reference `collect_diagnostics`: binding parse/conflict findings
    /// plus model-level checks for the surfaces hc-tui consumes.
    pub fn collect_diagnostics(&self) -> Vec<String> {
        let (prefix_diag, _, keybind_diags, _) = self.validated_keybinds();
        prefix_diag
            .into_iter()
            .chain(keybind_diags)
            .chain(self.remote_image_paste_key().err())
            .chain(self.theme.diagnostics())
            .chain(tab_bar::tab_bar_right_diagnostics(&self.ui.tab_bar_right))
            .chain(self.invalid_sidebar_bounds_diagnostic())
            .chain(window_title::window_title_diagnostics(
                &self.ui.window_title,
            ))
            .collect()
    }

    pub(crate) fn invalid_sidebar_bounds_diagnostic(&self) -> Option<String> {
        validated_sidebar_bounds(self.ui.sidebar_min_width, self.ui.sidebar_max_width)
            .is_none()
            .then(|| {
                format!(
                    "ui.sidebar_min_width ({}) is greater than sidebar_max_width ({})",
                    self.ui.sidebar_min_width, self.ui.sidebar_max_width
                )
            })
    }

    pub(crate) fn remote_image_paste_key(&self) -> Result<Option<keybinds::KeyCombo>, String> {
        let raw = self.keys.remote_image_paste.trim();
        if raw.is_empty() {
            return Ok(None);
        }
        keybinds::parse_key_combo(raw).map(Some).ok_or_else(|| {
            format!("invalid keybinding: keys.remote_image_paste = {raw:?}; disabling binding")
        })
    }

    pub fn live_keybinds(&self) -> LiveKeybindConfig {
        let (_, prefix, _, keybinds) = self.validated_keybinds();
        LiveKeybindConfig { prefix, keybinds }
    }

    // Derived from herdr src/config.rs (Apache-2.0).
    pub(crate) fn live_keybinds_with_diagnostics(
        &self,
    ) -> Result<(LiveKeybindConfig, Vec<String>), Vec<String>> {
        let (prefix_diag, prefix, diagnostics, keybinds) = self.validated_keybinds();
        if let Some(prefix_diag) = prefix_diag {
            Err(std::iter::once(prefix_diag).chain(diagnostics).collect())
        } else {
            Ok((LiveKeybindConfig { prefix, keybinds }, diagnostics))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct UpdateConfig {
    pub channel: UpdateChannelConfig,
    pub version_check: bool,
    pub manifest_check: bool,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            channel: UpdateChannelConfig::default(),
            version_check: true,
            manifest_check: true,
        }
    }
}

mod ui_model;
pub use ui_model::*;

mod keys_model;
pub use keys_model::*;

mod ui_options;
pub use ui_options::*;
