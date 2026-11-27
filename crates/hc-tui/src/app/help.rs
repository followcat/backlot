// Derived from herdr src/client/shell/overlay_input.rs and mouse.rs (Apache-2.0).
use super::{
    App,
    input::{MouseButton, MouseEventKind, MouseInput, TerminalKey},
};
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Position, Rect};

impl App {
    /// Whether `Ctrl+B ?` is the S5 action launcher rather than the
    /// plain keybind reference — a studio-mode client with at least
    /// one studio or a `[hollywood]` section (D6 keeps `--plain` and
    /// the no-studio frame on herdr's overlay).
    pub(crate) fn help_launcher(&self) -> bool {
        !self.plain && (self.has_hollywood_cfg || !self.panel_summaries().is_empty())
    }

    pub(super) fn handle_help_key(&mut self, key: &TerminalKey) {
        // The compact picker owns its keys while it floats inside
        // the overlay — Esc steps back to the list.
        if self.help_state.picker.is_some() {
            self.help_picker_key(key);
            return;
        }
        let combo = crate::input::terminal_key_of(key);
        let (code, modifiers) = crate::config::normalize_key_combo((combo.code, combo.modifiers));
        let character = crate::keybind_help::keybind_help_text_char(key);
        if self.help_state.search_focused {
            if let Some(changed) = self.help_state.query.handle_key(key) {
                if changed {
                    self.help_state.scroll = 0;
                    self.help_state.selected = 0;
                }
                return;
            }
            match code {
                KeyCode::Esc => self.help_back(),
                // A filtered Enter runs the selected launcher row (S5);
                // herdr's plain overlay keeps Enter = close.
                KeyCode::Enter if self.help_launcher() => {
                    self.help_state.search_focused = false;
                    self.help_launcher_activate();
                }
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
                    // The launcher's arrows move the selection through the
                    // filtered rows; herdr's search only scrolls the list.
                    if self.help_launcher() {
                        self.help_launcher_move(delta);
                    } else {
                        self.scroll_help(delta);
                    }
                }
                _ => {}
            }
            return;
        }
        match code {
            // S5: with a filter applied, Esc clears it first — a
            // second Esc (or herdr's overlay's one) closes.
            KeyCode::Esc if self.help_launcher() && !self.help_state.query.as_str().is_empty() => {
                self.help_state.query.clear();
                self.help_state.scroll = 0;
                self.help_state.selected = 0;
            }
            KeyCode::Esc => self.help = false,
            KeyCode::Enter if self.help_launcher() => self.help_launcher_activate(),
            KeyCode::Enter => self.help = false,
            KeyCode::Home if self.help_launcher() => {
                self.help_state.selected = 0;
                self.help_state.scroll = 0;
            }
            KeyCode::End if self.help_launcher() => {
                self.help_state.selected = usize::MAX;
                self.help_launcher_move(0);
            }
            KeyCode::Home => self.help_state.scroll = 0,
            KeyCode::End => self.help_state.scroll = self.help_hits.max_scroll,
            KeyCode::Up | KeyCode::Char('k') if self.help_launcher() => {
                self.help_launcher_move(-1);
            }
            KeyCode::Down | KeyCode::Char('j') if self.help_launcher() => {
                self.help_launcher_move(1);
            }
            KeyCode::PageUp | KeyCode::PageDown if self.help_launcher() => {
                let page = self
                    .help_hits
                    .metrics
                    .map(|m| m.viewport_rows.max(1))
                    .unwrap_or(8) as isize;
                self.help_launcher_move(if code == KeyCode::PageUp { -page } else { page });
            }
            KeyCode::Up | KeyCode::Char('k') => self.scroll_help(-1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_help(1),
            KeyCode::PageUp => self.scroll_help(-8),
            KeyCode::PageDown => self.scroll_help(8),
            _ if character == Some('/') => {
                self.help_state.search_focused = true;
                self.help_state.scroll = 0;
                self.help_state.selected = 0;
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
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let delta = if matches!(mouse.kind, MouseEventKind::ScrollUp) {
                    -1isize
                } else {
                    1isize
                };
                if let Some(picker) = self.help_state.picker.as_mut() {
                    let vis = self.help_hits.picker_rows.len().max(1);
                    let max_scroll = picker.rows.len().saturating_sub(vis);
                    picker.scroll = picker.scroll.saturating_add_signed(delta).min(max_scroll);
                    if picker.selected < picker.scroll {
                        picker.selected = picker.scroll;
                    } else if picker.selected >= picker.scroll + vis {
                        picker.selected = picker.scroll + vis - 1;
                    }
                } else {
                    self.scroll_help(delta * 3);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if self.help_state.picker.is_some() {
                    // The picker is modal inside the overlay: a row
                    // runs its target, an outside click steps back or closes.
                    let row = self
                        .help_hits
                        .picker_rows
                        .iter()
                        .find(|(rect, _)| contains(*rect))
                        .map(|(_, pos)| *pos);
                    if let Some(pos) = row {
                        if let Some(picker) = self.help_state.picker.as_mut() {
                            picker.selected = pos;
                        }
                        self.help_picker_run();
                    } else if self
                        .help_state
                        .picker
                        .as_ref()
                        .is_some_and(|p| p.standalone)
                    {
                        if !contains(self.help_hits.picker) {
                            self.help_state.picker = None;
                            self.help = false;
                        }
                    } else {
                        self.help_state.picker = None;
                    }
                } else if let Some((_, row)) = self
                    .help_hits
                    .rows
                    .iter()
                    .find(|(rect, _)| contains(*rect))
                    .map(|(r, pos)| (*r, *pos))
                {
                    // A launcher row: select it and run like Enter.
                    self.help_state.selected = row;
                    self.help_launcher_activate();
                } else if contains(self.help_hits.scrollbar) {
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
