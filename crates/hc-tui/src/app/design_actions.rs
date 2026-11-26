//! Shared PR 7 App actions for opening-design decisions (`r` reply/discuss,
//! `o` read design doc in Reader, `a` sign/skip confirmation dialog) invoked
//! uniformly from Call Sheet and Lot Dock.

use backlot_model::design::DesignPhase;
use backlot_model::summary::DesignSummary;

use super::App;
use crate::studio::StudioKey;
use crate::studio_cmds;

impl App {
    /// Active waiting `DesignSummary` for `studio`, if any.
    pub(in crate::app) fn waiting_design(&self, key: &StudioKey) -> Option<DesignSummary> {
        let summaries = self.panel_summaries();
        let m = summaries
            .iter()
            .find(|m| m.machine.as_deref() == key.machine.as_deref())?;
        let studio = m.summary.studios.iter().find(|s| s.id == key.id)?;
        if studio.needs_upgrade || studio.state == "finished" {
            return None;
        }
        studio
            .design
            .as_ref()
            .filter(|d| d.waiting_user || d.phase.is_waiting_user())
            .cloned()
    }

    /// Any non-idle/non-finished `DesignSummary` for `studio` (including role turns
    /// when reading via `o` in Lot Dock).
    pub(in crate::app) fn any_design(&self, key: &StudioKey) -> Option<DesignSummary> {
        let summaries = self.panel_summaries();
        let m = summaries
            .iter()
            .find(|m| m.machine.as_deref() == key.machine.as_deref())?;
        let studio = m.summary.studios.iter().find(|s| s.id == key.id)?;
        studio
            .design
            .as_ref()
            .filter(|d| d.phase != DesignPhase::Idle)
            .cloned()
    }

    /// `o` on an opening-design row: load `backlot design show` into the Reader.
    pub(in crate::app) fn design_read(&mut self, key: &StudioKey) {
        let Some(design) = self.any_design(key) else {
            return;
        };
        self.run_action(studio_cmds::design_show(
            key,
            &design.doc_file,
            design.version,
        ));
    }

    /// `a` on an opening-design row: open confirmation showing mode (`sign off`
    /// vs `skip`) and the file that will be frozen before running `design sign`.
    pub(in crate::app) fn design_sign_confirm(&mut self, key: &StudioKey) {
        let Some(design) = self.waiting_design(key) else {
            return;
        };
        self.studio_dialog = Some(crate::hollywood::design_confirm_dialog(key, &design));
    }

    /// `r` on a Call Sheet `Wait::Decision`: on `SuggestSkip` runs `design open`
    /// (`DesignStartRound`); otherwise focuses Call Sheet compose replying to producer.
    pub(in crate::app) fn design_sheet_reply_or_open(&mut self, key: &StudioKey) {
        let Some(design) = self.waiting_design(key) else {
            return;
        };
        if design.suggest_skip || design.phase == DesignPhase::SuggestSkip {
            self.run_action(studio_cmds::design_open(key));
        } else {
            self.hollywood_reply(key, "producer");
        }
    }

    /// `r` on a Lot Dock `Target::Design`: on `SuggestSkip` runs `design open`
    /// (`DesignStartRound`); otherwise focuses the studio's compose row to producer.
    pub(in crate::app) fn design_panel_reply_or_open(&mut self, key: &StudioKey) {
        let Some(design) = self.waiting_design(key) else {
            return;
        };
        if design.suggest_skip || design.phase == DesignPhase::SuggestSkip {
            self.run_action(studio_cmds::design_open(key));
        } else {
            self.panel_input_focus_studio(key);
        }
    }
}
