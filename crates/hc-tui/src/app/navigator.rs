// Derived from herdr src/client/shell/overlay_input.rs, actions.rs,
// endpoint_navigation.rs and mouse.rs (Apache-2.0).
//! The "Go to" overlay's input and activation: keys, mouse and the
//! selected target's focus. Rows render through
//! `chrome::navigator_render`; the model lives in `chrome::navigator`.

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::{Position, Rect};

use crate::chrome::navigator::{
    NavigatorFilter, NavigatorRow, NavigatorState, NavigatorTarget, navigator_rows,
    navigator_selected_index, selected_navigator_target,
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
    /// The current rows — the reference recomputes them for every
    /// interaction from live endpoint snapshots.
    fn navigator_rows_now(&self) -> Vec<NavigatorRow> {
        let Some(navigator) = self.navigator.as_ref() else {
            return Vec::new();
        };
        navigator_rows(
            &self.projection.endpoints,
            &self.projection.active_endpoint_id,
            navigator,
        )
    }

    /// `open_navigator_overlay` (`prefix+g`): fresh state preselecting
    /// the row marked `current` — the focused pane.
    pub(super) fn open_navigator_overlay(&mut self) {
        let mut navigator = NavigatorState::default();
        navigator.selected = navigator_rows(
            &self.projection.endpoints,
            &self.projection.active_endpoint_id,
            &navigator,
        )
        .iter()
        .find(|row| row.current)
        .map(|row| row.target.clone());
        self.navigator = Some(navigator);
    }

    /// `move_navigator_selection`: arrows, j/k, ctrl-d/u and the wheel
    /// all step the selection through the flat row list.
    pub(super) fn move_navigator_selection(&mut self, delta: isize) {
        let Some(navigator) = self.navigator.as_mut() else {
            return;
        };
        let rows = navigator_rows(
            &self.projection.endpoints,
            &self.projection.active_endpoint_id,
            navigator,
        );
        if rows.is_empty() {
            navigator.selected = None;
            return;
        }
        let selected = navigator_selected_index(&rows, navigator).unwrap_or(0);
        let next =
            (selected as isize + delta).clamp(0, rows.len().saturating_sub(1) as isize) as usize;
        navigator.selected = Some(rows[next].target.clone());
    }

    /// `scroll_navigator_to`: scrollbar jumps/drags write the scroll
    /// directly, then clamp the selection into the dragged viewport so
    /// rendering does not snap back to it.
    pub(super) fn scroll_navigator_to(&mut self, scroll: usize, viewport_rows: usize) {
        let Some(navigator) = self.navigator.as_mut() else {
            return;
        };
        let rows = navigator_rows(
            &self.projection.endpoints,
            &self.projection.active_endpoint_id,
            navigator,
        );
        let viewport_rows = viewport_rows.max(1);
        navigator.scroll = scroll.min(rows.len().saturating_sub(viewport_rows));
        let selected = navigator_selected_index(&rows, navigator).unwrap_or(0);
        let selected = selected.clamp(navigator.scroll, navigator.scroll + viewport_rows - 1);
        navigator.selected = rows.get(selected).map(|row| row.target.clone());
    }

    /// `move_navigator_workspace`: ←/→ hop to the previous/next
    /// workspace section's first pane row.
    fn move_navigator_workspace(&mut self, forward: bool) {
        let Some(navigator) = self.navigator.as_mut() else {
            return;
        };
        let rows = navigator_rows(
            &self.projection.endpoints,
            &self.projection.active_endpoint_id,
            navigator,
        );
        let Some(selected) = navigator_selected_index(&rows, navigator) else {
            return;
        };
        let section = rows[..=selected]
            .iter()
            .rposition(|row| !matches!(row.target, NavigatorTarget::Pane { .. }))
            .unwrap_or(selected);
        let mut destinations = rows.windows(2).enumerate().filter(|(index, pair)| {
            matches!(pair[0].target, NavigatorTarget::Workspace { .. })
                && matches!(pair[1].target, NavigatorTarget::Pane { .. })
                && if forward {
                    *index > section
                } else {
                    *index < section
                }
        });
        let destination = if forward {
            destinations.next()
        } else {
            destinations.next_back()
        };
        if let Some((_, pair)) = destination {
            navigator.selected = Some(pair[1].target.clone());
        }
    }

    /// `accept_navigator_selection`: activate the selected row —
    /// machines switch the endpoint, workspaces/panes focus their
    /// terminal. Offline remote endpoints report unavailable and keep
    /// the overlay open, like the reference.
    pub(super) fn accept_navigator_selection(&mut self) {
        let Some(navigator) = self.navigator.as_ref() else {
            return;
        };
        let target = selected_navigator_target(
            &navigator_rows(
                &self.projection.endpoints,
                &self.projection.active_endpoint_id,
                navigator,
            ),
            navigator,
        );
        let Some(target) = target else {
            return;
        };
        let endpoint_id = match &target {
            NavigatorTarget::Machine { endpoint_id }
            | NavigatorTarget::Workspace { endpoint_id, .. }
            | NavigatorTarget::Pane { endpoint_id, .. } => endpoint_id.clone(),
        };
        let online = self.endpoint_is_online(&endpoint_id);
        if !online && !endpoint_id.is_local() {
            let label = self.endpoint_label(&endpoint_id);
            self.show_notice("Endpoint unavailable", &format!("{label} is not ready"));
            return;
        }
        match target {
            NavigatorTarget::Machine { endpoint_id } => {
                // The reference pushes ActivateEndpoint when the pick is
                // not the active endpoint; the focused pane defines the
                // active endpoint here, so select its first pane.
                if endpoint_id != self.projection.active_endpoint_id
                    && let Some(pane_id) = self
                        .projection
                        .endpoints
                        .iter()
                        .find(|e| e.endpoint_id == endpoint_id)
                        .and_then(|e| e.snapshot.as_deref())
                        .and_then(|s| s.panes.first())
                        .map(|p| p.pane_id.clone())
                {
                    self.focus_pane(pane_id);
                }
            }
            NavigatorTarget::Workspace {
                endpoint_id,
                workspace_id,
            } => {
                self.focus_workspace(&endpoint_id, &workspace_id);
            }
            NavigatorTarget::Pane { pane_id, .. } => {
                self.focus_pane(pane_id);
            }
        }
        self.navigator = None;
    }

    /// The navigator's key dispatch — `overlay_input.rs`'s Navigator
    /// branch, in the reference's check order.
    pub(super) fn handle_navigator_key(&mut self, key: &TerminalKey) {
        let combo = crate::input::terminal_key_of(key);
        let (code, modifiers) = crate::config::normalize_key_combo((combo.code, combo.modifiers));
        let search_focused = self
            .navigator
            .as_ref()
            .is_some_and(|navigator| navigator.search_focused);
        if code == KeyCode::Esc {
            if let Some(navigator) = self.navigator.as_mut() {
                if navigator.search_focused {
                    navigator.search_focused = false;
                } else {
                    self.navigator = None;
                }
            }
            return;
        }
        if code == KeyCode::Enter {
            self.accept_navigator_selection();
            return;
        }
        if search_focused {
            if let Some(navigator) = self.navigator.as_mut()
                && let Some(content_changed) = navigator.query.handle_key(key)
            {
                if content_changed {
                    navigator.filter = None;
                    navigator.selected = None;
                }
                return;
            }
            if code == KeyCode::Up
                || code == KeyCode::Char('p') && modifiers == KeyModifiers::CONTROL
            {
                self.move_navigator_selection(-1);
                return;
            }
            if code == KeyCode::Down
                || code == KeyCode::Char('n') && modifiers == KeyModifiers::CONTROL
            {
                self.move_navigator_selection(1);
                return;
            }
            return;
        }
        if matches!(code, KeyCode::Left | KeyCode::Right) && modifiers.is_empty() {
            self.move_navigator_workspace(code == KeyCode::Right);
            return;
        }
        if code == KeyCode::Backspace && modifiers.is_empty() {
            if let Some(navigator) = self.navigator.as_mut()
                && navigator.filter.take().is_some()
            {
                navigator.selected = None;
            }
            return;
        }
        if code == KeyCode::Home && modifiers.is_empty() {
            if let Some(navigator) = self.navigator.as_mut() {
                navigator.selected = None;
                navigator.scroll = 0;
            }
            return;
        }
        if matches!(code, KeyCode::End | KeyCode::Char('G')) && modifiers.is_empty() {
            let last = self
                .navigator_rows_now()
                .last()
                .map(|row| row.target.clone());
            if let Some(navigator) = self.navigator.as_mut() {
                navigator.selected = last;
            }
            return;
        }
        // Match the typed character so layouts where `/` needs Shift work
        // when the host reports it as a shifted key (report-all panes).
        // A shifted key needs the host's text or shifted alternate: bare
        // `CSI 47;2u` is Shift+/, which is not `/` on most layouts.
        let shifted_without_evidence = modifiers.contains(KeyModifiers::SHIFT)
            && key.text_commit.is_none()
            && key.shifted_codepoint.is_none();
        if !shifted_without_evidence
            && crate::keybind_help::keybind_help_text_char(key) == Some('/')
        {
            if let Some(navigator) = self.navigator.as_mut() {
                navigator.search_focused = true;
                navigator.filter = None;
            }
            return;
        }
        if matches!(code, KeyCode::Down | KeyCode::Char('j')) && modifiers.is_empty() {
            self.move_navigator_selection(1);
            return;
        }
        if matches!(code, KeyCode::Up | KeyCode::Char('k')) && modifiers.is_empty() {
            self.move_navigator_selection(-1);
            return;
        }
        if code == KeyCode::Char('d') && modifiers.contains(KeyModifiers::CONTROL) {
            self.move_navigator_selection(8);
            return;
        }
        if code == KeyCode::Char('u') && modifiers.contains(KeyModifiers::CONTROL) {
            self.move_navigator_selection(-8);
            return;
        }
        if let Some(filter) = match code {
            KeyCode::Char('b') if modifiers.is_empty() => Some(NavigatorFilter::Blocked),
            KeyCode::Char('w') if modifiers.is_empty() => Some(NavigatorFilter::Working),
            KeyCode::Char('i') if modifiers.is_empty() => Some(NavigatorFilter::Idle),
            KeyCode::Char('d') if modifiers.is_empty() => Some(NavigatorFilter::Done),
            _ => None,
        } {
            if let Some(navigator) = self.navigator.as_mut() {
                navigator.query.clear();
                navigator.filter = Some(filter);
                navigator.selected = None;
            }
            return;
        }
        if code == KeyCode::Char('a')
            && modifiers.is_empty()
            && let Some(navigator) = self.navigator.as_mut()
        {
            navigator.query.clear();
            navigator.filter = None;
            navigator.selected = None;
        }
    }

    /// The navigator's mouse dispatch — `mouse.rs`'s Navigator branch:
    /// hover selects, the scrollbar jumps/drags, the search line grabs
    /// focus, a row click accepts, outside dismisses and the wheel
    /// steps the selection by three.
    pub(super) fn handle_navigator_mouse(&mut self, mouse: MouseInput) {
        let point = (mouse.col, mouse.row);
        let row_hit = self
            .navigator_hits
            .rows
            .iter()
            .find(|(rect, _)| contains(*rect, point))
            .cloned();
        match mouse.kind {
            MouseEventKind::Moved => {
                if let Some((_, target)) = row_hit
                    && let Some(navigator) = self.navigator.as_mut()
                {
                    navigator.selected = Some(target);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if contains(self.navigator_hits.scrollbar, point) {
                    if let Some(metrics) = self.navigator_hits.metrics {
                        match crate::chrome::scroll::scrollbar_thumb_grab_offset(
                            metrics,
                            self.navigator_hits.scrollbar,
                            mouse.row,
                        ) {
                            Some(grab_row_offset) => {
                                self.chrome_drag =
                                    Some(super::mouse::ChromeDrag::NavigatorScrollbar {
                                        grab_row_offset,
                                    });
                            }
                            None => {
                                let offset = crate::chrome::scroll::scrollbar_offset_from_row(
                                    metrics,
                                    self.navigator_hits.scrollbar,
                                    mouse.row,
                                );
                                self.scroll_navigator_to(
                                    metrics.max_offset_from_bottom.saturating_sub(offset),
                                    metrics.viewport_rows,
                                );
                            }
                        }
                    }
                } else if contains(self.navigator_hits.search, point) {
                    if let Some(navigator) = self.navigator.as_mut() {
                        navigator.search_focused = true;
                        navigator.filter = None;
                    }
                } else if let Some((_, target)) = row_hit {
                    if let Some(navigator) = self.navigator.as_mut() {
                        navigator.selected = Some(target);
                    }
                    self.accept_navigator_selection();
                } else if !contains(self.navigator_hits.area, point) {
                    self.navigator = None;
                }
            }
            MouseEventKind::ScrollUp => self.move_navigator_selection(-3),
            MouseEventKind::ScrollDown => self.move_navigator_selection(3),
            _ => {}
        }
    }
}
