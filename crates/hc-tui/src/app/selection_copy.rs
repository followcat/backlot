// Derived from herdr src/client/shell/mouse.rs `request_selection_copy`,
// src/pane/terminal.rs `extract_selection` and
// src/client/clipboard_forwarding.rs (Apache-2.0).
//! Copying through OSC 52: a finished selection, and what programs copy.

use std::time::Instant;

use super::App;

impl App {
    /// Write what programs in panes copied to the host terminal. The focused
    /// pane's input wire carries the same bytes as its observer, and a peek
    /// only looks at a worker, so observers and popups are the ones that
    /// count: each copy reaches the clipboard once. The dedup holds only
    /// while the observer stays painted — an opening or reattaching wire
    /// clears its captures on first paint, so then the active wire is the
    /// only live copy and its writes go instead.
    pub(super) fn drain_clipboard_writes(&mut self) {
        let mut writes = self.pane.take_clipboard_writes();
        if let Some(peek) = self.peek.as_mut() {
            peek.pane.take_clipboard_writes();
        }
        if self
            .pane
            .target()
            .and_then(|id| self.observers.get(id))
            .is_some_and(|observer| observer.is_painted())
        {
            writes.clear();
        }
        writes.extend(
            self.observers
                .values_mut()
                .chain(self.popup.as_mut().map(|popup| &mut popup.pane))
                .flat_map(|pane| pane.take_clipboard_writes()),
        );
        let mut seen = std::collections::HashSet::new();
        for content in writes {
            if !seen.insert(content.clone()) {
                continue;
            }
            let sequence = crate::selection::osc52_sequence(&content);
            let _ = (self.host_control)(sequence.as_bytes());
            if self.show_copy_feedback(Instant::now()) {
                self.sched.mark_dirty(Instant::now());
            }
        }
    }

    /// Extract the selection's text from the pane's own displayed VT —
    /// atomic at release — and write it to the host terminal as OSC 52.
    /// Empty reads (a blank stretch, a pane without a VT) write nothing.
    /// A written copy raises the reference's `copied to clipboard` card.
    pub(super) fn request_selection_copy(&mut self) {
        let Some(selection) = self.selection.as_ref() else {
            return;
        };
        let ((start_row, start_col), (end_row, end_col)) = selection.ordered_cells();
        let pane_id = selection.pane_id.clone();
        let text = self
            .displayed_pane(&pane_id)
            .and_then(|pane| pane.vt_ref())
            .and_then(|vt| {
                vt.read_text_screen((start_col, start_row), (end_col, end_row), false)
                    .ok()
            });
        let Some(text) = text.filter(|text| !text.is_empty()) else {
            return;
        };
        let sequence = crate::selection::osc52_sequence(text.as_bytes());
        let _ = (self.host_control)(sequence.as_bytes());
        if self.show_copy_feedback(Instant::now()) {
            self.sched.mark_dirty(Instant::now());
        }
    }
}
