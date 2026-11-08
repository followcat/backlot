//! Herdr shell chrome ported to hc-tui. Every file names its herdr source;
//! the snapshot model is neutral so `watch::View` data projects onto herdr's
//! presentation without inventing hc-tui-specific output.

pub mod agents;
pub(crate) mod config_diagnostic;
pub mod detect;
pub(crate) mod dialogs;
pub mod endpoint_agents;
pub mod endpoint_sidebar;
pub mod endpoint_sidebar_expanded;
pub mod endpoints;
pub(crate) mod help;
pub(crate) mod menu;
pub mod mobile;
pub mod mobile_items;
pub mod mobile_switcher;
pub mod modebar;
pub mod navigate;
pub(crate) mod navigator;
pub(crate) mod navigator_render;
pub(crate) mod notice_render;
pub mod palette;
pub mod panes;
pub mod panes_draw;
pub mod render;
pub mod scroll;
pub mod sidebar_render;
pub mod snapshot;
pub mod spans;
pub mod status;
pub mod tabs;
pub mod text;
pub(crate) mod text_editor;
pub(crate) mod title_text;
pub mod tokens;
pub mod workspace;

use std::path::PathBuf;

use crate::config::{
    AgentPanelSortConfig, AgentsSidebarConfig, LiveKeybindConfig, PaneBordersConfig,
    SidebarCollapsedModeConfig, SpacesSidebarConfig, StatusIndicatorStyle, TabBarPositionConfig,
    ToastDelivery, ToastHerdrPosition,
};
use palette::{Palette, ThemeRuntimeConfig};

/// The display-relevant configuration every chrome renderer consumes, built
/// from `Config` once per app run. Field-for-field mirror of the reference
/// `ClientShellConfig` subset hc-tui implements.
#[derive(Debug, Clone)]
pub struct ClientShellConfig {
    pub sidebar_width: u16,
    pub sidebar_min_width: u16,
    pub sidebar_max_width: u16,
    pub sidebar_start_collapsed: bool,
    pub sidebar_collapsed_mode: SidebarCollapsedModeConfig,
    pub mobile_width_threshold: u16,
    pub tab_bar_position: TabBarPositionConfig,
    pub hide_tab_bar_when_single_tab: bool,
    pub spaces: SpacesSidebarConfig,
    pub agents: AgentsSidebarConfig,
    pub agent_panel_sort: AgentPanelSortConfig,
    pub status_indicators: StatusIndicatorStyle,
    pub theme_name: String,
    pub theme_runtime: ThemeRuntimeConfig,
    pub palette: Palette,
    pub keybinds: LiveKeybindConfig,
    pub mouse_capture: bool,
    pub draw_host_cursor: bool,
    pub window_title: Option<crate::config::WindowTitleTemplate>,
    pub tab_bar_right: Vec<crate::config::TabBarRightEntryConfig>,
    pub tab_bar_right_separator: String,
    pub mouse_scroll_lines: usize,
    pub right_click_passthrough_modifiers: Option<hc_vt::Mods>,
    pub prompt_new_tab_name: bool,
    pub prompt_new_workspace_name: bool,
    pub confirm_close: bool,
    /// The `[ui.toast]` projection the notification policy consumes.
    pub toast_delivery: ToastDelivery,
    pub toast_delay_seconds: u64,
    pub toast_position: ToastHerdrPosition,
    /// `[ui.sound]` — whether synthesized notification sounds ring the bell.
    pub sound: crate::config::SoundConfig,
    pub pane_borders: PaneBordersConfig,
    pub pane_gaps: bool,
    pub pane_outer_borders: bool,
    pub pane_scrollbars: bool,
    pub show_agent_labels_on_pane_borders: bool,
    pub startup_config_diagnostic: Option<String>,
    pub local_config_path: PathBuf,
}

impl ClientShellConfig {
    pub(crate) fn layout_config(&self) -> snapshot::ClientShellLayoutConfig {
        snapshot::ClientShellLayoutConfig {
            sidebar_min_width: self.sidebar_min_width,
            sidebar_max_width: self.sidebar_max_width,
            sidebar_collapsed_mode: self.sidebar_collapsed_mode,
            mobile_width_threshold: self.mobile_width_threshold,
            tab_bar_position: self.tab_bar_position,
            hide_tab_bar_when_single_tab: self.hide_tab_bar_when_single_tab,
        }
    }

    /// Build the immutable shell config from the loaded user `Config` —
    /// the same projection the reference `ClientShellConfig` performs.
    /// `diagnostic` is the startup config-diagnostic banner, if any.
    pub fn from_config(
        config: &crate::config::Config,
        appearance: Option<palette::HostAppearance>,
        diagnostic: Option<String>,
        local_config_path: std::path::PathBuf,
    ) -> Self {
        let ui = &config.ui;
        let theme_runtime = palette::ThemeRuntimeConfig::from_config(config);
        let (resolved, theme_name) = palette::resolve_effective_theme(&theme_runtime, appearance);
        Self {
            sidebar_width: ui.sidebar_width,
            sidebar_min_width: ui.sidebar_min_width,
            sidebar_max_width: ui.sidebar_max_width,
            sidebar_start_collapsed: ui.sidebar_start_collapsed,
            sidebar_collapsed_mode: ui.sidebar_collapsed_mode,
            mobile_width_threshold: ui.mobile_width_threshold,
            tab_bar_position: ui.tab_bar_position,
            hide_tab_bar_when_single_tab: ui.hide_tab_bar_when_single_tab,
            spaces: ui.sidebar.spaces.clone(),
            agents: ui.sidebar.agents.clone(),
            agent_panel_sort: ui.agent_panel_sort,
            status_indicators: ui.status_indicators,
            theme_name,
            theme_runtime,
            palette: resolved,
            keybinds: config.live_keybinds(),
            mouse_capture: ui.mouse_capture,
            window_title: crate::config::WindowTitleTemplate::parse(&ui.window_title)
                .ok()
                .flatten(),
            draw_host_cursor: match ui.host_cursor {
                crate::config::HostCursorModeConfig::Drawn => true,
                crate::config::HostCursorModeConfig::Native => false,
                crate::config::HostCursorModeConfig::Auto => host_cursor_auto_drawn(),
            },
            tab_bar_right: ui.tab_bar_right.clone(),
            tab_bar_right_separator: ui.tab_bar_right_separator.clone(),
            mouse_scroll_lines: ui.mouse_scroll_lines(),
            right_click_passthrough_modifiers: ui.right_click_passthrough_modifiers().map(|mods| {
                let mut value = hc_vt::Mods::NONE;
                for (key, bit) in [
                    (crossterm::event::KeyModifiers::SHIFT, hc_vt::Mods::SHIFT),
                    (crossterm::event::KeyModifiers::CONTROL, hc_vt::Mods::CTRL),
                    (crossterm::event::KeyModifiers::ALT, hc_vt::Mods::ALT),
                    (crossterm::event::KeyModifiers::SUPER, hc_vt::Mods::SUPER),
                ] {
                    if mods.contains(key) {
                        value = value.union(bit);
                    }
                }
                value
            }),
            prompt_new_tab_name: ui.prompt_new_tab_name,
            prompt_new_workspace_name: ui.prompt_new_workspace_name,
            confirm_close: ui.confirm_close,
            toast_delivery: ui.toast.delivery,
            toast_delay_seconds: ui.toast.delay_seconds,
            toast_position: ui.toast.herdr.position,
            sound: ui.sound.clone(),
            pane_borders: ui.pane_borders,
            pane_gaps: ui.pane_gaps,
            pane_outer_borders: ui.pane_outer_borders,
            pane_scrollbars: ui.pane_scrollbars,
            show_agent_labels_on_pane_borders: ui.show_agent_labels_on_pane_borders,
            startup_config_diagnostic: diagnostic,
            local_config_path,
        }
    }
}

// Derived from herdr src/platform/linux.rs (Apache-2.0).
fn host_cursor_auto_drawn() -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    ["/proc/sys/kernel/osrelease", "/proc/version"]
        .iter()
        .any(|path| {
            std::fs::read_to_string(path).is_ok_and(|text| {
                let text = text.to_ascii_lowercase();
                text.contains("microsoft") || text.contains("wsl")
            })
        })
        || ["WSL_DISTRO_NAME", "WSL_INTEROP"]
            .iter()
            .any(|key| std::env::var_os(key).is_some())
        || std::path::Path::new("/run/WSL").exists()
}
