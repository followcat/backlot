//! The dock's studio actions: the `o`/`p`/`r`/`f`/`+`/`-` runners
//! that build `backlot` argv for the selected row and spawn it on
//! the studio's machine — the only keys that leave the view and
//! write to the ledger.

use backlot_model::event::Priority;

use crate::panel::data::StudioRef;
use crate::panel::{PanelAction, Target};

use super::App;

impl App {
    /// `o` on a closed person: `backlot reopen --person <role>` on
    /// the studio's machine — the direct reopen the closed hint
    /// names (S2 §4). Anything else is a no-op.
    pub(super) fn panel_reopen_selected(&mut self) {
        let machines = self.panel_summaries();
        let targets = self.panel_targets();
        let index = self.panel_selected_index(&targets);
        let Some(Target::Person(key, role_name)) = targets.get(index).cloned() else {
            return;
        };
        let Some((studio, role)) = crate::panel::data::studio(&machines, &key).and_then(|s| {
            s.studio
                .roles
                .iter()
                .find(|r| r.role == role_name)
                .map(|r| (s, r))
        }) else {
            return;
        };
        if role.state != "closed" {
            return;
        }
        if !role.resumable {
            self.show_notice(
                "Cannot reopen",
                &format!("{} has no saved session to resume", role.name),
            );
            return;
        }
        self.panel.closed_hint = None;
        let argv = self.panel_argv(
            studio,
            &["reopen".into(), "--person".into(), role.role.clone()],
        );
        self.run_panel_action(PanelAction {
            label: format!("reopen {}", role.name),
            argv,
            machine: studio.machine.map(str::to_owned),
        });
    }

    /// `p` on a person: the role's live screen, read-only — a
    /// user-owned prompt's peek carries the `y`/`d` answer like the
    /// Call Sheet's (D5, S2 §4).
    pub(super) fn panel_peek_selected(&mut self) {
        let machines = self.panel_summaries();
        let targets = self.panel_targets();
        let index = self.panel_selected_index(&targets);
        let Some(Target::Person(key, role_name)) = targets.get(index).cloned() else {
            return;
        };
        let Some(role) = crate::panel::data::studio(&machines, &key)
            .and_then(|s| s.studio.roles.iter().find(|r| r.role == role_name))
        else {
            return;
        };
        let Some(terminal) = role.terminal.clone() else {
            self.show_notice("Peek unavailable", "the role has no screen to peek");
            return;
        };
        self.open_peek(terminal);
        if role.attention_user
            && let Some(peek) = self.peek.as_mut()
        {
            peek.prompt = Some((key.clone(), role.role.clone()));
        }
    }

    /// `y`/`d` while the dock owns the keys: an armed finish confirm
    /// answers first — `y` runs `studio finish`, `d` stands down;
    /// otherwise the key answers a selected person's user-owned
    /// prompt (`backlot approve --person <role> --decision <d>` on
    /// the studio's machine, S2 §4).
    pub(super) fn panel_decide(&mut self, allow: bool) {
        if let Some(key) = self.panel.finish_confirm.take() {
            if allow {
                self.panel_finish_run(&key);
            }
            return;
        }
        self.panel_prompt_decision(allow);
    }

    /// `r` while the selected row's studio has a down stage:
    /// `backlot stage start` on that machine — the `[r] start` the
    /// owed-step line prints (S2 §2). Any other row is a no-op.
    pub(super) fn panel_stage_start(&mut self) {
        let machines = self.panel_summaries();
        let targets = self.panel_targets();
        let index = self.panel_selected_index(&targets);
        let Some(key) = targets.get(index).and_then(|t| t.studio()) else {
            return;
        };
        let Some(studio) = crate::panel::data::studio(&machines, key) else {
            return;
        };
        if self.machine_stage_running(studio.machine) {
            return;
        }
        let mut argv = vec!["stage".into(), "start".into(), "--as".into(), "user".into()];
        if studio.machine.is_none() {
            argv.extend(self.panel_endpoint_args.iter().cloned());
        }
        self.run_panel_action(PanelAction {
            label: format!("stage start {}", studio.machine.unwrap_or("Local")),
            argv,
            machine: studio.machine.map(str::to_owned),
        });
    }

    /// `f` on a ready-to-finish studio's rows: arm the inline
    /// `finish <id>? [y/n]` line — `y` runs `studio finish`, `n` or
    /// `Esc` stands down (S2 §2's `[f]` hint made real).
    pub(super) fn panel_finish_selected(&mut self) {
        let machines = self.panel_summaries();
        let targets = self.panel_targets();
        let index = self.panel_selected_index(&targets);
        let Some(key) = targets.get(index).and_then(|t| t.studio()).cloned() else {
            return;
        };
        let ready =
            crate::panel::data::studio(&machines, &key).is_some_and(|s| s.studio.ready_to_finish);
        if ready {
            self.panel.finish_confirm = Some(key);
        }
    }

    /// The armed finish confirm's `y`: `backlot studio finish` on
    /// the studio's machine — the same command the Call Sheet's
    /// confirm runs (S1 §5).
    pub(super) fn panel_finish_run(&mut self, key: &crate::studio::StudioKey) {
        let machines = self.panel_summaries();
        let Some(studio) = crate::panel::data::studio(&machines, key) else {
            return;
        };
        let argv = self.panel_argv(studio, &["studio".into(), "finish".into()]);
        self.run_panel_action(PanelAction {
            label: format!("studio finish {}", key.id),
            argv,
            machine: studio.machine.map(str::to_owned),
        });
    }

    /// `y`/`d` on a person with a user-owned prompt: `backlot
    /// approve --person <role> --decision <d>` on the studio's
    /// machine — the same command the Call Sheet rows run (S2 §4).
    /// A director-owned or absent prompt is a no-op.
    pub(super) fn panel_prompt_decision(&mut self, allow: bool) {
        let machines = self.panel_summaries();
        let targets = self.panel_targets();
        let index = self.panel_selected_index(&targets);
        let Some(Target::Person(key, role_name)) = targets.get(index).cloned() else {
            return;
        };
        let owned = crate::panel::data::studio(&machines, &key)
            .and_then(|s| s.studio.roles.iter().find(|r| r.role == role_name))
            .is_some_and(|r| r.attention_user);
        if owned {
            self.hollywood_prompt_decision(&key, &role_name, allow);
        }
    }

    /// `+`/`-` on a selected actor or task row: raise/lower the
    /// task's priority (`backlot priority --task … --as user`, S2
    /// §4). Clamped at the ends — no action is spawned there.
    pub(super) fn panel_priority(&mut self, delta: i32) {
        let machines = self.panel_summaries();
        let targets = self.panel_targets();
        let index = self.panel_selected_index(&targets);
        // (studio, task id, current level, label word): a person's
        // own task, or the task row's task itself.
        let Some((studio, task, priority, label)) = (match targets.get(index).cloned() {
            Some(Target::Person(key, role_name)) => crate::panel::data::studio(&machines, &key)
                .and_then(|s| {
                    s.studio
                        .roles
                        .iter()
                        .find(|r| r.role == role_name)
                        .map(|r| (s, r.task.clone(), r.priority, r.name.clone()))
                }),
            Some(Target::Task(key, id)) => {
                crate::panel::data::studio(&machines, &key).and_then(|s| {
                    s.studio
                        .tasks
                        .iter()
                        .find(|t| t.id == id)
                        .map(|t| (s, Some(t.id.clone()), Some(t.priority), t.id.clone()))
                })
            }
            _ => None,
        }) else {
            return;
        };
        let (Some(task), Some(priority)) = (task, priority) else {
            return; // only actors/tasks carry a task priority
        };
        let next = match (delta > 0, priority) {
            (true, Priority::Low) => Some(Priority::Normal),
            (true, Priority::Normal) => Some(Priority::High),
            (false, Priority::High) => Some(Priority::Normal),
            (false, Priority::Normal) => Some(Priority::Low),
            _ => None,
        };
        let Some(next) = next else { return };
        let argv = self.panel_argv(
            studio,
            &[
                "priority".into(),
                "--task".into(),
                task,
                "--level".into(),
                next.to_string(),
            ],
        );
        self.run_panel_action(PanelAction {
            label: format!("priority {} {}", label, next),
            argv,
            machine: studio.machine.map(str::to_owned),
        });
    }

    /// The argv tail for a panel command — the command word first
    /// (`backlot` dispatches studio commands on argv[1]), then its
    /// flags, then the common `--studio`/`--as` and, for a local
    /// studio, the endpoint flags. A remote studio's tail carries no
    /// endpoint flags: the terminal it runs in on that machine
    /// reaches that machine's core through its own `HC_*` variables.
    pub(super) fn panel_argv(&self, studio: StudioRef<'_>, command: &[String]) -> Vec<String> {
        let mut argv = command.to_vec();
        argv.extend([
            "--studio".into(),
            studio.studio.id.clone(),
            "--as".into(),
            "user".into(),
        ]);
        if studio.machine.is_none() {
            argv.extend(self.panel_endpoint_args.iter().cloned());
        }
        argv
    }

    /// Spawn the action off the event loop through the injected
    /// runner; the result comes back on `panel_rx`. The opening
    /// page's `s` runs `stage start` through the same path.
    pub(crate) fn run_panel_action(&mut self, action: PanelAction) {
        let future = match &action.machine {
            None => (self.panel_runner)(action.argv.clone()),
            Some(machine) => match &self.remote_runner {
                Some(runner) => runner.run(machine.clone(), action.argv.clone()),
                None => {
                    self.show_notice(
                        "Studio action failed",
                        &format!("{}: cannot reach {machine}", action.label),
                    );
                    return;
                }
            },
        };
        let tx = self.panel_action_tx.clone();
        let label = action.label;
        tokio::spawn(async move {
            let _ = tx.send((label, future.await)).await;
        });
    }

    /// The action's result — a non-zero exit shows its first stderr
    /// line (the runner extracted it).
    pub(crate) fn on_panel_action(&mut self, (label, result): (String, Result<(), String>)) {
        if let Err(error) = result {
            self.show_notice("Studio action failed", &format!("{label}: {error}"));
        }
    }
}
