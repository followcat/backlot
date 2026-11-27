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

#[test]
fn wizard_72_cols_step2_renders_deco_greenlight_and_plaque_step_row() {
    use super::{Step, render_styled};

    let palette = Palette::backlot();
    let elev = palette.elevation();
    let band = Rect::new(0, 0, 72, 24);
    let mut buf = Buffer::empty(band);
    let mut ns = NewStudio::new();
    ns.step = Step::IdTitle;
    let hits = render(&mut buf, &ns, &palette, band);
    let dump: String = (0..band.height)
        .map(|y| {
            (0..band.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        dump.contains("╔══╡ ✦ NEW STUDIO · [2/4] ID & Title ╞"),
        "{dump}"
    );
    assert!(dump.contains("esc step back ══╗"), "{dump}");
    assert!(dump.contains("✓ 1 Machine & Repo"), "{dump}");
    assert!(dump.contains("╡ 2 ID & Title ╞"), "{dump}");
    assert!(dump.contains("3 Cast"), "{dump}");
    assert!(dump.contains("4 Check & Start"), "{dump}");

    // Locate `✓`, `▸`, and `╡ 2 ID & Title ╞` directly on the step row to verify styles.
    let step_y = (0..band.height)
        .find(|&y| {
            (0..band.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .contains("╡ 2 ID & Title ╞")
        })
        .expect("step row");
    let divider_row: String = (0..band.width)
        .map(|x| buf[(x, step_y + 1)].symbol())
        .collect();
    assert!(
        divider_row.contains("╟───") && divider_row.contains("───╢"),
        "expected step divider under step strip:\n{dump}"
    );
    let div_x = (0..band.width)
        .find(|&x| buf[(x, step_y + 1)].symbol() == "╟")
        .expect("left divider cap");
    assert_eq!(buf[(div_x, step_y + 1)].fg, elev.gilt);
    assert_eq!(buf[(div_x + 1, step_y + 1)].fg, elev.border_muted);
    let check_x = (0..band.width)
        .find(|&x| buf[(x, step_y)].symbol() == "✓")
        .expect("check glyph");
    assert_eq!(buf[(check_x, step_y)].fg, palette.teal);
    let sep_x = (0..band.width)
        .find(|&x| buf[(x, step_y)].symbol() == "▸")
        .expect("gilt separator");
    assert_eq!(buf[(sep_x, step_y)].fg, elev.gilt);
    let plaque_x = (0..band.width)
        .find(|&x| buf[(x, step_y)].symbol() == "╡")
        .expect("active step plaque");
    assert_eq!(buf[(plaque_x, step_y)].bg, palette.accent);
    assert_eq!(
        buf[(plaque_x, step_y)].fg,
        crate::chrome::dialogs::contrast(&palette)
    );
    assert!(
        buf[(plaque_x + 2, step_y)]
            .modifier
            .contains(Modifier::BOLD)
    );

    let tab_hit = hits
        .cells
        .iter()
        .find(|(_, hit)| matches!(hit, super::NsHit::StepTab(Step::MachineRepo)))
        .map(|(r, _)| *r)
        .expect("completed step tab hit rect");
    assert_eq!(tab_hit.y, step_y);
    assert!(tab_hit.contains(ratatui::layout::Position::new(check_x, step_y)));

    // Rounded Herdr mode (`deco = false`) remains unchanged.
    let mut plain_buf = Buffer::empty(band);
    let _plain_hits = render_styled(&mut plain_buf, &ns, &palette, band, true, false);
    let plain_dump: String = (0..band.height)
        .map(|y| {
            (0..band.width)
                .map(|x| plain_buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(plain_dump.contains('╭'), "{plain_dump}");
    assert!(plain_dump.contains("2 ID & Title"), "{plain_dump}");
    assert!(!plain_dump.contains("✦ NEW STUDIO"), "{plain_dump}");
}

#[test]
fn a_failed_dry_run_shows_the_first_line_and_the_reason() {
    let band = Rect::new(0, 0, 80, 24);
    let mut buf = Buffer::empty(band);
    let palette = Palette::backlot();
    let mut ns = NewStudio::new();
    ns.step = super::Step::CheckStart;
    ns.dry = super::Probe::Failed(
        "--cast -: TOML parse error at line 1, column 1\n  |\n1 |\n  | ^\nmissing field `producer`"
            .into(),
    );

    render(&mut buf, &ns, &palette, band);
    let dump: String = (0..band.height)
        .map(|y| {
            (0..band.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        dump.contains("--cast -: TOML parse error"),
        "the error's first line is missing:\n{dump}"
    );
    assert!(
        dump.contains("missing field `producer`"),
        "the error's reason is missing:\n{dump}"
    );
}
