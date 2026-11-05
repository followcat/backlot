//! Pending keys cannot outlive their program incarnation or their key-up.
use super::*;
impl Pane {
    pub(super) fn discard_pending_incarnation(&mut self) {
        self.allocation_changed_disconnected |= self
            .pending
            .iter()
            .any(|item| matches!(item, Pending::Control(Out::Resize(..))));
        self.focus_changed_disconnected |= self
            .pending
            .iter()
            .any(|item| matches!(item, Pending::Control(Out::Focus(_))));
        let dropped = self
            .pending
            .iter()
            .filter(|item| !matches!(item, Pending::Control(_)))
            .count();
        if dropped > 0 {
            self.input_rejection =
                Some("Queued input belonged to an ended or ambiguous stream batch".into());
        }
        self.dropped_input += dropped as u64;
        self.pending
            .retain(|item| matches!(item, Pending::Control(_)));
        self.pending_bytes = 0;
    }

    pub(crate) fn cancel_undispatched_key(&mut self, key: &crate::app::input::TerminalKey) {
        let before = self.pending.len();
        self.pending.retain(|item| match item {
            Pending::Key(pending) | Pending::Prepared(_, pending) => {
                pending.identity() != key.identity()
            }
            Pending::Encoded(_, id) => *id != Some(key.dispatch_id),
            Pending::Paste(_) | Pending::Mouse(_, _) | Pending::Control(_) => true,
        });
        let removed = before - self.pending.len();
        self.pending_bytes = self.pending.iter().map(charge).sum();
        if removed > 0 {
            self.dropped_input += removed as u64;
            self.input_rejection =
                Some("Disconnected key tap was cancelled before delivery".into());
        }
    }
}
