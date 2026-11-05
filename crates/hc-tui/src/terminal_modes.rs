//! Host mouse-reporting mode sequences. Derived from herdr
//! src/terminal_modes.rs (Apache-2.0).

use std::io::{self, Write};

/// Disables every mouse-reporting mode the host could have on —
/// including SGR pixels (`1016`) and the legacy `9`/`1005`/`1015`
/// encodings — so a straggler host mode never feeds pixels where the
/// reader expects cells, or vice versa.
const DISABLE_HOST_MOUSE_REPORTING_SEQUENCE: &[u8] =
    b"\x1b[?1006l\x1b[?1016l\x1b[?1015l\x1b[?1005l\x1b[?1003l\x1b[?1002l\x1b[?1000l\x1b[?9l";

/// Clears all known host mouse modes — written unconditionally at
/// setup before capture is enabled, and again on restore.
pub(crate) fn clear_host_mouse_reporting<W: Write>(writer: &mut W) -> io::Result<()> {
    writer.write_all(DISABLE_HOST_MOUSE_REPORTING_SEQUENCE)?;
    writer.flush()
}

/// Source's set_mouse_capture: clear all formats before the negotiated one.
pub(crate) fn mouse_capture_sequence(enabled: bool, pixels: bool) -> io::Result<Vec<u8>> {
    use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
    let mut output = Vec::new();
    clear_host_mouse_reporting(&mut output)?;
    if enabled {
        crossterm::queue!(output, EnableMouseCapture)?;
        if pixels {
            output.extend_from_slice(b"\x1b[?1016h");
        }
    } else {
        crossterm::queue!(output, DisableMouseCapture)?;
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The clear sequence names every mode the reference resets —
    /// a missing entry would leave a stale host mode live.
    #[test]
    fn clears_all_known_host_mouse_modes() {
        let mut output = Vec::new();
        clear_host_mouse_reporting(&mut output).unwrap();
        let text = String::from_utf8_lossy(&output);
        for mode in ["9", "1000", "1002", "1003", "1005", "1006", "1015", "1016"] {
            assert!(
                text.contains(&format!("[?{mode}l")),
                "missing mouse mode {mode}"
            );
        }
    }
}
