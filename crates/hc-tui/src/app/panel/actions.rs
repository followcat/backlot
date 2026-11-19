//! The dock's studio actions: the `o`/`p`/`r`/`f`/`+`/`-`/`y`/`d`
//! runners that build a `studio_cmds::Action` for the selected row
//! and run it through the one `ActionTracker` (S3 §3) — the only
//! keys that leave the view and write to the ledger.

use backlot_model::event::Priority;

use crate::panel::Target;
use crate::studio_cmds;

use super::App;

impl App {
    /// `o` on a closed person: `backlot reopen --person <role>` on
    /// the studio's machine (S2 §4). Anything else is a no-op.
    pub(super) fn panel_reopen_selected(&mut self) {
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
        if role.state != "closed" {
            return;
        }
        // The dock menu's rule: a resumable seat reopens, a fixed
        // seat (the producer) reopens anyway.
        let fixed = role.role == "producer" || !role.role.starts_with("actor");
        if !(role.resumable || fixed) {
            self.show_notice(
                "Cannot reopen",
                &format!("{} has no saved session to resume", role.name),
            );
            return;
        }
        self.run_action(studio_cmds::reopen(&key, &role.role));
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
    /// otherwise the key answers a selected person's prompt. A
    /// user-owned prompt answers at once; a director-routed one
    /// asks the override confirm first (D9, S3 §2.2).
    pub(super) fn panel_decide(&mut self, allow: bool) {
        if let Some(key) = self.panel.finish_confirm.take() {
            if allow {
                self.panel_finish_run(&key);
            }
            return;
        }
        let machines = self.panel_summaries();
        let targets = self.panel_targets();
        let index = self.panel_selected_index(&targets);
        let Some(Target::Person(key, role_name)) = targets.get(index).cloned() else {
            return;
        };
        let Some(role) = crate::panel::data::studio(&machines, &key)
            .and_then(|s| s.studio.roles.iter().find(|r| r.role == role_name))
            .cloned()
        else {
            return;
        };
        if role.attention_user {
            self.run_action(studio_cmds::decide(&key, &role.role, allow));
        } else if role.attention_seq.is_some() {
            // D9: the prompt is routed to the director — answering it
            // myself takes the §2.2 override confirm.
            self.studio_dialog = Some(crate::studio_dialog::StudioDialog::dir_override(
                &key, &role.role, allow,
            ));
        }
    }

    /// `r` while the selected row's studio has a down stage:
    /// `backlot stage start` on that machine — the `[r] start` the
    /// owed-step line prints (S2 §2). Any other row is a no-op.
    pub(super) fn panel_stage_start(&mut self) {
        let machines = self.panel_summaries();
        let targets = self.panel_targets();
        let index = self.panel_selected_index(&targets);
        let machine = match targets.get(index) {
            // The stage-down row's own machine (U6c) — Enter/click
            // already runs it; `r` on the row does too.
            Some(Target::Stage(machine)) => machine.clone(),
            Some(target) => {
                let Some(studio) = target
                    .studio()
                    .and_then(|key| crate::panel::data::studio(&machines, key))
                else {
                    return;
                };
                if self.machine_stage_running(studio.machine) {
                    return;
                }
                studio.machine.map(str::to_owned)
            }
            None => return,
        };
        if self.machine_stage_running(machine.as_deref()) {
            return;
        }
        self.run_action(studio_cmds::stage_start(machine));
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
        self.run_action(studio_cmds::finish(key, None));
    }

    /// `+`/`-` on a selected actor or task row: raise/lower the
    /// task's priority (`backlot priority --task … --as user`, S2
    /// §4). Clamped at the ends — no action is spawned there.
    pub(super) fn panel_priority(&mut self, delta: i32) {
        let machines = self.panel_summaries();
        let targets = self.panel_targets();
        let index = self.panel_selected_index(&targets);
        // (studio, task id, current level): a person's own task, or
        // the task row's task itself.
        let Some((key, task, priority)) = (match targets.get(index).cloned() {
            Some(Target::Person(key, role_name)) => crate::panel::data::studio(&machines, &key)
                .and_then(|s| {
                    s.studio
                        .roles
                        .iter()
                        .find(|r| r.role == role_name)
                        .map(|r| (key.clone(), r.task.clone(), r.priority))
                }),
            Some(Target::Task(key, id)) => {
                crate::panel::data::studio(&machines, &key).and_then(|s| {
                    s.studio
                        .tasks
                        .iter()
                        .find(|t| t.id == id)
                        .map(|t| (key.clone(), Some(t.id.clone()), Some(t.priority)))
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
        self.run_action(studio_cmds::priority(&key, &task, &next.to_string()));
    }
}
