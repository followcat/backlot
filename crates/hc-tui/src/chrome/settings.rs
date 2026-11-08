// Derived from herdr src/client/shell/settings.rs and state.rs (Apache-2.0).
//
//! The Settings overlay's state model: section selection, per-section
//! choice indices and the original-theme restore snapshot. hc's core has
//! no integration methods, so `integrations` stays empty and the
//! Integrations section renders its unavailable state (parity.md).
use super::palette::Palette;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsSection {
    Theme,
    Indicators,
    Sound,
    Toast,
    Integrations,
}

impl SettingsSection {
    pub(crate) const ALL: &[Self] = &[
        Self::Theme,
        Self::Indicators,
        Self::Sound,
        Self::Toast,
        Self::Integrations,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Theme => "theme",
            Self::Indicators => "indicators",
            Self::Sound => "sound",
            Self::Toast => "toasts",
            Self::Integrations => "integrations",
        }
    }
}

/// The reference `api::schema::IntegrationState`. hc's core publishes no
/// integration rows; the enum survives so the render port stays verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IntegrationState {
    #[allow(dead_code)]
    Current,
    Outdated,
    NotInstalled,
}

/// The reference `api::schema::IntegrationInfo` row; never populated.
#[derive(Debug)]
pub(crate) struct IntegrationInfo {
    pub state: IntegrationState,
    pub available: bool,
    #[allow(dead_code)]
    pub label: &'static str,
}

#[derive(Debug)]
pub(crate) struct SettingsOverlay {
    pub section: SettingsSection,
    pub selected: usize,
    pub original_theme_name: String,
    pub original_palette: Palette,
    /// Always empty in hc-tui — there is no `integration.list` method to
    /// populate it; the section renders "no integration targets available".
    pub integrations: Vec<IntegrationInfo>,
    pub loading_integrations: bool,
    pub installing_integrations: bool,
}

pub(crate) fn normalized_theme_name(name: &str) -> String {
    name.to_lowercase().replace([' ', '_'], "-")
}

pub(crate) fn theme_index(name: &str) -> usize {
    let normalized = normalized_theme_name(name);
    crate::config::THEME_NAMES
        .iter()
        .position(|candidate| normalized_theme_name(candidate) == normalized)
        .unwrap_or(0)
}

pub(crate) fn indicator_index(style: crate::config::StatusIndicatorStyle) -> usize {
    usize::from(style == crate::config::StatusIndicatorStyle::Symbols)
}

pub(crate) fn toast_index(delivery: crate::config::ToastDelivery) -> usize {
    match delivery {
        crate::config::ToastDelivery::Off => 0,
        crate::config::ToastDelivery::Herdr => 1,
        crate::config::ToastDelivery::Terminal => 2,
        crate::config::ToastDelivery::System => 3,
    }
}

pub(crate) fn integration_needs_install(info: &IntegrationInfo) -> bool {
    info.state == IntegrationState::Outdated
        || info.available && info.state == IntegrationState::NotInstalled
}
