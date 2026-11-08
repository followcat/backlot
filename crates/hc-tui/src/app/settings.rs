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
        self.settings = Some(SettingsOverlay {
            section: SettingsSection::Theme,
            selected: theme_index(&self.shell.theme_name),
            original_theme_name: self.shell.theme_name.clone(),
            original_palette: self.shell.palette.clone(),
            integrations: Vec::new(),
            loading_integrations: false,
            installing_integrations: false,
        });
    }

    fn selected_index_for_settings_section(&self, section: SettingsSection) -> usize {
        match section {
            SettingsSection::Theme => theme_index(&self.shell.theme_name),
            SettingsSection::Indicators => indicator_index(self.shell.status_indicators),
            SettingsSection::Sound => usize::from(!self.shell.sound.enabled),
            SettingsSection::Toast => toast_index(self.shell.toast_delivery),
            SettingsSection::Integrations => 0,
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
        let current = SettingsSection::ALL
            .iter()
            .position(|section| *section == settings.section)
            .unwrap_or(0);
        let next =
            (current as isize + delta).rem_euclid(SettingsSection::ALL.len() as isize) as usize;
        self.select_settings_section(SettingsSection::ALL[next]);
    }

    fn settings_choice_count(&self) -> usize {
        match self.settings.as_ref() {
            Some(settings) => match settings.section {
                SettingsSection::Theme => crate::config::THEME_NAMES.len(),
                SettingsSection::Indicators | SettingsSection::Sound => 2,
                SettingsSection::Toast => 4,
                SettingsSection::Integrations => settings.integrations.len(),
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
        let Some(name) = crate::config::THEME_NAMES.get(settings.selected) else {
            return;
        };
        self.shell.theme_name = (*name).to_owned();
        self.shell.palette = crate::chrome::palette::resolve_palette_for_theme_name(
            name,
            "catppuccin",
            &self.shell.theme_runtime,
            None,
        );
    }

    /// `cancel_settings_overlay`: Esc or an outside click restores the
    /// theme snapshot — unsaved previews never stick.
    pub(super) fn cancel_settings_overlay(&mut self) {
        let Some(settings) = self.settings.take() else {
            return;
        };
        self.shell.theme_name = settings.original_theme_name;
        self.shell.palette = settings.original_palette;
    }

    /// `save_settings_edit`: write the TOML first, then reload the live
    /// config the same way `reload-config` does. hc's core has no
    /// `server.reload-config` method; the reload is client-local.
    fn save_settings_edit(&mut self, edit: crate::config::write::ConfigEdit<'_>) -> bool {
        if let Err(error) = crate::config::write::write_edit_at(&self.shell.local_config_path, edit)
        {
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
                let Some(name) = crate::config::THEME_NAMES.get(selected).copied() else {
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
