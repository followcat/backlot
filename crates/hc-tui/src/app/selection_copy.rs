// Derived from herdr src/client/shell/mouse.rs `request_selection_copy` and
// src/pane/terminal.rs `extract_selection` (Apache-2.0).
//! Copying a finished selection through OSC 52.

use super::App;

impl App {
    /// Extract the selection's text from the pane's own displayed VT —
    /// atomic at release — and write it to the host terminal as OSC 52.
    /// Empty reads (a blank stretch, a pane without a VT) write nothing.
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
    }
}
