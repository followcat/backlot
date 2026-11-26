// Derived from herdr src/client/shell/settings.rs, overlay_input.rs and
// mouse.rs (Apache-2.0).
//
//! The Settings overlay's input and apply path: section/choice
//! navigation, live theme preview, the `write_edit` save plus the
//! client-local config reload. The reference also pushes
//! `ServerReloadConfig` to the endpoint; hc's core has no such method —
//! the reload stays local (parity.md).
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Position, Rect};

use crate::chrome::settings::{
    SettingsOverlay, SettingsSection, indicator_index, theme_index, toast_index,
};

use super::App;
use super::input::{MouseButton, MouseEventKind, MouseInput, TerminalKey};

fn contains(rect: Rect, point: (u16, u16)) -> bool {
    rect.contains(Position {
        x: point.0,
        y: point.1,
    })
}

impl App {
    /// `open_settings_overlay` (`prefix+s`): the Theme section selected
    /// on the configured theme, with the restore snapshot taken.
    pub(super) fn open_settings_overlay(&mut self) {
        self.release_pane_mouse();
        // R3-M15: the studio tab re-reads `[hollywood]` — an edit
        // made since start lands without a restart.
        self.reload_hollywood_cfg();
        self.settings = Some(SettingsOverlay {
            section: SettingsSection::Theme,
            selected: theme_index(self.shell.own_theme.names(), &self.shell.theme_name),
            sections: SettingsSection::visible(self.studio.active),
            studio: self.studio_view(),
            original_theme_name: self.shell.theme_name.clone(),
            theme_names: self.shell.own_theme.names(),
            original_palette: self.shell.palette.clone(),
            integrations: Vec::new(),
            loading_integrations: false,
            installing_integrations: false,
        });
    }

    /// The lot menu's `Hollywood settings…` (S5 §2.3): the overlay
    /// opened straight on the Studio section — the start policy,
    /// `Alt+Y`, dock width and the `[hollywood]` overseer row.
    pub(super) fn open_studio_settings(&mut self) {
        self.open_settings_overlay();
        self.select_settings_section(SettingsSection::Studio);
    }

    /// The studio tab's current values — `None` outside studio mode.
    fn studio_view(&self) -> Option<crate::chrome::settings::StudioView> {
        self.studio
            .active
            .then(|| crate::chrome::settings::StudioView {
                start: self.panel_config.call_sheet_start,
                alt_y: self.hollywood_key.is_some_and(|keys| keys.direct.is_some()),
                dock_width: self.panel_config.width,
                overseer: self.hollywood_overseer.clone(),
                setup: (self.hollywood_overseer.is_none()).then(|| {
                    format!(
                        "not configured · backlot hollywood setup --runner {} --model <m>",
                        self.setup_runner_hint.as_deref().unwrap_or("<r>")
                    )
                }),
            })
    }

    fn selected_index_for_settings_section(&self, section: SettingsSection) -> usize {
        match section {
            SettingsSection::Theme => {
                theme_index(self.shell.own_theme.names(), &self.shell.theme_name)
            }
            SettingsSection::Indicators => indicator_index(self.shell.status_indicators),
            SettingsSection::Sound => usize::from(!self.shell.sound.enabled),
            SettingsSection::Toast => toast_index(self.shell.toast_delivery),
            SettingsSection::Integrations | SettingsSection::Studio => 0,
        }
    }

    /// `select_settings_section`; the reference's integration list is
    /// requested here too — hc's core has no `integration.list` method,
    /// so the section renders its unavailable state permanently.
    pub(super) fn select_settings_section(&mut self, section: SettingsSection) {
        let selected = self.selected_index_for_settings_section(section);
        if let Some(settings) = self.settings.as_mut() {
            settings.section = section;
            settings.selected = selected;
        }
    }

    fn move_settings_section(&mut self, delta: isize) {
        let Some(settings) = self.settings.as_ref() else {
            return;
        };
        let current = settings
            .sections
            .iter()
            .position(|section| *section == settings.section)
            .unwrap_or(0);
        let next = (current as isize + delta).rem_euclid(settings.sections.len() as isize) as usize;
        let next = settings.sections[next];
        self.select_settings_section(next);
    }

    fn settings_choice_count(&self) -> usize {
        match self.settings.as_ref() {
            Some(settings) => match settings.section {
                SettingsSection::Theme => settings.theme_names.len(),
                SettingsSection::Indicators | SettingsSection::Sound => 2,
                SettingsSection::Toast => 4,
                SettingsSection::Integrations => settings.integrations.len(),
                SettingsSection::Studio => crate::chrome::settings::STUDIO_ROWS,
            },
            _ => 0,
        }
    }

    pub(super) fn move_settings_selection(&mut self, delta: isize) {
        let count = self.settings_choice_count();
        let Some(settings) = self.settings.as_mut() else {
            return;
        };
        if count == 0 {
            settings.selected = 0;
            return;
        }
        settings.selected = (settings.selected as isize + delta)
            .clamp(0, count.saturating_sub(1) as isize) as usize;
        if settings.section == SettingsSection::Theme {
            self.preview_selected_theme();
        }
    }

    pub(super) fn select_settings_choice(&mut self, index: usize) {
        let count = self.settings_choice_count();
        if let Some(settings) = self.settings.as_mut()
            && count > 0
        {
            settings.selected = index.min(count - 1);
        }
        if matches!(
            self.settings,
            Some(SettingsOverlay {
                section: SettingsSection::Theme,
                ..
            })
        ) {
            self.preview_selected_theme();
        }
    }

    /// `preview_selected_theme`: the whole shell repaints against the
    /// highlighted theme before it is saved.
    fn preview_selected_theme(&mut self) {
        let Some(settings) = self.settings.as_ref() else {
            return;
        };
        let Some(name) = settings.theme_names.get(settings.selected) else {
            return;
        };
        self.shell.theme_name = (*name).to_owned();
        self.shell.palette = crate::chrome::palette::resolve_palette_for_theme_name(
            name,
            "catppuccin",
            &self.shell.theme_runtime,
            None,
        );
        self.shell.studio_injected_sidebar_bg = false;
        self.sync_studio_chrome();
    }

    /// `cancel_settings_overlay`: Esc or an outside click restores the
    /// theme snapshot — unsaved previews never stick.
    pub(super) fn cancel_settings_overlay(&mut self) {
        let Some(settings) = self.settings.take() else {
            return;
        };
        self.shell.theme_name = settings.original_theme_name;
        self.shell.palette = settings.original_palette;
        self.shell.studio_injected_sidebar_bg = false;
        self.sync_studio_chrome();
    }

    /// `save_settings_edit`: write the TOML first, then reload the live
    /// config the same way `reload-config` does. hc's core has no
    /// `server.reload-config` method; the reload is client-local.
    fn save_settings_edit(&mut self, edit: crate::config::write::ConfigEdit<'_>) -> bool {
        // A theme goes to the file the theme is read from (backlot's own
        // unless `source = "herdr"`); a backlot edit goes to backlot's
        // own `config.toml`; everything else stays in herdr's.
        let path = if edit.is_backlot() {
            &self.panel_config_path
        } else {
            match edit {
                crate::config::write::ConfigEdit::Theme(_) => self
                    .shell
                    .own_theme
                    .save_path(&self.shell.local_config_path),
                _ => &self.shell.local_config_path,
            }
        };
        if let Err(error) = crate::config::write::write_edit_at(path, edit) {
            self.show_notice("Settings", &error);
            return false;
        }
        self.reload_config();
        true
    }

    /// `apply_settings_choice` — Enter/apply on the highlighted row.
    pub(super) fn apply_settings_choice(&mut self) {
        let Some(settings) = self.settings.as_ref() else {
            return;
        };
        let section = settings.section;
        let selected = settings.selected;
        match section {
            SettingsSection::Theme => {
                let Some(name) = settings.theme_names.get(selected).copied() else {
                    return;
                };
                if self.save_settings_edit(crate::config::write::ConfigEdit::Theme(name)) {
                    self.settings = None;
                }
            }
            SettingsSection::Indicators => {
                let style = if selected == 0 {
                    crate::config::StatusIndicatorStyle::Dots
                } else {
                    crate::config::StatusIndicatorStyle::Symbols
                };
                self.save_settings_edit(crate::config::write::ConfigEdit::StatusIndicators(style));
            }
            SettingsSection::Sound => {
                self.save_settings_edit(crate::config::write::ConfigEdit::Sound(selected == 0));
            }
            SettingsSection::Toast => {
                let delivery = match selected {
                    0 => crate::config::ToastDelivery::Off,
                    1 => crate::config::ToastDelivery::Herdr,
                    2 => crate::config::ToastDelivery::Terminal,
                    _ => crate::config::ToastDelivery::System,
                };
                self.save_settings_edit(crate::config::write::ConfigEdit::ToastDelivery(delivery));
            }
            // No `integration.install` method exists — Enter is inert.
            SettingsSection::Integrations => {}
            SettingsSection::Studio => self.apply_studio_row(selected),
        }
    }

    /// The studio tab's rows: the start policy and dock width cycle
    /// their choices, `Alt+Y` toggles; the overseer row opens the
    /// setup dialog (R3-5). Every write lands in backlot's own
    /// `config.toml`.
    fn apply_studio_row(&mut self, row: usize) {
        use crate::chrome::settings::{
            STUDIO_ALT_Y, STUDIO_DOCK_WIDTH, STUDIO_OVERSEER, STUDIO_START,
        };
        use crate::panel::CallSheetStart;
        match row {
            STUDIO_START => {
                let next = match self.panel_config.call_sheet_start {
                    CallSheetStart::Collapsed => CallSheetStart::WhenWaiting,
                    CallSheetStart::WhenWaiting => CallSheetStart::Expanded,
                    CallSheetStart::Expanded => CallSheetStart::Collapsed,
                };
                self.panel_config.call_sheet_start = next;
                self.save_settings_edit(crate::config::write::ConfigEdit::CallSheetStart(
                    next.as_str(),
                ));
            }
            STUDIO_ALT_Y => {
                let on = self.hollywood_key.is_some_and(|keys| keys.direct.is_some());
                if on {
                    self.panel_config.keys_call_sheet = Some(String::new());
                    self.hollywood_key =
                        self.hollywood_key
                            .map(|keys| crate::hollywood::CallSheetKeys {
                                direct: None,
                                ..keys
                            });
                } else {
                    self.panel_config.keys_call_sheet = None;
                    if let Some(combo) = crate::config::parse_key_combo("alt+y") {
                        self.hollywood_key = Some(crate::hollywood::CallSheetKeys {
                            direct: Some(combo),
                            chord: self.hollywood_key.and_then(|keys| keys.chord),
                        });
                    }
                }
                self.save_settings_edit(crate::config::write::ConfigEdit::CallSheetToggle(!on));
            }
            STUDIO_DOCK_WIDTH => {
                let next = match self.panel_config.width {
                    36 => 42,
                    42 => 48,
                    _ => 36,
                };
                self.panel_config.width = next;
                self.save_settings_edit(crate::config::write::ConfigEdit::DockWidth(next));
            }
            // The overseer row opens the setup dialog — prefilled
            // when `[hollywood]` exists, a first setup without one.
            STUDIO_OVERSEER => self.hw_setup_open(),
            _ => {}
        }
        let view = self.studio_view();
        if let Some(settings) = self.settings.as_mut() {
            settings.studio = view;
        }
    }

    /// `route_settings_key` — while the overlay is open it owns every key.
    pub(super) fn route_settings_key(&mut self, key: &TerminalKey) -> bool {
        if self.settings.is_none() {
            return false;
        }
        let combo = crate::input::terminal_key_of(key);
        let (code, modifiers) = crate::config::normalize_key_combo((combo.code, combo.modifiers));
        if code == KeyCode::Esc {
            if !matches!(
                self.settings,
                Some(SettingsOverlay {
                    installing_integrations: true,
                    ..
                })
            ) {
                self.cancel_settings_overlay();
            }
            return true;
        }
        if matches!(code, KeyCode::Tab | KeyCode::Right | KeyCode::Char('l'))
            && modifiers.is_empty()
        {
            self.move_settings_section(1);
            return true;
        }
        if matches!(code, KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h'))
            && modifiers.difference(KeyModifiers::SHIFT).is_empty()
        {
            self.move_settings_section(-1);
            return true;
        }
        if matches!(code, KeyCode::Up | KeyCode::Char('k')) && modifiers.is_empty() {
            self.move_settings_selection(-1);
            return true;
        }
        if matches!(code, KeyCode::Down | KeyCode::Char('j')) && modifiers.is_empty() {
            self.move_settings_selection(1);
            return true;
        }
        if matches!(code, KeyCode::Enter | KeyCode::Char(' ')) && modifiers.is_empty() {
            self.apply_settings_choice();
            return true;
        }
        true
    }

    /// The reference's Settings mouse branch: tab and choice clicks,
    /// immediate apply for the radio sections, primary/cancel buttons,
    /// and an outside press that cancels.
    pub(super) fn handle_settings_mouse(&mut self, mouse: MouseInput) {
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            return;
        }
        let point = (mouse.col, mouse.row);
        if let Some((_, section)) = self
            .settings_hits
            .tabs
            .iter()
            .find(|(rect, _)| contains(*rect, point))
            .copied()
        {
            self.select_settings_section(section);
        } else if let Some((_, index)) = self
            .settings_hits
            .choices
            .iter()
            .find(|(rect, _)| contains(*rect, point))
            .copied()
        {
            self.select_settings_choice(index);
            let immediate = matches!(
                self.settings,
                Some(SettingsOverlay {
                    section: SettingsSection::Indicators
                        | SettingsSection::Sound
                        | SettingsSection::Toast,
                    ..
                })
            );
            if immediate {
                self.apply_settings_choice();
            }
        } else if contains(self.settings_hits.primary, point) {
            self.apply_settings_choice();
        } else if contains(self.settings_hits.cancel, point)
            || !contains(self.settings_hits.popup, point)
        {
            let installing = matches!(
                self.settings,
                Some(SettingsOverlay {
                    installing_integrations: true,
                    ..
                })
            );
            if !installing {
                self.cancel_settings_overlay();
            }
        }
    }
}
