//! The lifecycle dialogs' app wiring (W4b): `Restart`'s two choices,
//! the recast picker's `runners --json` probe, and the archived
//! studios list with its `studio restore` dry run.

use crossterm::event::KeyCode;

use crate::app::input::TerminalKey;
use crate::new_studio::{self, Probe};
use crate::studio::StudioKey;
use crate::studio_cmds;
use crate::studio_dialog::{RecastDialog, RecastStep, RestartDialog, StudioDialog};

use super::App;

impl App {
    /// `Recast…` — open the picker on the role's current identity and
    /// ask the studio's machine for `runners --json`.
    pub(crate) fn recast_dialog_open(
        &mut self,
        key: &StudioKey,
        role: &str,
        role_name: &str,
        runner: Option<String>,
        model: Option<String>,
        effort: Option<String>,
    ) {
        self.studio_dialog = Some(StudioDialog::Recast(RecastDialog::new(
            key, role, role_name, runner, model, effort,
        )));
        self.recast_probe(key);
    }

    /// The picker's `runners --json` — off the event loop, a stale
    /// generation dropped like every dialog fetch.
    fn recast_probe(&mut self, key: &StudioKey) {
        self.studio_dialog_gen += 1;
        let epoch = self.studio_dialog_gen;
        let tx = self.dialog_fetch_tx.clone();
        let run = self.ns_runner();
        let machine = key.machine.clone();
        tokio::spawn(async move {
            let out = run(machine, vec!["runners".into(), "--json".into()], None).await;
            let _ = tx.send((epoch, out)).await;
        });
    }

    // ---------- keys ----------

    pub(crate) fn restart_key(
        &mut self,
        mut d: RestartDialog,
        _key: &TerminalKey,
        code: KeyCode,
    ) -> Option<StudioDialog> {
        match code {
            KeyCode::Esc => None,
            KeyCode::Up | KeyCode::Char('k') => {
                d.move_selection(-1);
                Some(StudioDialog::Restart(d))
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                d.move_selection(1);
                Some(StudioDialog::Restart(d))
            }
            KeyCode::Char('r') => {
                d.selected = 0;
                self.restart_submit(&d)
            }
            KeyCode::Char('f') => {
                d.selected = 1;
                self.restart_submit(&d)
            }
            KeyCode::Enter if d.chosen().is_some() => self.restart_submit(&d),
            _ => Some(StudioDialog::Restart(d)),
        }
    }

    pub(crate) fn restart_submit(&mut self, d: &RestartDialog) -> Option<StudioDialog> {
        let choice = d.chosen()?;
        let action = match &d.key {
            Some(key) => studio_cmds::restart(key, &d.role, choice.fresh),
            None => studio_cmds::restart_hollywood(choice.fresh),
        };
        self.run_action(action);
        None
    }

    pub(crate) fn recast_key(
        &mut self,
        mut d: RecastDialog,
        key: &TerminalKey,
        code: KeyCode,
        mods: crossterm::event::KeyModifiers,
    ) -> Option<StudioDialog> {
        match code {
            KeyCode::Esc => return None,
            KeyCode::Up | KeyCode::Char('k') => {
                self.recast_move(&mut d, -1);
                return Some(StudioDialog::Recast(d));
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.recast_move(&mut d, 1);
                return Some(StudioDialog::Recast(d));
            }
            KeyCode::Left | KeyCode::Backspace => {
                d.step = match d.step {
                    RecastStep::Runner => RecastStep::Runner,
                    RecastStep::Model => RecastStep::Runner,
                    RecastStep::Effort => RecastStep::Model,
                    RecastStep::Apply => {
                        if d.has_effort() {
                            RecastStep::Effort
                        } else {
                            RecastStep::Model
                        }
                    }
                };
                d.error = None;
                return Some(StudioDialog::Recast(d));
            }
            KeyCode::Enter => {}
            _ => {
                let _ = (key, mods);
                return Some(StudioDialog::Recast(d));
            }
        }
        // Enter — advance, or apply on the summary.
        if self.recast_advance(&mut d) {
            return None;
        }
        Some(StudioDialog::Recast(d))
    }

    /// One Enter on the picker: next step, or the recast on the
    /// summary. `true` when the action ran and the dialog closes.
    pub(crate) fn recast_advance(&mut self, d: &mut RecastDialog) -> bool {
        match d.step {
            RecastStep::Runner => match d.selected_runner() {
                Some(_) => {
                    d.reseat();
                    d.step = RecastStep::Model;
                }
                None => d.error = Some("still loading the machine's runners…".into()),
            },
            RecastStep::Model => {
                if d.selected_model().is_some() {
                    d.step = d.next_step();
                } else {
                    d.error = Some("pick a model".into());
                }
            }
            RecastStep::Effort => {
                d.step = RecastStep::Apply;
            }
            RecastStep::Apply => {
                if let (Some(runner), Some(model)) = (d.runner_name(), d.selected_model()) {
                    let effort = d.selected_effort();
                    self.run_action(studio_cmds::recast(
                        &d.key,
                        &d.role,
                        runner,
                        &model,
                        effort.as_deref(),
                    ));
                    return true;
                }
                d.error = Some("pick a runner and a model".into());
            }
        }
        false
    }

    /// Move inside the current step's list; a runner move reseats the
    /// model and effort choices onto that runner.
    fn recast_move(&self, d: &mut RecastDialog, delta: isize) {
        let len = match d.step {
            RecastStep::Runner => d.found().len(),
            RecastStep::Model => d.models().len(),
            RecastStep::Effort => d.efforts().len(),
            RecastStep::Apply => return,
        };
        if len == 0 {
            return;
        }
        let slot = match d.step {
            RecastStep::Runner => &mut d.runner_sel,
            RecastStep::Model => &mut d.model_sel,
            RecastStep::Effort => &mut d.effort_sel,
            RecastStep::Apply => return,
        };
        *slot = (*slot as isize + delta).rem_euclid(len as isize) as usize;
        if d.step == RecastStep::Runner {
            d.reseat();
        }
    }

    /// A recast or restore fetch landed — `true` when it owned the
    /// reply. The recast parses the machine's runner rows; the restore
    /// fills the dry-run plan (or its refusal) and unlocks the apply.
    pub(crate) fn lifecycle_dialog_fetch(&mut self, result: Result<String, String>) -> bool {
        let Some(dialog) = self.studio_dialog.take() else {
            return false;
        };
        match dialog {
            StudioDialog::Recast(mut d) => {
                match result {
                    Ok(text) => match new_studio::parse_runners(&text) {
                        Ok(rows) => {
                            d.runners = Probe::Done(rows);
                            // Preselect the role's own runner when the
                            // machine still reports it.
                            d.runner_sel = d
                                .found()
                                .iter()
                                .position(|r| {
                                    Some(r.runner.as_str()) == d.current_runner.as_deref()
                                })
                                .unwrap_or(0);
                            d.reseat();
                            d.error = None;
                        }
                        Err(error) => {
                            d.runners = Probe::Failed(error.clone());
                            d.error = Some(error);
                        }
                    },
                    Err(error) => {
                        d.runners = Probe::Failed(error.clone());
                        d.error = Some(error);
                    }
                }
                self.studio_dialog = Some(StudioDialog::Recast(d));
            }
            other => {
                self.studio_dialog = Some(other);
                return false;
            }
        }
        true
    }
}
