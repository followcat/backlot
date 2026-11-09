// Derived from herdr src/client/shell/config.rs and actions.rs (Apache-2.0).
use super::*;

impl App {
    pub(super) fn reload_config(&mut self) {
        let loaded = match crate::config::load_live(&self.shell.local_config_path) {
            Ok(loaded) => loaded,
            Err(diagnostics) => {
                self.shell.startup_config_diagnostic =
                    crate::config::config_diagnostic_summary(&diagnostics);
                self.sched.mark_dirty(Instant::now());
                return;
            }
        };
        let previous = self.shell.clone();
        let mut diagnostics = loaded.diagnostics;
        let invalid = |name: &str| {
            loaded
                .invalid_sections
                .iter()
                .any(|section| section == name)
        };
        let mut next = ClientShellConfig::from_config(
            &loaded.config,
            self.host_appearance,
            None,
            previous.local_config_path.clone(),
        );
        if invalid("ui") {
            next = previous.clone();
        } else if let Some(diagnostic) = loaded.config.invalid_sidebar_bounds_diagnostic() {
            diagnostics.push(format!("{diagnostic}; keeping previous [ui] settings"));
            next = previous.clone();
        }
        // Reload does not reset startup preferences or a user's current collapse state.
        next.sidebar_start_collapsed = previous.sidebar_start_collapsed;
        next.keybinds = previous.keybinds.clone();
        if invalid("keys") {
            next.remote_image_paste_key = previous.remote_image_paste_key;
        } else {
            match loaded.config.live_keybinds_with_diagnostics() {
                Ok((keybinds, notices)) => {
                    next.keybinds = keybinds;
                    diagnostics.extend(notices);
                }
                Err(notices) => diagnostics.extend(
                    notices
                        .into_iter()
                        .map(|notice| format!("{notice}; kept current keybinds")),
                ),
            }
        }
        if invalid("theme") {
            next.theme_runtime = previous.theme_runtime;
            next.theme_name = previous.theme_name;
            next.palette = previous.palette;
        } else {
            next.theme_runtime =
                crate::chrome::palette::ThemeRuntimeConfig::from_config(&loaded.config);
            (next.palette, next.theme_name) = crate::chrome::palette::resolve_effective_theme(
                &next.theme_runtime,
                self.host_appearance,
            );
        }
        next.startup_config_diagnostic = crate::config::config_diagnostic_summary(&diagnostics);
        self.live = next.keybinds.clone();
        if !self.sidebar_width_manual {
            self.sidebar_width = next.sidebar_width;
        }
        if self.agent_panel_sort_manual {
            next.agent_panel_sort = previous.agent_panel_sort;
        }
        self.shell = next;
        self.prefix.reset();
        self.release_pane_mouse();
        self.chrome_drag = None;
        self.tab_press = None;
        self.workspace_press = None;
        self.tab_dragged = false;
        self.tab_drag_index = None;
        self.shell_state.reveal_focused_tab = true;
        self.shell_state.reveal_focused_workspace = true;
        self.last_window_title = None;
        self.refresh_projection();
        self.sync_pixel_mouse();
        self.sched.mark_dirty(Instant::now());
    }
}
