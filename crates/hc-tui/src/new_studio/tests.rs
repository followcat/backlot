//! New Studio's draw tests — the dialog must `Clear` its rect after
//! the caller's dim pass and before the panel fill, so a dead pane's
//! frozen screen (reverse video, foreign colours) never bleeds
//! through the modal.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::step3::version_label;
use super::{NewStudio, render};
use crate::chrome::palette::Palette;

/// A dead pane's frozen screen: every cell a reverse-video red `#`.
fn frozen_screen(buf: &mut Buffer) {
    let style = Style::default()
        .fg(Color::Red)
        .add_modifier(Modifier::REVERSED | Modifier::UNDERLINED);
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            buf[(x, y)].set_symbol("#").set_style(style);
        }
    }
}

/// Nothing the frozen screen left survives inside the dialog's rect.
fn assert_no_bleed(buf: &Buffer, area: Rect) {
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let cell = &buf[(x, y)];
            assert!(
                !cell
                    .modifier
                    .intersects(Modifier::REVERSED | Modifier::UNDERLINED),
                "foreign modifier at ({x}, {y}): {:?}",
                cell.modifier
            );
            assert_ne!(cell.fg, Color::Red, "foreign colour at ({x}, {y})");
            assert_ne!(cell.symbol(), "#", "frozen glyph at ({x}, {y})");
        }
    }
}

#[test]
fn the_dialog_clears_the_frozen_screen_behind_it() {
    let band = Rect::new(0, 0, 80, 24);
    let mut buf = Buffer::empty(band);
    frozen_screen(&mut buf);
    let palette = Palette::catppuccin();
    let hits = render(&mut buf, &NewStudio::new(), &palette, band);
    assert!(hits.area.width > 0, "the frame must fit the band");
    assert_no_bleed(&buf, hits.area);
}

/// The detection line names each runner once: `claude --version`
/// already prints `claude 2.1.0`, and `fake` prints `fake-runner
/// 1.0` — both lose the duplicate token.
#[test]
fn runner_versions_never_repeat_the_runner_name() {
    assert_eq!(version_label("claude", "claude 2.1.0"), "claude 2.1.0");
    assert_eq!(version_label("Claude", "claude 2.1.0"), "Claude 2.1.0");
    assert_eq!(version_label("fake", "fake-runner 1.0"), "fake 1.0");
    assert_eq!(version_label("codex", "codex 0.9.0"), "codex 0.9.0");
    // A version that is only the name prints it once.
    assert_eq!(version_label("cline", "cline"), "cline");
    // A version without the name keeps both words.
    assert_eq!(version_label("codex", "0.9.0"), "codex 0.9.0");
    // A longer word that merely starts with the name is not a token.
    assert_eq!(version_label("fake", "fake-runner2"), "fake fake-runner2");
}
