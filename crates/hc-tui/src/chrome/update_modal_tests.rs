//! Render tests for the update card: the 80x24 and 60x24 `opened`
//! cards are compared row by row with the design mockups
//! (`design/update-card/mockups/opened-*.txt`, card region only).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::{Body, ModalRender, ModalView, Phase, render};
use crate::chrome::dialogs::popup;
use crate::chrome::palette::Palette;
use crate::chrome::release_notes::{Callout, NotesState, Stream};
use crate::chrome::update_status::{Status, Step};
use crate::update_state::CachedRelease;

const OPENED_80: [&str; 22] = [
    "┌──────────────────────────────────────────────────────────────────────────┐",
    "│ v0.1.13                                                installed v0.1.11 │",
    "│ update ready                                                             │",
    "│                                                                          │",
    "│ ● updates this client · programs keep running                           ▐│",
    "│ press enter to install, or run backlot --upgrade                        ▐│",
    "│                                                                         ▐│",
    "│ v0.1.13 · 2026-11-16                                                    ▐│",
    "│ ADDED                                                                   ▐│",
    "│ • Waiting status: show ◌ waiting while a background -c shell outlives   ▐│",
    "│   an agent's turn.                                                      ▐│",
    "│ FIXED                                                                   ▐│",
    "│ • Update hint: keep ↑v… visible at the default 26-column sidebar width. ▕│",
    "│ • Pane output: pressing a key resets scrollback to the tail, not row 0. ▕│",
    "│ • Attach: release call registrations when an attach closes early.       ▕│",
    "│                                                                         ▕│",
    "│ ─────────────────────────────────────────────────────────────────────── ▕│",
    "│                                                                          │",
    "│ scroll j/k ↑↓ pgup pgdn                                                  │",
    "│                                                                          │",
    "│                     ↵ update    s skip    esc close                      │",
    "└──────────────────────────────────────────────────────────────────────────┘",
];

const OPENED_60: [&str; 22] = [
    "┌──────────────────────────────────────────────────────┐",
    "│ v0.1.13                            installed v0.1.11 │",
    "│ update ready                                         │",
    "│                                                      │",
    "│ ● updates this client · programs keep running       ▐│",
    "│ press enter to install, or run backlot --upgrade    ▐│",
    "│                                                     ▐│",
    "│ v0.1.13 · 2026-11-16                                ▐│",
    "│ ADDED                                               ▐│",
    "│ • Waiting status: show ◌ waiting while a            ▐│",
    "│   background -c shell outlives a turn.              ▐│",
    "│ FIXED                                               ▕│",
    "│ • Update hint: keep ↑v… at default width.           ▕│",
    "│ • Pane output: key press resets viewport to         ▕│",
    "│   the tail, never row 0.                            ▕│",
    "│ • Attach: release early-closed call slots.          ▕│",
    "│                                                     ▕│",
    "│                                                      │",
    "│ scroll j/k ↑↓ pgup pgdn                              │",
    "│                                                      │",
    "│           ↵ update    s skip    esc close            │",
    "└──────────────────────────────────────────────────────┘",
];

/// The mockup's release notes: 0.1.13 with its two sections, then 0.1.12.
pub(super) fn releases() -> Vec<CachedRelease> {
    vec![
        CachedRelease {
            version: "0.1.13".into(),
            date: "2026-11-16".into(),
            body: "### Added\n- **Waiting status:** show `◌ waiting` while a background `-c` shell outlives an agent's turn.\n### Fixed\n- **Update hint:** keep `↑v…` visible at the default 26-column sidebar width.\n- **Pane output:** pressing a key resets scrollback to the tail, not row 0.\n- **Attach:** release call registrations when an attach closes early.\n".into(),
        },
        CachedRelease {
            version: "0.1.12".into(),
            date: "2026-11-10".into(),
            body: "### Added\n- **Sidebar:** default agent sort to `priority`.\n### Fixed\n- **Notes:** keep the trailing blank row.\n- **Tabs:** keep the active tab visible.\n".into(),
        },
    ]
}

fn opened(releases: &[CachedRelease]) -> ModalView<'_> {
    ModalView {
        phase: Phase::Opened,
        installed: "0.1.11",
        latest: "0.1.13",
        writable: true,
        body: Body::Notes(Stream {
            releases,
            installed: "0.1.11",
            latest: "0.1.13",
            notes: NotesState::Loaded,
            callout: Some(Callout {
                writable: true,
                install_dir: "/home/user/.local/bin",
                chip: "backlot --upgrade",
                managed: None,
            }),
        }),
        scroll: 0,
    }
}

/// The card's rectangle inside a frame of the buffer's size.
fn outer_of(buf: &Buffer) -> Rect {
    popup(buf.area, 80, 24).expect("the card fits this frame")
}

/// The card's rows as the terminal shows them.
pub(super) fn card(buf: &Buffer) -> Vec<String> {
    let outer = outer_of(buf);
    (outer.y..outer.bottom())
        .map(|y| {
            (outer.x..outer.right())
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}

pub(super) fn draw(width: u16, height: u16, view: &ModalView<'_>) -> (Buffer, ModalRender) {
    let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
    let rendered = render(&mut buf, view, &Palette::catppuccin()).expect("the card fits");
    (buf, rendered)
}

#[test]
fn opened_at_80x24_matches_the_mockup_card() {
    let releases = releases();
    let (buf, _) = draw(80, 24, &opened(&releases));
    assert_eq!(outer_of(&buf), Rect::new(2, 1, 76, 22));
    assert_eq!(card(&buf), OPENED_80);
}

/// At 60x24 the mockup's body copy is shorter and wrapped narrower than
/// the 80x24 copy, so rows 9..=16 (the release body) are not compared.
/// Header, callout, footer, scrollbar and buttons must match exactly.
#[test]
fn opened_at_60x24_matches_the_mockup_card_outside_the_body() {
    let releases = releases();
    let (buf, rendered) = draw(60, 24, &opened(&releases));
    assert_eq!(outer_of(&buf), Rect::new(2, 1, 56, 22));
    let rows = card(&buf);
    for row in (0..22).filter(|row| !(9..=16).contains(row)) {
        assert_eq!(rows[row], OPENED_60[row], "card row {row}");
    }
    assert!(rendered.track.is_some(), "the scrollbar column stays");
}

#[test]
fn the_scrollbar_and_the_scroll_hint_appear_only_when_the_notes_overflow() {
    let releases = releases();
    let (buf, rendered) = draw(80, 24, &opened(&releases));
    assert!(rendered.track.is_some(), "the notes overflow 13 rows");
    assert!(rendered.max_scroll > 0);
    let short = [releases[1].clone()];
    let (buf_short, rendered_short) = draw(80, 24, &opened(&short));
    assert_eq!(rendered_short.track, None);
    assert!(
        !card(&buf_short)
            .iter()
            .any(|row| row.contains("scroll j/k")),
        "no footer hint when nothing scrolls"
    );
    assert!(card(&buf)[18].contains("scroll j/k ↑↓ pgup pgdn"));
}

#[test]
fn the_scroll_offset_moves_the_body_and_clamps_to_the_end() {
    let releases = releases();
    let mut view = opened(&releases);
    view.scroll = 500;
    let (buf, rendered) = draw(80, 24, &view);
    assert_eq!(rendered.max_scroll, 9, "22 stream rows less 13 visible");
    let body = card(&buf);
    assert!(
        body[16].contains("installed v0.1.11"),
        "the clamp shows the end of the stream: {:?}",
        body[16]
    );
    assert!(
        body[1].contains("installed v0.1.11"),
        "the header stays put"
    );
}

#[test]
fn a_card_too_small_for_its_guard_draws_only_the_panel() {
    let releases = releases();
    for (width, height) in [(1, 1), (10, 4), (19, 9), (24, 10)] {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
        let rendered = render(&mut buf, &opened(&releases), &Palette::catppuccin());
        if let Some(rendered) = rendered {
            assert!(rendered.buttons.is_empty(), "no buttons below the guard");
        }
    }
}

#[test]
fn the_what_s_new_card_has_only_the_header_close_button() {
    let releases = releases();
    let view = ModalView {
        phase: Phase::WhatsNew,
        installed: "0.1.13",
        latest: "0.1.13",
        writable: true,
        body: Body::Notes(Stream {
            releases: &releases[..1],
            installed: "0.1.13",
            latest: "0.1.13",
            notes: NotesState::Loaded,
            callout: None,
        }),
        scroll: 0,
    };
    let (buf, rendered) = draw(80, 24, &view);
    let kinds: Vec<_> = rendered.buttons.iter().map(|(_, kind)| *kind).collect();
    assert_eq!(kinds, [super::Button::Close]);
    let header = card(&buf)[1].clone();
    assert!(
        header.contains("v0.1.13") && header.contains("esc close"),
        "{header:?}"
    );
    assert!(card(&buf)[2].contains("what's new in this release"));
}

const UPDATING_80: [&str; 22] = [
    "┌──────────────────────────────────────────────────────────────────────────┐",
    "│ v0.1.13                                                installed v0.1.11 │",
    "│ updates this client · programs keep running                              │",
    "│                                                                          │",
    "│ ◐ running official installer…                                       0:04 │",
    "│ · check new binary (backlot --version)                                   │",
    "│                                                                          │",
    "│ OUTPUT                                                                   │",
    "│ ▏ $ curl -fsSL                                                           │",
    "│ ▏   https://github.com/followcat/backlot/releases/latest/download/install│",
    "│ ▏   .sh \\                                                                │",
    "│ ▏   | BACKLOT_VERSION=v0.1.13 BACKLOT_SKIP_CORE=1 \\                      │",
    "│ ▏     BACKLOT_INSTALL_DIR=/home/user/.local/bin sh                       │",
    "│ ▏                                                                        │",
    "│ ▏ waiting for installer output…                                          │",
    "│                                                                          │",
    "│ cancelling leaves v0.1.11 in place                                       │",
    "│                                                                          │",
    "│                                                                          │",
    "│                                                                          │",
    "│                                esc cancel                                │",
    "└──────────────────────────────────────────────────────────────────────────┘",
];

const DONE_80: [&str; 22] = [
    "┌──────────────────────────────────────────────────────────────────────────┐",
    "│ v0.1.13                                                  v0.1.11 running │",
    "│ update installed · restart to use v0.1.13                                │",
    "│                                                                          │",
    "│ ✓ official installer finished                                       0:06 │",
    "│ ✓ verified new binary: backlot 0.1.13                                    │",
    "│                                                                          │",
    "│ OUTPUT                                                                   │",
    "│ ▏ installed backlot 0.1.13 (x86_64-unknown-linux-musl)                   │",
    "│ ▏   to /home/user/.local/bin/backlot                                     │",
    "│                                                                          │",
    "│ Restarting re-execs backlot in place and reconnects to this session.     │",
    "│ Your hc core, spaces, and running programs keep running.                 │",
    "│                                                                          │",
    "│                                                                          │",
    "│                                                                          │",
    "│                                                                          │",
    "│                                                                          │",
    "│                                                                          │",
    "│                                                                          │",
    "│                          ↵ restart    esc later                          │",
    "└──────────────────────────────────────────────────────────────────────────┘",
];

const FAILED_80: [&str; 22] = [
    "┌──────────────────────────────────────────────────────────────────────────┐",
    "│ v0.1.13                                                installed v0.1.11 │",
    "│ update failed · v0.1.11 is unchanged                                     │",
    "│                                                                          │",
    "│ × installer exited with status 1                                    0:03 │",
    "│                                                                          │",
    "│ OUTPUT                                                                   │",
    "│ ▏ curl: (6) Could not resolve host: github.com                           │",
    "│ ▏ install.sh: download failed — does release v0.1.13 contain             │",
    "│ ▏   backlot-0.1.13-x86_64-unknown-linux-musl.tar.gz?                     │",
    "│                                                                          │",
    "│ Upgrade manually from a shell:                                           │",
    "│   backlot --upgrade                                                      │",
    "│                                                                          │",
    "│ or: curl -fsSL https://github.com/followcat/backlot/releases/latest/downl│",
    "│                                                                          │",
    "│                                                                          │",
    "│                                                                          │",
    "│                                                                          │",
    "│                                                                          │",
    "│                           ↵ retry    esc close                           │",
    "└──────────────────────────────────────────────────────────────────────────┘",
];

/// The installer status bodies, fed the same facts the mockups show.
fn status_view(phase: Phase, status: Status<'_>) -> ModalView<'_> {
    ModalView {
        phase,
        installed: "0.1.11",
        latest: "0.1.13",
        writable: true,
        body: Body::Status(status),
        scroll: 0,
    }
}

fn echo() -> Vec<String> {
    crate::update_run::plan(std::path::Path::new("/home/user/.local/bin"), "0.1.13").echo
}

#[test]
fn updating_at_80x24_matches_the_mockup_card() {
    let echo = echo();
    let status = Status {
        step: Step::Running,
        elapsed_secs: 4,
        spinner: 0,
        echo: &echo,
        tail: &[],
        verified: None,
        reason: None,
        installed: "0.1.11",
        manual: "backlot --upgrade",
    };
    let (buf, _) = draw(80, 24, &status_view(Phase::Updating, status));
    assert_eq!(card(&buf), UPDATING_80);
}

#[test]
fn done_at_80x24_matches_the_mockup_card() {
    let tail = [
        "installed backlot 0.1.13 (x86_64-unknown-linux-musl)".to_owned(),
        "  to /home/user/.local/bin/backlot".to_owned(),
    ];
    let status = Status {
        step: Step::Done,
        elapsed_secs: 6,
        spinner: 0,
        echo: &[],
        tail: &tail,
        verified: Some("backlot 0.1.13"),
        reason: None,
        installed: "0.1.11",
        manual: "backlot --upgrade",
    };
    let (buf, _) = draw(80, 24, &status_view(Phase::Done, status));
    assert_eq!(card(&buf), DONE_80);
}

#[test]
fn failed_at_80x24_matches_the_mockup_card() {
    let tail = [
        "curl: (6) Could not resolve host: github.com".to_owned(),
        "install.sh: download failed — does release v0.1.13 contain backlot-0.1.13-x86_64-unknown-linux-musl.tar.gz?".to_owned(),
    ];
    let status = Status {
        step: Step::Failed,
        elapsed_secs: 3,
        spinner: 0,
        echo: &[],
        tail: &tail,
        verified: None,
        reason: Some("installer exited with status 1"),
        installed: "0.1.11",
        manual: "backlot --upgrade",
    };
    let (buf, _) = draw(80, 24, &status_view(Phase::Failed, status));
    assert_eq!(card(&buf), FAILED_80);
}

#[test]
fn the_status_cards_keep_their_header_and_buttons_at_60x24() {
    let echo = echo();
    let status = Status {
        step: Step::Running,
        elapsed_secs: 4,
        spinner: 0,
        echo: &echo,
        tail: &[],
        verified: None,
        reason: None,
        installed: "0.1.11",
        manual: "backlot --upgrade",
    };
    let (buf, rendered) = draw(60, 24, &status_view(Phase::Updating, status));
    let rows = card(&buf);
    assert!(rows[1].contains("installed v0.1.11"), "{:?}", rows[1]);
    assert!(rows[2].contains("updates this client"), "{:?}", rows[2]);
    assert!(rows[20].contains("esc cancel"), "{:?}", rows[20]);
    let kinds: Vec<_> = rendered.buttons.iter().map(|(_, kind)| *kind).collect();
    assert_eq!(kinds, [super::Button::Cancel]);
}
