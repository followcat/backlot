//! The `Ctrl+B ?` launcher's keys (S5 §1, A1.1): arrows move the
//! selection through the filtered rows, Enter runs a runnable row or
//! focuses a surface row, `…` rows open the compact target picker and
//! herdr's reference rows just close the overlay.

use super::App;
use crate::help_launcher::{HelpPicker, Launch, Surface};

impl App {
    /// Move the selection by `delta` rows, clamped, keeping the row
    /// inside the scroll window.
    pub(crate) fn help_launcher_move(&mut self, delta: isize) {
        let count = self.help_rows().len();
        if count == 0 {
            self.help_state.selected = 0;
            return;
        }
        let next = self.help_state.selected as isize + delta;
        self.help_state.selected = (next.clamp(0, count as isize - 1)) as usize;
        self.help_ensure_selected_visible();
    }

    /// Scroll the list so the selected row is inside the viewport —
    /// its draw-line position mapped through `draw_index`.
    fn help_ensure_selected_visible(&mut self) {
        let groups = self.help_filtered_groups();
        let index = crate::chrome::help_launcher::draw_index(&groups);
        let Some(pos) = index
            .iter()
            .position(|d| *d == Some(self.help_state.selected))
        else {
            return;
        };
        let viewport = self
            .help_hits
            .metrics
            .map(|m| m.viewport_rows)
            .unwrap_or(10)
            .max(1);
        if pos < self.help_state.scroll {
            self.help_state.scroll = pos;
        } else if pos >= self.help_state.scroll + viewport {
            self.help_state.scroll = pos + 1 - viewport;
        }
    }

    /// Enter on the selected row: a runnable act leaves through
    /// `run_studio_act`, a surface row focuses its surface, a `…`
    /// row opens its picker, herdr's rows just close the overlay.
    pub(crate) fn help_launcher_activate(&mut self) {
        let rows = self.help_rows();
        let Some(row) = rows.get(self.help_state.selected).cloned() else {
            self.help = false;
            return;
        };
        match row.launch {
            Launch::Act { menu, act } => {
                self.help = false;
                self.run_studio_act(&menu, act);
            }
            Launch::Surface(surface) => {
                self.help = false;
                self.open_help_surface(surface);
            }
            Launch::Pick(pick) => {
                let rows = self.picker_rows(pick);
                if rows.is_empty() {
                    return;
                }
                self.help_state.picker = Some(HelpPicker {
                    title: row.label.trim_end_matches('…').to_owned(),
                    rows,
                    selected: 0,
                });
            }
            Launch::Nav(nav) => {
                self.help = false;
                self.studio_nav(nav);
            }
            Launch::Menu(menu) => {
                let point = self.help_row_point(self.help_state.selected);
                self.help = false;
                self.open_studio_menu(*menu, point);
            }
            Launch::Close => self.help = false,
        }
    }

    /// Where a launcher row's menu opens — its drawn row's corner when
    /// the last frame had it, else the launcher's top edge. The menu
    /// clamps itself into the frame either way.
    fn help_row_point(&self, index: usize) -> (u16, u16) {
        self.help_hits
            .rows
            .iter()
            .find(|(_, row)| *row == index)
            .map(|(rect, _)| (rect.x + 1, rect.y + 1))
            .unwrap_or((self.area.0 / 2, self.area.1 / 2))
    }

    /// A surface row's focus: the Call Sheet expanded (its keys), the
    /// `tell hollywood` line editing, the dock via its toggle, or the
    /// focused pane's studio menu.
    fn open_help_surface(&mut self, surface: Surface) {
        match surface {
            Surface::CallSheet => self.hollywood_open(),
            Surface::CallSheetTell => {
                self.hollywood_open();
                self.hollywood_select(crate::hollywood::Target::Input);
                self.hollywood.editing = true;
            }
            Surface::Dock => self.panel_toggle(),
            Surface::FocusLot => self.focus_lot(),
            Surface::PaneMenu => self.pane_menu(),
        }
    }
}
