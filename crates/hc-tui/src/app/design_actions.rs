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

    /// `o` on an opening-design row: open the design document in PR 6's Design Reader (`ReaderKind::Design`).
    pub(in crate::app) fn design_read(&mut self, key: &StudioKey) {
        let Some(design) = self.any_design(key) else {
            return;
        };
        let file = if design.doc_file.starts_with("design/") {
            design.doc_file
        } else {
            format!("design/v{}.md", design.version)
        };
        self.run_action(studio_cmds::design_show(key, &file));
    }

    /// `↵` on a Call Sheet `Wait::Decision` with proposed checks: open a read-only
    /// Decision show view starting with `Signing off also activates N checks:` and
    /// every proposed check's name, command, timeout, and expect verbatim.
    pub(in crate::app) fn design_decision_show(&mut self, key: &StudioKey) {
        let Some(design) = self.waiting_design(key) else {
            return;
        };
        let file = if design.doc_file.starts_with("design/") {
            design.doc_file.clone()
        } else {
            format!("design/v{}.md", design.version)
        };
        let body = crate::hollywood::decision::show_body(&design);
        let lines: Vec<String> = body.lines().map(str::to_string).collect();
        if lines.is_empty() {
            self.design_read(key);
            return;
        }
        let machines = self.panel_summaries();
        let meta =
            crate::app::hollywood::reader_meta_from::design_meta(&machines, key.clone(), &file);
        self.hollywood.view = Some(crate::hollywood::CardView {
            card: None,
            title: "Design".to_owned(),
            lines,
            raw_body: body,
            scroll: 0,
            meta,
            ..crate::hollywood::CardView::default()
        });
    }

    /// `a` on an opening-design row: open the shared sign-off/skip confirmation modal
    /// before running `backlot design sign [--skip]`.
    pub(in crate::app) fn design_sign_confirm(&mut self, key: &StudioKey) {
        let Some(design) = self.waiting_design(key) else {
            return;
        };
        let skip = design.suggest_skip || design.phase == DesignPhase::SuggestSkip;
        let file = if design.doc_file.is_empty() {
            format!("design/v{}.md", design.version)
        } else {
            design.doc_file.clone()
        };
        self.studio_dialog = Some(crate::studio_dialog::StudioDialog::design_sign(
            key,
            skip,
            Some(&file),
            &design.proposed_checks,
        ));
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
            self.hollywood_reply(key, "producer", crate::hollywood::ReplyTargetKind::Design);
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
