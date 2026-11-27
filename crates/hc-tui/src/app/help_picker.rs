//! The launcher's compact target picker (S5 §1): the row list each
//! `Pick` builds — machines for `+ new studio`, studios for the
//! studio verbs, tasks for the sheet and `pause`-style verbs, closed
//! people for reopen — and its keys. Enter lands on `run_studio_act`,
//! the same arm every other surface's activation takes (S3).

use super::App;
use super::help_launcher::help_place;
use crate::focus::CastTarget;
use crate::help_launcher::{HelpPicker, Pick, PickerAct, PickerRow};
use crate::panel::data;
use crate::studio::StudioKey;
use crate::studio_menu::{StudioAct, StudioMenu};

/// Compare two task ids with embedded numbers sorted numerically (`t1 < t2 < … < t10 < t16`).
fn cmp_task_id_numeric(a: &str, b: &str) -> std::cmp::Ordering {
    fn next_chunk(s: &str) -> (&str, &str) {
        let mut chars = s.char_indices();
        let Some((_, first)) = chars.next() else {
            return ("", "");
        };
        let want_digit = first.is_ascii_digit();
        for (idx, ch) in chars {
            if ch.is_ascii_digit() != want_digit {
                return (&s[..idx], &s[idx..]);
            }
        }
        (s, "")
    }
    let (mut ra, mut rb) = (a, b);
    while !ra.is_empty() && !rb.is_empty() {
        let (ca, next_a) = next_chunk(ra);
        let (cb, next_b) = next_chunk(rb);
        let ord = if ca.starts_with(|c: char| c.is_ascii_digit())
            && cb.starts_with(|c: char| c.is_ascii_digit())
        {
            let ta = ca.trim_start_matches('0');
            let tb = cb.trim_start_matches('0');
            ta.len()
                .cmp(&tb.len())
                .then_with(|| ta.cmp(tb))
                .then_with(|| ca.len().cmp(&cb.len()))
        } else {
            ca.cmp(cb)
        };
        if ord != std::cmp::Ordering::Equal {
            return ord;
        }
        ra = next_a;
        rb = next_b;
    }
    ra.len().cmp(&rb.len())
}

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
                        dim: None,
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
                        dim: None,
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
                        dim: None,
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
                                    dim: None,
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
                                    dim: None,
                                    menu,
                                    act: PickerAct::Act(StudioAct::Reopen),
                                })
                        })
                })
                .collect(),
        }
    }

    /// Build the studio-scoped reopen picker rows for `key`, sorted
    /// numerically by task id (`t1, t2, … t10, t16`), with taskless roles
    /// last by name. Offline/unresumable rows stay present and dimmed.
    pub(crate) fn studio_reopen_picker_rows(&self, key: &StudioKey) -> Vec<PickerRow> {
        let machines = self.panel_summaries();
        let Some(s) = data::studio(&machines, key) else {
            return Vec::new();
        };
        let offline = !self.machine_reachable(s.machine);
        let mut closed: Vec<_> = s
            .studio
            .roles
            .iter()
            .filter(|r| r.state == "closed")
            .collect();
        closed.sort_by(|a, b| {
            let ta = a.task.as_deref().filter(|t| !t.is_empty());
            let tb = b.task.as_deref().filter(|t| !t.is_empty());
            match (ta, tb) {
                (Some(ida), Some(idb)) => cmp_task_id_numeric(ida, idb)
                    .then_with(|| a.name.cmp(&b.name))
                    .then_with(|| a.role.cmp(&b.role)),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.name.cmp(&b.name).then_with(|| a.role.cmp(&b.role)),
            }
        });
        closed
            .into_iter()
            .filter_map(|r| {
                let menu = StudioMenu::role(key, r, offline);
                let dim = menu
                    .items()
                    .into_iter()
                    .find(|i| i.act == StudioAct::Reopen)
                    .map(|i| i.dim)?;
                let tid = r.task.as_deref().filter(|t| !t.is_empty());
                let label = match tid {
                    Some(tid) => {
                        let title = s
                            .studio
                            .tasks
                            .iter()
                            .find(|t| t.id == tid)
                            .map(|t| t.title.trim())
                            .filter(|t| !t.is_empty());
                        match title {
                            Some(title) => format!("{tid}  {}  {title}", r.name),
                            None => format!("{tid}  {}", r.name),
                        }
                    }
                    None => r.name.clone(),
                };
                Some(PickerRow {
                    label,
                    dim,
                    menu,
                    act: PickerAct::Act(StudioAct::Reopen),
                })
            })
            .collect()
    }

    /// Open the standalone reopen picker for one studio's closed roles.
    pub(crate) fn open_cast_reopen_picker(&mut self, key: &StudioKey) {
        let rows = self.studio_reopen_picker_rows(key);
        if rows.is_empty() {
            return;
        }
        self.help_state.picker = Some(HelpPicker {
            title: "reopen a closed role".to_owned(),
            rows,
            selected: 0,
            scroll: 0,
            standalone: true,
        });
        self.help = true;
    }

    /// Handle `o` inside focused Cast: opens the studio-scoped reopen picker
    /// on the closed-roles line (or on a closed role in legacy chrome).
    pub(super) fn cast_reopen(&mut self) {
        match self.cast_focus.clone() {
            Some(CastTarget::ClosedRoles { studio }) => self.open_cast_reopen_picker(&studio),
            Some(CastTarget::Role { studio, role }) => {
                let machines = self.panel_summaries();
                let is_closed = data::studio(&machines, &studio).is_some_and(|s| {
                    s.studio
                        .roles
                        .iter()
                        .any(|r| r.role == role && r.state == "closed")
                });
                if is_closed {
                    self.open_cast_reopen_picker(&studio);
                }
            }
            _ => {}
        }
    }

    /// Keys while the picker is open — arrows move, Enter runs the
    /// row, Esc/`?` steps back to the launcher list (or closes standalone).
    pub(crate) fn help_picker_key(&mut self, key: &crate::app::input::TerminalKey) {
        use crossterm::event::KeyCode;
        let combo = crate::input::terminal_key_of(key);
        if key.input.action != hc_vt::Action::Press {
            return;
        }
        let vis = self.help_hits.picker_rows.len().max(1);
        match combo.code {
            KeyCode::Esc | KeyCode::Char('?') => {
                let standalone = self
                    .help_state
                    .picker
                    .as_ref()
                    .is_some_and(|p| p.standalone);
                self.help_state.picker = None;
                if standalone {
                    self.help = false;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(picker) = self.help_state.picker.as_mut()
                    && picker.selected + 1 < picker.rows.len()
                {
                    picker.selected += 1;
                    picker.scroll = crate::chrome::help_picker::visible_start(picker, vis);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(picker) = self.help_state.picker.as_mut() {
                    picker.selected = picker.selected.saturating_sub(1);
                    picker.scroll = crate::chrome::help_picker::visible_start(picker, vis);
                }
            }
            KeyCode::Enter => self.help_picker_run(),
            _ => {}
        }
    }

    /// Enter on a picker row — `Sheet` and `NewStudio` open their
    /// surfaces; `Act` leaves through `run_studio_act` (dimmed rows
    /// close without running).
    pub(super) fn help_picker_run(&mut self) {
        let Some(picker) = self.help_state.picker.take() else {
            return;
        };
        let Some(row) = picker.rows.get(picker.selected).cloned() else {
            self.help = false;
            return;
        };
        self.help = false;
        if row.dim.is_some() {
            return;
        }
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
