//! The launcher's compact target picker (S5 §1): the row list each
//! `Pick` builds — machines for `+ new studio`, studios for the
//! studio verbs, tasks for the sheet and `pause`-style verbs, closed
//! people for reopen — and its keys. Enter lands on `run_studio_act`,
//! the same arm every other surface's activation takes (S3).

use super::App;
use super::help_launcher::help_place;
use crate::help_launcher::{Pick, PickerAct, PickerRow};
use crate::panel::data;
use crate::studio_menu::{StudioAct, StudioMenu};

impl App {
    // ---------- the picker ----------

    /// The picker's row list for `pick` — each row a concrete target
    /// plus the press it stands for.
    pub(super) fn picker_rows(&self, pick: Pick) -> Vec<PickerRow> {
        let machines = self.panel_summaries();
        let local = crate::project::LOCAL_LABEL;
        let multi = data::spans_machines(&machines);
        match pick {
            Pick::Machine => machines
                .iter()
                .map(|m| {
                    let mut menu = StudioMenu::stage(false);
                    menu.machine = m.machine.clone();
                    PickerRow {
                        label: m
                            .machine
                            .clone()
                            .unwrap_or_else(|| "Local (this machine)".to_owned()),
                        menu,
                        act: PickerAct::NewStudio,
                    }
                })
                .collect(),
            Pick::Studio(act) => data::studios(&machines)
                .iter()
                .map(|s| {
                    let key = s.key();
                    PickerRow {
                        label: format!("{} · {}", s.studio.id, s.machine.unwrap_or(local)),
                        menu: StudioMenu::studio(
                            &key,
                            s.studio,
                            !self.machine_stage_running(s.machine),
                            !self.machine_reachable(s.machine),
                        ),
                        act: PickerAct::Act(act),
                    }
                })
                .collect(),
            Pick::Task => data::studios(&machines)
                .iter()
                .flat_map(|s| {
                    let key = s.key();
                    let offline = !self.machine_reachable(s.machine);
                    s.studio.tasks.iter().map(move |t| PickerRow {
                        label: format!(
                            "{} {} · {}",
                            t.id,
                            crate::studio::glyphs::Task::of(t.status).glyph(),
                            help_place(&key, local, multi)
                        ),
                        menu: StudioMenu::task(&key, t, offline),
                        act: PickerAct::Sheet,
                    })
                })
                .collect(),
            Pick::TaskVerb => data::studios(&machines)
                .iter()
                .flat_map(|s| {
                    let key = s.key();
                    let offline = !self.machine_reachable(s.machine);
                    s.studio.tasks.iter().flat_map(move |t| {
                        let menu = StudioMenu::task(&key, t, offline);
                        menu.items()
                            .into_iter()
                            .filter(|i| i.dim.is_none())
                            .filter_map(|i| match i.act {
                                StudioAct::TaskVerb(_) => Some(PickerRow {
                                    label: format!(
                                        "{} {} · {}",
                                        i.label.trim_end_matches('…'),
                                        t.id,
                                        help_place(&key, local, multi)
                                    ),
                                    menu: menu.clone(),
                                    act: PickerAct::Act(i.act),
                                }),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect(),
            Pick::PersonReopen => data::studios(&machines)
                .iter()
                .flat_map(|s| {
                    let key = s.key();
                    let offline = !self.machine_reachable(s.machine);
                    s.studio
                        .roles
                        .iter()
                        .filter(|r| r.state == "closed")
                        .flat_map(move |r| {
                            let menu = StudioMenu::role(&key, r, offline);
                            menu.items()
                                .into_iter()
                                .find(|i| i.act == StudioAct::Reopen && i.dim.is_none())
                                .map(|i| PickerRow {
                                    label: format!(
                                        "{} · {}",
                                        i.label.trim_end_matches(" (o)"),
                                        help_place(&key, local, multi)
                                    ),
                                    menu,
                                    act: PickerAct::Act(StudioAct::Reopen),
                                })
                        })
                })
                .collect(),
        }
    }

    /// Keys while the picker is open — arrows move, Enter runs the
    /// row, Esc/`?` steps back to the launcher list.
    pub(crate) fn help_picker_key(&mut self, key: &crate::app::input::TerminalKey) {
        use crossterm::event::KeyCode;
        let combo = crate::input::terminal_key_of(key);
        if key.input.action != hc_vt::Action::Press {
            return;
        }
        match combo.code {
            KeyCode::Esc | KeyCode::Char('?') => self.help_state.picker = None,
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(picker) = self.help_state.picker.as_mut()
                    && picker.selected + 1 < picker.rows.len()
                {
                    picker.selected += 1;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(picker) = self.help_state.picker.as_mut() {
                    picker.selected = picker.selected.saturating_sub(1);
                }
            }
            KeyCode::Enter => self.help_picker_run(),
            _ => {}
        }
    }

    /// Enter on a picker row — `Sheet` and `NewStudio` open their
    /// surfaces; `Act` leaves through `run_studio_act`.
    pub(super) fn help_picker_run(&mut self) {
        let Some(picker) = self.help_state.picker.take() else {
            return;
        };
        let Some(row) = picker.rows.get(picker.selected).cloned() else {
            return;
        };
        self.help = false;
        match row.act {
            PickerAct::Act(act) => self.run_studio_act(&row.menu, act),
            PickerAct::Sheet => {
                if let Some(key) = row.menu.studio.clone() {
                    let task = row.menu.task.map(|t| t.id);
                    self.sheet_open(key, task);
                }
            }
            PickerAct::NewStudio => {
                self.new_studio_open(row.menu.machine().map(str::to_owned));
            }
        }
    }
}
