// Derived from herdr src/client/shell/overlay_input.rs and mouse.rs (Apache-2.0).
use super::{
    App,
    input::{MouseButton, MouseEventKind, MouseInput, TerminalKey},
};
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Position, Rect};

impl App {
    pub(super) fn handle_help_key(&mut self, key: &TerminalKey) {
        let combo = crate::input::terminal_key_of(key);
        let (code, modifiers) = crate::config::normalize_key_combo((combo.code, combo.modifiers));
        let character = crate::keybind_help::keybind_help_text_char(key);
        if self.help_state.search_focused {
            if let Some(changed) = self.help_state.query.handle_key(key) {
                if changed {
                    self.help_state.scroll = 0;
                }
                return;
            }
            match code {
                KeyCode::Esc => self.help_back(),
                KeyCode::Enter => self.help = false,
                KeyCode::Up
                | KeyCode::Down
                | KeyCode::PageUp
                | KeyCode::PageDown
                | KeyCode::Char('n' | 'p')
                    if !matches!(code, KeyCode::Char(_)) || modifiers == KeyModifiers::CONTROL =>
                {
                    let delta = match code {
                        KeyCode::Up | KeyCode::Char('p') => -1,
                        KeyCode::Down | KeyCode::Char('n') => 1,
                        KeyCode::PageUp => -8,
                        _ => 8,
                    };
                    self.scroll_help(delta);
                }
                _ => {}
            }
            return;
        }
        match code {
            KeyCode::Esc | KeyCode::Enter => self.help = false,
            KeyCode::Home => self.help_state.scroll = 0,
            KeyCode::End => self.help_state.scroll = self.help_hits.max_scroll,
            KeyCode::Up | KeyCode::Char('k') => self.scroll_help(-1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_help(1),
            KeyCode::PageUp => self.scroll_help(-8),
            KeyCode::PageDown => self.scroll_help(8),
            _ if character == Some('/') => {
                self.help_state.search_focused = true;
                self.help_state.scroll = 0;
            }
            _ if character == Some('?') => self.help = false,
            _ => {}
        }
    }

    fn help_back(&mut self) {
        if self.help_state.search_focused {
            self.help_state.search_focused = false;
            self.help_state.query.clear();
            self.help_state.scroll = 0;
        } else {
            self.help = false;
        }
    }

    fn scroll_help(&mut self, delta: isize) {
        self.help_state.scroll = self
            .help_state
            .scroll
            .saturating_add_signed(delta)
            .min(self.help_hits.max_scroll);
    }

    pub(super) fn handle_help_mouse(&mut self, mouse: MouseInput) {
        let contains = |rect: Rect| rect.contains(Position::new(mouse.col, mouse.row));
        match mouse.kind {
            MouseEventKind::ScrollUp => self.scroll_help(-3),
            MouseEventKind::ScrollDown => self.scroll_help(3),
            MouseEventKind::Down(MouseButton::Left) => {
                if contains(self.help_hits.scrollbar) {
                    if let Some(metrics) = self.help_hits.metrics {
                        self.help_scroll_grab = crate::chrome::scroll::scrollbar_thumb_grab_offset(
                            metrics,
                            self.help_hits.scrollbar,
                            mouse.row,
                        );
                        if self.help_scroll_grab.is_none() {
                            let offset = crate::chrome::scroll::scrollbar_offset_from_row(
                                metrics,
                                self.help_hits.scrollbar,
                                mouse.row,
                            );
                            self.help_state.scroll =
                                metrics.max_offset_from_bottom.saturating_sub(offset);
                        }
                    }
                } else if contains(self.help_hits.cancel) {
                    self.help_back();
                } else if !contains(self.help_hits.area) {
                    self.help = false;
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let (Some(grab), Some(metrics)) = (self.help_scroll_grab, self.help_hits.metrics)
                {
                    let offset = crate::chrome::scroll::scrollbar_offset_from_drag_row(
                        metrics,
                        self.help_hits.scrollbar,
                        mouse.row,
                        grab,
                    );
                    self.help_state.scroll = metrics.max_offset_from_bottom.saturating_sub(offset);
                }
            }
            MouseEventKind::Up(MouseButton::Left) => self.help_scroll_grab = None,
            _ => {}
        }
    }
}
