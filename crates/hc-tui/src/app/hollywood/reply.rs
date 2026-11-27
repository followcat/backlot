//! The Call Sheet compose line's one-moment reply target.

use super::App;
use crate::studio::StudioKey;
use crate::studio_cmds;

impl App {
    /// Newest explicit question seq a role opened, projected in the studio exchange.
    pub(crate) fn ask_target(&self, studio: &StudioKey, role: &str) -> Option<u64> {
        let machines = self.panel_summaries();
        let studio = crate::panel::data::studio(&machines, studio)?;
        let open = studio
            .studio
            .roles
            .iter()
            .any(|r| r.role == role && r.asks.is_some());
        if !open {
            return None;
        }
        studio
            .studio
            .exchange
            .iter()
            .filter(|e| e.ask && e.who == role)
            .map(|e| e.seq)
            .max()
    }

    /// A reply belongs to one open ask only: once that ask is gone
    /// from the published document — answered, acknowledged, finished,
    /// or its studio vanished — Compose is Hollywood's again.
    pub(crate) fn sync_hollywood_reply_target(&mut self) {
        let Some((studio, role)) = self.hollywood.reply_to.clone() else {
            return;
        };
        if self.ask_target(&studio, &role).is_none() {
            self.hollywood.reply_to = None;
        }
    }

    /// `x` on an ASKS row: acknowledge the question without answering it.
    pub(super) fn hollywood_ack(&mut self, studio: &StudioKey, role: &str) {
        match self.ask_target(studio, role) {
            Some(target) => self.run_action(studio_cmds::ask_acknowledge(studio, role, target)),
            None => self.show_notice("Hollywood", "that question was already answered elsewhere"),
        }
    }

    /// `r` on an ASKS row: compose the person's reply in the sheet's own
    /// compose line, not in a dialog away from the question.
    pub(in crate::app) fn hollywood_reply(&mut self, studio: &StudioKey, role: &str) {
        self.hollywood.reply_to = Some((studio.clone(), role.to_string()));
        self.hollywood_select(crate::hollywood::Target::Input);
        self.hollywood.editing = true;
    }
}
