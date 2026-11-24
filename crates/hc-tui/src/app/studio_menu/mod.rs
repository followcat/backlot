//! The studio context menus' app side (S3 §1, A3.2/A10.2): `m` on
//! a dock row, `Ctrl+B m` on a focused pane, right-click on the
//! dock, the cast and the Call Sheet all land here — the menu's
//! target model comes from the same summary row the pointer named,
//! so a menu always matches its row. Activation either spawns the
//! action through `run_action` or opens the native dialog the item
//! names.

use crate::chrome::menu::{ContextMenu, ContextMenuTarget};
use crate::studio::StudioKey;
use crate::studio_menu::StudioMenu;

use super::App;

mod activate;
mod delete;
mod prune;

impl App {
    /// Right-click on the sidebar's studio surfaces — the studio
    /// header opens the studio menu; a cast row resolves its terminal
    /// like `Ctrl+B m` does. Returns `true` when a menu opened so the
    /// chrome dispatch below skips the row.
    pub(crate) fn open_studio_menu_at(&mut self, point: (u16, u16)) -> bool {
        if self.plain {
            return false;
        }
        let pos = ratatui::layout::Position::new(point.0, point.1);
        // S5 §2.3: the lot pill's right-click is the lot menu — the
        // pill is the whole lot, not the current studio (Decision 5
        // only moved the pill's *left*-click and its display).
        if self.hit_map.lot_pill.contains(pos) {
            self.open_studio_menu(StudioMenu::lot(), point);
            return self.menu.is_some();
        }
        if self.hit_map.studio_header.contains(pos)
            && let Some(key) = self.hit_map.studio_key.clone()
        {
            self.studio_header_menu(&key, point);
            return true;
        }
        let terminal = self
            .hit_map
            .agents
            .iter()
            .find(|(rect, _)| rect.contains(pos))
            .map(|(_, terminal)| terminal.clone());
        if let Some(terminal) = terminal {
            return self.cast_menu(&terminal, point);
        }
        let ws = self
            .hit_map
            .workspaces
            .iter()
            .find(|h| h.rect.contains(pos))
            .map(|h| (h.endpoint_id.clone(), h.workspace_id.clone()));
        if let Some((endpoint_id, workspace_id)) = ws
            && let Some(key) = self.studio_for_workspace(&endpoint_id, &workspace_id)
        {
            self.studio.set_studio_focus(key.clone());
            self.studio_header_menu(&key, point);
            return true;
        }
        false
    }

    /// Seat `menu` at `point` — the context-menu shell renders and
    /// routes it like every other menu.
    pub(crate) fn open_studio_menu(&mut self, menu: StudioMenu, point: (u16, u16)) {
        if menu.items().is_empty() {
            return;
        }
        self.menu = Some(ContextMenu {
            target: ContextMenuTarget::Studio(Box::new(menu)),
            x: point.0,
            y: point.1,
            highlighted: 0,
        });
    }

    /// The dock row's menu — `m` on the selection or right-click on
    /// a row. `None` targets (dividers, drawer lines, the title)
    /// open nothing.
    pub(crate) fn panel_menu(&mut self, target: &crate::panel::Target, point: (u16, u16)) {
        let machines = self.panel_summaries();
        let Some(menu) = self.studio_menu_for(target, &machines) else {
            return;
        };
        self.open_studio_menu(menu.inside_dock(), point);
    }

    /// `m` while the dock owns the keys — the selected row's menu,
    /// anchored under the panel's title corner.
    pub(crate) fn panel_menu_selected(&mut self) {
        let targets = self.panel_targets();
        let index = self.panel_selected_index(&targets);
        let Some(target) = targets.get(index).cloned() else {
            return;
        };
        // The menu anchors where the selection is — its row's rect
        // when the last frame drew it, else the panel's top edge.
        let point = self
            .panel_hits
            .rows
            .iter()
            .find(|(_, t)| *t == target)
            .map(|(rect, _)| (rect.x + 1, rect.y + 1))
            .unwrap_or((self.panel_hits.area.x + 1, self.panel_hits.area.y + 2));
        self.panel_menu(&target, point);
    }

    /// `Ctrl+B m` on the focused pane (§1): the pane's studio role
    /// menu when the terminal is a cast member, else the current
    /// studio's menu — a shell pane's menu of "the current studio".
    pub(crate) fn pane_menu(&mut self) {
        if self.plain {
            return;
        }
        let machines = self.panel_summaries();
        let point = self.pane_menu_point();
        match self.studio.focus.clone() {
            Some(crate::studio::StudioFocus::Studio(key)) => {
                if let Some(studio) = crate::panel::data::studio(&machines, &key) {
                    let menu = StudioMenu::studio(
                        &key,
                        studio.studio,
                        !self.machine_stage_running(studio.machine),
                        !self.machine_reachable(studio.machine),
                    );
                    self.open_studio_menu(menu, point);
                }
            }
            Some(crate::studio::StudioFocus::Role(key, terminal)) => {
                if let Some(studio) = crate::panel::data::studio(&machines, &key) {
                    if let Some(role) = studio
                        .studio
                        .roles
                        .iter()
                        .find(|r| r.terminal.as_deref() == Some(terminal.as_str()))
                    {
                        let offline = !self.machine_reachable(studio.machine);
                        self.open_studio_menu(StudioMenu::role(&key, role, offline), point);
                    } else {
                        let menu = StudioMenu::studio(
                            &key,
                            studio.studio,
                            !self.machine_stage_running(studio.machine),
                            !self.machine_reachable(studio.machine),
                        );
                        self.open_studio_menu(menu, point);
                    }
                }
            }
            None => {
                if let Some(terminal) = self.focused.clone() {
                    for studio in crate::panel::data::studios(&machines) {
                        if let Some(role) = studio
                            .studio
                            .roles
                            .iter()
                            .find(|r| r.terminal.as_deref() == Some(terminal.as_str()))
                        {
                            let key = studio.key();
                            let offline = !self.machine_reachable(studio.machine);
                            self.open_studio_menu(StudioMenu::role(&key, role, offline), point);
                            return;
                        }
                    }
                }
                if let Some(key) = self.studio.current.clone()
                    && let Some(studio) = crate::panel::data::studio(&machines, &key)
                {
                    let menu = StudioMenu::studio(
                        &key,
                        studio.studio,
                        !self.machine_stage_running(studio.machine),
                        !self.machine_reachable(studio.machine),
                    );
                    self.open_studio_menu(menu, point);
                }
            }
        }
    }

    /// The pane menu's anchor — the focused pane's own corner, or
    /// the frame's centre when it is not hit-mapped.
    fn pane_menu_point(&self) -> (u16, u16) {
        if let Some(focused) = self.focused.as_deref()
            && let Some(hit) = self.hit_map.panes.iter().find(|h| h.pane_id == focused)
        {
            return (hit.rect.x + 2, hit.rect.y + 1);
        }
        (self.area.0 / 2, self.area.1 / 2)
    }

    /// The `Target`'s menu — the row's own data names everything.
    fn studio_menu_for(
        &self,
        target: &crate::panel::Target,
        machines: &[crate::panel::data::MachineSummary],
    ) -> Option<StudioMenu> {
        use crate::panel::Target;
        match target {
            Target::Studio(key) | Target::Block(key) | Target::Strip(key) => {
                let studio = crate::panel::data::studio(machines, key)?;
                Some(StudioMenu::studio(
                    key,
                    studio.studio,
                    !self.machine_stage_running(studio.machine),
                    !self.machine_reachable(studio.machine),
                ))
            }
            Target::Fold(key) | Target::Input(key) | Target::ReopenProducer(key) => {
                let studio = crate::panel::data::studio(machines, key)?;
                Some(StudioMenu::studio(
                    key,
                    studio.studio,
                    !self.machine_stage_running(studio.machine),
                    !self.machine_reachable(studio.machine),
                ))
            }
            Target::Talk(key, seq) => {
                let studio = crate::panel::data::studio(machines, key)?;
                let file = studio
                    .studio
                    .exchange
                    .iter()
                    .find(|e| e.seq == *seq)
                    .and_then(|e| e.file.clone());
                Some(StudioMenu::studio_talk(
                    key,
                    *seq,
                    file,
                    !self.machine_reachable(studio.machine),
                ))
            }
            Target::Person(key, role_name) => {
                let studio = crate::panel::data::studio(machines, key)?;
                let role = studio.studio.roles.iter().find(|r| r.role == *role_name)?;
                Some(StudioMenu::role(
                    key,
                    role,
                    !self.machine_reachable(studio.machine),
                ))
            }
            Target::Task(key, id) => {
                let studio = crate::panel::data::studio(machines, key)?;
                let task = studio.studio.tasks.iter().find(|t| t.id == *id)?;
                Some(StudioMenu::task(
                    key,
                    task,
                    !self.machine_reachable(studio.machine),
                ))
            }
            Target::Stage(machine) => {
                let mut menu = StudioMenu::stage(!self.machine_reachable(machine.as_deref()));
                menu.machine = machine.clone();
                Some(menu)
            }
            // The setup hint is a button, not a menu row.
            Target::Setup | Target::None => None,
        }
    }

    /// Right-click on a Sidebar Cast row: the `agents` hit map lists
    /// the role's terminal in studio mode — its row menu is the role
    /// menu, never Herdr's agent menu.
    pub(crate) fn cast_menu(&mut self, pane_id: &str, point: (u16, u16)) -> bool {
        if !self.studio.active {
            return false;
        }
        let machines = self.panel_summaries();
        for studio in crate::panel::data::studios(&machines) {
            if let Some(role) = studio
                .studio
                .roles
                .iter()
                .find(|r| r.terminal.as_deref() == Some(pane_id))
            {
                let key = studio.key();
                self.studio.current = Some(key.clone());
                self.studio.focus = Some(crate::studio::StudioFocus::Role(
                    key.clone(),
                    pane_id.to_owned(),
                ));
                let offline = !self.machine_reachable(studio.machine);
                self.open_studio_menu(StudioMenu::role(&key, role, offline), point);
                return true;
            }
        }
        if let Some(key) = self.shell_studio_key(pane_id, &machines)
            && let Some(studio) = crate::panel::data::studio(&machines, &key)
        {
            self.studio.current = Some(key.clone());
            self.studio.focus = Some(crate::studio::StudioFocus::Role(
                key.clone(),
                pane_id.to_owned(),
            ));
            let menu = StudioMenu::studio(
                &key,
                studio.studio,
                !self.machine_stage_running(studio.machine),
                !self.machine_reachable(studio.machine),
            );
            self.open_studio_menu(menu, point);
            return true;
        }
        false
    }

    /// The workspace header's studio name — right-click opens the
    /// studio menu (A10.2).
    pub(crate) fn studio_header_menu(&mut self, key: &StudioKey, point: (u16, u16)) {
        let machines = self.panel_summaries();
        let Some(studio) = crate::panel::data::studio(&machines, key) else {
            return;
        };
        self.studio.set_studio_focus(key.clone());
        let menu = StudioMenu::studio(
            key,
            studio.studio,
            !self.machine_stage_running(studio.machine),
            !self.machine_reachable(studio.machine),
        );
        self.open_studio_menu(menu, point);
    }

    /// A Call Sheet row's menu — frames, wait rows and the title's
    /// own. `target` is the row's `hollywood::Target`.
    pub(crate) fn hollywood_menu(&mut self, target: &crate::hollywood::Target, point: (u16, u16)) {
        use crate::hollywood::Target as T;
        let machines = self.panel_summaries();
        let menu = match target {
            T::Studio(key) | T::Ready(key) => crate::panel::data::studio(&machines, key).map(|s| {
                StudioMenu::studio(
                    key,
                    s.studio,
                    !self.machine_stage_running(s.machine),
                    !self.machine_reachable(s.machine),
                )
            }),
            T::Prompt(key, role_name) => crate::panel::data::studio(&machines, key).and_then(|s| {
                s.studio
                    .roles
                    .iter()
                    .find(|r| r.role == *role_name)
                    .map(|r| StudioMenu::role(key, r, !self.machine_reachable(s.machine)))
            }),
            T::Stage(machine, offline) => {
                let mut m = StudioMenu::stage(*offline);
                m.machine = machine.clone();
                Some(m)
            }
            T::Card(id) => Some(StudioMenu::card(id)),
            T::Title => Some(StudioMenu::hollywood_title(self.hollywood_cfg.as_ref())),
            T::Headline => Some(StudioMenu::hollywood(self.hollywood_headline_seq())),
            T::Talk(seq) => Some(StudioMenu::hollywood(Some(*seq))),
            _ => None,
        };
        if let Some(menu) = menu {
            self.open_studio_menu(menu, point);
        }
    }

    /// `m` while the expanded Call Sheet owns the keys — the
    /// selected row's menu at its drawn position.
    pub(crate) fn hollywood_menu_selected(&mut self) {
        let targets = self.hollywood_targets();
        let index = self.hollywood_selected_index(&targets);
        let Some(target) = targets.get(index).cloned() else {
            return;
        };
        let point = self
            .hollywood_hits
            .rows
            .iter()
            .find(|(_, t)| *t == target)
            .map(|(rect, _)| (rect.x + 1, rect.y + 1))
            .unwrap_or((
                self.hollywood_hits.area.x + 1,
                self.hollywood_hits.area.y + 1,
            ));
        self.hollywood_menu(&target, point);
    }
}
