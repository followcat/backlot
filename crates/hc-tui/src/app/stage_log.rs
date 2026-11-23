//! S1's App wiring: the stage's terminal is a read-only log. Its keys
//! are dropped, its frame says `stage log — read only`, and the machine
//! menu's `Stage log` opens it as the existing observe-only peek.
//!
//! Starting the stage never steals the focus: a start from anywhere
//! keeps the focus it had, and one from the opening page lands on a
//! studio's view or on the lot once the stage publishes its studios.

use super::{App, StageFocus};

impl App {
    /// `term` is a stage terminal — its keys are dropped and its frame
    /// says `stage log — read only`.
    pub(crate) fn is_stage_terminal(&self, terminal: &str) -> bool {
        self.view
            .inventory
            .get(terminal)
            .is_some_and(crate::panel::data::is_stage_terminal)
    }

    /// The attached pane is the stage log — the read-only arm.
    pub(super) fn stage_terminal_focused(&self) -> bool {
        self.pane
            .target()
            .is_some_and(|terminal| self.is_stage_terminal(terminal))
    }

    /// The machine menu's `stage log`: the live stage terminal on
    /// `machine`, opened as the existing observe-only peek.
    pub(super) fn open_stage_log(&mut self, machine: Option<String>) {
        let address = self
            .view
            .inventory
            .terminals()
            .values()
            .find(|term| {
                term.state == "live"
                    && crate::panel::data::is_stage_terminal(term)
                    && term.address.split_once('/').map(|(m, _)| m) == machine.as_deref()
            })
            .map(|term| term.address.clone());
        match address {
            Some(address) => self.open_peek(address),
            None => self.show_notice(
                "Stage log",
                &format!(
                    "no stage is running on {}",
                    machine.as_deref().unwrap_or("Local")
                ),
            ),
        }
    }

    /// A stage start begins: remember the focus it must keep, and
    /// whether the opening page was the surface.
    pub(super) fn note_stage_start(&mut self) {
        if self.plain || self.stage_focus.is_some() {
            return;
        }
        self.stage_focus = Some(StageFocus {
            restore: self.focused.clone(),
            from_opening: self.opening_visible(),
        });
    }

    /// A publish landed while a stage start's focus was owed: keep the
    /// remembered focus, or — from the opening page — land on the
    /// studio's view or the lot. Never the stage log.
    pub(super) fn land_after_stage_start(&mut self) {
        let Some(plan) = self.stage_focus.clone() else {
            return;
        };
        if !self
            .focused
            .as_deref()
            .is_some_and(|t| self.is_stage_terminal(t))
            && self.focused.is_some()
        {
            // The focus never moved (or the person moved it): done.
            self.stage_focus = None;
            return;
        }
        if let Some(restore) = plan.restore.clone()
            && self
                .view
                .inventory
                .get(&restore)
                .is_some_and(|t| t.state == "live")
            && !self.is_stage_terminal(&restore)
        {
            self.focused = Some(restore);
            self.stage_focus = None;
            return;
        }
        if !plan.from_opening {
            self.stage_focus = None;
            return;
        }
        let machines = self.panel_summaries();
        let studios = crate::panel::data::studios(&machines);
        let live: Vec<_> = studios
            .iter()
            .filter(|studio| studio.studio.state != "finished")
            .collect();
        let target = self
            .studio
            .current
            .clone()
            .or_else(|| (live.len() == 1).then(|| live[0].key()));
        match target {
            Some(key) if studios.iter().any(|studio| studio.key() == key) => {
                self.stage_focus = None;
                self.focus_studio(&key);
            }
            _ if !studios.is_empty() => {
                // Nothing to land on: the lot owns the keys, not the log.
                self.stage_focus = None;
                self.panel.focused = true;
            }
            // The stage has not published the studios yet — wait for it.
            _ => {}
        }
    }
}
