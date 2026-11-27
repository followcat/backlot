//! W1b's keyboard path into the left machines tree. `Ctrl+B t` moves
//! the focus into the tree (opening the sidebar first when it is
//! collapsed), `Esc` returns it to the terminal, `↑`/`↓` (and `k`/`j`)
//! move the highlighted row, `Enter` acts like a left-click on that row
//! and `m` opens its context menu. The cursor is a `SidebarRow` of the
//! flat tree list, so the highlight and the action always name the row
//! the person sees; the section scrolls to it through #114's reveal.
//!
//! The key is `prefix+t`, free in the default keymap (`prefix+tab` and
//! `prefix+h` — the brief's examples — already cycle panes and move the
//! pane focus left). It is a studio-mode chord like `prefix+m`, so
//! `--plain` never enters the tree.

use std::time::Instant;

use crossterm::event::KeyCode;

use crate::app::input::TerminalKey;
use crate::chrome::sidebar_nav;
use crate::chrome::snapshot::ClientEndpointId;
use crate::chrome::studio_tree::{Row as StudioRow, SidebarRow};
use crate::studio::StudioKey;

use super::App;

impl App {
    /// Whether this press is the tree's focus chord tail (`t` while the
    /// prefix is armed). Studio mode only.
    pub(crate) fn tree_focus_press(&self, key: &TerminalKey) -> bool {
        if !self.studio.active {
            return false;
        }
        let combo = crate::input::terminal_key_of(key);
        let (code, modifiers) = crate::config::normalize_key_combo((combo.code, combo.modifiers));
        code == KeyCode::Char('t') && modifiers.is_empty()
    }

    /// Whether the left tree owns the keys right now.
    pub(crate) fn tree_focused(&self) -> bool {
        self.tree_focus.is_some()
    }

    /// `Ctrl+B t`: open the sidebar if it is collapsed, select the row
    /// that stands for the current place (the focused workspace's space
    /// on the active machine, else the first row), and hand it the keys.
    pub(crate) fn tree_focus_enter(&mut self) {
        if !self.studio.active {
            return;
        }
        self.shell_state.sidebar_collapsed = false;
        // Entering the tree hands it the keys from any other owner: a
        // focused dock, the expanded Call Sheet, Navigate mode, the
        // opening page — the same releases the studio keys run.
        self.nav_leave_overlays();
        if self.mode == crate::app::Mode::Navigate {
            self.mode = crate::app::Mode::Normal;
            self.navigate_target = None;
        }
        self.peek = None;
        let rows = self.tree_rows();
        if rows.is_empty() {
            return;
        }
        let focused_workspace = self.projection.snapshot.focused_workspace_id.clone();
        let active = self.projection.active_endpoint_id.clone();
        let selected = focused_workspace
            .and_then(|id| {
                rows.iter()
                    .find(|row| {
                        matches!(row, SidebarRow::Workspace { endpoint, workspace_id }
                            if *workspace_id == id && *endpoint == active)
                    })
                    .cloned()
            })
            .or_else(|| rows.first().cloned());
        self.tree_focus = selected;
        self.shell_state.reveal_tree_row = true;
        self.sched.mark_dirty(Instant::now());
    }

    /// `Esc` (or a press that leaves the tree): give the keys back.
    pub(crate) fn tree_focus_leave(&mut self) {
        if self.tree_focus.take().is_some() {
            self.sched.mark_dirty(Instant::now());
        }
    }

    /// The flat left-tree rows right now — the cursor's own list.
    pub(crate) fn tree_rows(&self) -> Vec<SidebarRow> {
        if self.plain {
            return Vec::new();
        }
        let tree = self.studio_sidebar_tree();
        sidebar_nav::rows(
            &self.projection.endpoints,
            &self.shell_state.collapsed_endpoints,
            &self.shell_state.collapsed_groups,
            &self.shell_state.remote_collapsed_groups,
            true,
            Some(&tree),
        )
    }

    /// One press while the tree owns the keys — every key is consumed
    /// here, like the dock and the Call Sheet. Repeats move only.
    pub(crate) fn handle_tree_key(&mut self, key: &TerminalKey) {
        let combo = crate::input::terminal_key_of(key);
        let (code, modifiers) = crate::config::normalize_key_combo((combo.code, combo.modifiers));
        let plain = modifiers.is_empty();
        let press = key.input.action == hc_vt::Action::Press;
        match code {
            KeyCode::Esc if press => self.tree_focus_leave(),
            KeyCode::Up | KeyCode::Char('k') if plain => self.tree_move(-1),
            KeyCode::Down | KeyCode::Char('j') if plain => self.tree_move(1),
            KeyCode::Enter if press => self.tree_activate(),
            KeyCode::Char('m') if plain && press => self.tree_menu(),
            _ => {}
        }
    }

    /// Move the cursor onto `key`'s row — a studio step while the tree
    /// owns the keys keeps them here, on the studio it landed on: the
    /// studio's workspace row when it has a space, else its extra
    /// `stage down`/ledger row.
    pub(crate) fn tree_focus_studio_row(&mut self, key: &StudioKey) {
        let rows = self.tree_rows();
        let found = rows.into_iter().find(|row| match row {
            SidebarRow::Workspace {
                endpoint,
                workspace_id,
            } => self
                .studio_for_workspace(endpoint, workspace_id)
                .is_some_and(|ws| &ws == key),
            SidebarRow::Studio { endpoint, row } => match row {
                StudioRow::Studio { id, .. } | StudioRow::Finished { id } => {
                    StudioKey {
                        machine: machine_of(endpoint),
                        id: id.clone(),
                    } == *key
                }
                StudioRow::FinishedFold { .. }
                | StudioRow::Archived { .. }
                | StudioRow::Unreachable => false,
            },
            SidebarRow::Machine(_) => false,
        });
        if let Some(row) = found {
            self.tree_focus = Some(row);
            self.shell_state.reveal_tree_row = true;
            self.sched.mark_dirty(Instant::now());
        }
    }

    /// Move the cursor `delta` rows, and scroll the section to it.
    pub(crate) fn tree_move(&mut self, delta: isize) {
        let rows = self.tree_rows();
        if rows.is_empty() {
            return;
        }
        let current = self
            .tree_focus
            .as_ref()
            .and_then(|selected| rows.iter().position(|row| row == selected))
            .unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, rows.len() as isize - 1) as usize;
        self.tree_focus = Some(rows[next].clone());
        self.shell_state.reveal_tree_row = true;
        self.sched.mark_dirty(Instant::now());
    }

    /// Enter on the highlighted row: a studio's space focuses it, a
    /// stage-down studio starts the stage, the finished fold toggles, a
    /// finished studio opens its menu, a machine row folds its section —
    /// each the same action a left-click runs.
    pub(crate) fn tree_activate(&mut self) {
        let Some(row) = self.tree_focus.clone() else {
            return;
        };
        match row {
            SidebarRow::Machine(endpoint) => self.tree_toggle_endpoint(&endpoint),
            SidebarRow::Workspace {
                endpoint,
                workspace_id,
            } => {
                self.tree_focus_leave();
                self.focus_workspace(&endpoint, &workspace_id);
            }
            SidebarRow::Studio { endpoint, row } => self.tree_activate_studio(&endpoint, row),
        }
    }

    /// `m` on the highlighted row: the row's own context menu, the same
    /// list a right-click opens.
    pub(crate) fn tree_menu(&mut self) {
        let Some(row) = self.tree_focus.clone() else {
            return;
        };
        let point = self.tree_row_point();
        match row {
            SidebarRow::Machine(endpoint) => {
                let menu = self.machine_menu(&endpoint);
                let anchor = self
                    .hit_map
                    .machines
                    .iter()
                    .find(|hit| hit.endpoint_id == endpoint)
                    .map(|hit| (hit.rect.x + 1, hit.rect.y + 1))
                    .unwrap_or(point);
                self.open_studio_menu(menu, anchor);
            }
            SidebarRow::Workspace {
                endpoint,
                workspace_id,
            } => {
                if let Some(key) = self.studio_for_workspace(&endpoint, &workspace_id) {
                    self.studio.set_studio_focus(key.clone());
                    self.studio_header_menu(&key, point);
                }
            }
            SidebarRow::Studio { endpoint, row } => match row {
                StudioRow::Finished { id } => {
                    let key = StudioKey {
                        machine: machine_of(&endpoint),
                        id,
                    };
                    let menu = self.finished_studio_menu(&key);
                    self.open_studio_menu(menu, point);
                }
                StudioRow::Studio { id, .. } => {
                    let key = StudioKey {
                        machine: machine_of(&endpoint),
                        id,
                    };
                    self.studio.set_studio_focus(key.clone());
                    self.studio_header_menu(&key, point);
                }
                StudioRow::FinishedFold { .. }
                | StudioRow::Archived { .. }
                | StudioRow::Unreachable => {
                    let menu = self.machine_menu(&endpoint);
                    self.open_studio_menu(menu, point);
                }
            },
        }
    }

    fn tree_activate_studio(&mut self, endpoint: &ClientEndpointId, row: StudioRow) {
        let machine = machine_of(endpoint);
        match row {
            StudioRow::Studio { id, .. } => {
                // A stage-down ledger studio starts the machine's stage;
                // a live studio with no space still becomes current.
                if !self.machine_stage_running(machine.as_deref()) {
                    self.tree_focus_leave();
                    if self.can_start_stage(machine.as_deref()) {
                        self.run_action(crate::studio_cmds::stage_start(machine));
                    }
                } else {
                    let key = StudioKey { machine, id };
                    self.tree_focus_leave();
                    self.focus_studio(&key);
                }
            }
            StudioRow::FinishedFold { .. } => {
                self.toggle_finished_group(machine);
            }
            StudioRow::Archived { .. } => {
                self.tree_focus_leave();
                self.open_archived(machine);
            }
            // A machine that cannot be reached has no stage to start —
            // Enter folds its one line, like the machine row itself.
            StudioRow::Unreachable => self.tree_toggle_endpoint(endpoint),
            StudioRow::Finished { id } => {
                let key = StudioKey { machine, id };
                let menu = self.finished_studio_menu(&key);
                let point = self.tree_row_point();
                self.open_studio_menu(menu, point);
            }
        }
    }

    /// Enter on a machine row folds or unfolds its section — the same
    /// toggle a click on the row's marker runs.
    fn tree_toggle_endpoint(&mut self, endpoint: &ClientEndpointId) {
        if !self.shell_state.collapsed_endpoints.remove(endpoint) {
            self.shell_state
                .collapsed_endpoints
                .insert(endpoint.clone());
        }
        self.sched.mark_dirty(Instant::now());
    }

    /// Where a tree row's menu opens — the sidebar body's corner when the
    /// last frame drew it, else a fixed spot. The menu clamps itself.
    fn tree_row_point(&self) -> (u16, u16) {
        let body = self.hit_map.workspace_body;
        if body.is_empty() {
            return (2, 2);
        }
        (body.x + 2, body.y + 1)
    }
}

/// The machine an endpoint names — `None` for this core.
fn machine_of(endpoint: &ClientEndpointId) -> Option<String> {
    match endpoint {
        ClientEndpointId::Local => None,
        ClientEndpointId::Remote(name) => Some(name.clone()),
    }
}
