//! The pane's last painted ratatui surface (`r2-load-lag.md` R2-T4).
//!
//! The cache is keyed by everything a repaint consults: the VT's
//! identity (a fresh `Vt` can never alias a dead one's cells), the
//! inner rect the surface was composed for, the `Paint` resolution
//! inputs and the program's live default/palette colors. A dirty VT
//! repaints only the rows its render state reports; a clean VT replays
//! this buffer verbatim, so ratatui's swap-cleared frame re-shows the
//! pane for zero cell reads.

use super::Paint;

/// One pane's painted surface, pane-local (`Rect::new(0, 0, w, h)`).
/// Wide-grapheme tail slots carry their `CellDiffOption::Skip` mark —
/// the buffer copy replays them so a narrowing head re-emits the slot.
pub(crate) struct PaneRenderCache {
    /// `Vt::id` of the emulator these cells came from.
    pub vt_id: u64,
    /// Inner rect the surface was composed for.
    pub width: u16,
    pub height: u16,
    /// The `Paint` inputs the styles resolved under.
    pub paint: Paint,
    /// The VT defaults/palette at paint time — a program OSC change
    /// mismatches this and forces a full repaint.
    pub colors: hc_vt::Colors,
    /// The painted cells, row-major over `(0, 0, width, height)`.
    pub buf: ratatui::buffer::Buffer,
}
