//! Tests for the release-notes renderer.

use super::*;

fn palette() -> Palette {
    Palette::catppuccin()
}

fn text_of(line: &Line<'_>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

fn release(version: &str, date: &str, body: &str) -> CachedRelease {
    CachedRelease {
        version: version.into(),
        date: date.into(),
        body: body.into(),
    }
}

#[test]
fn bullets_hang_their_wrapped_lines_and_strip_markup() {
    let p = palette();
    let body = "- **Waiting status:** show `waiting` while a background shell outlives a turn.";
    let releases = [release("0.1.13", "2026-11-16", body)];
    let lines = stream_lines(
        &Stream {
            releases: &releases,
            installed: "0.1.11",
            latest: "0.1.13",
            notes: NotesState::Loaded,
            callout: None,
        },
        40,
        &p,
    );
    assert_eq!(text_of(&lines[0]), " v0.1.13 · 2026-11-16");
    assert_eq!(
        text_of(&lines[1]),
        " • Waiting status: show waiting while a"
    );
    assert_eq!(text_of(&lines[2]), "   background shell outlives a turn.");
    let bold = &lines[1].spans[1];
    assert_eq!(bold.content.as_ref(), "Waiting status:");
    assert_eq!(bold.style.fg, Some(p.text));
    assert!(bold.style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn inline_code_is_a_chip_without_backticks() {
    let p = palette();
    let releases = [release("0.1.13", "", "- use `backlot --upgrade` now")];
    let lines = stream_lines(
        &Stream {
            releases: &releases,
            installed: "0.1.11",
            latest: "0.1.13",
            notes: NotesState::Loaded,
            callout: None,
        },
        60,
        &p,
    );
    let chip = lines[1]
        .spans
        .iter()
        .find(|s| s.content.as_ref() == "backlot --upgrade")
        .expect("the code span is its own run");
    assert_eq!(chip.style.fg, Some(p.accent));
    assert_eq!(chip.style.bg, Some(p.surface0));
    assert_eq!(text_of(&lines[1]), " • use backlot --upgrade now");
}

#[test]
fn fenced_blocks_are_gutter_rows_filled_to_the_width() {
    let p = palette();
    let releases = [release("0.1.13", "", "```sh\nbacklot --upgrade\n```")];
    let lines = stream_lines(
        &Stream {
            releases: &releases,
            installed: "0.1.11",
            latest: "0.1.13",
            notes: NotesState::Loaded,
            callout: None,
        },
        24,
        &p,
    );
    assert_eq!(
        text_of(&lines[1]),
        format!(" ▏ backlot --upgrade{}", " ".repeat(4))
    );
    let gutter = lines[1]
        .spans
        .iter()
        .find(|span| span.content.as_ref() == "▏")
        .expect("the gutter is its own run");
    assert_eq!(gutter.style.bg, Some(p.surface1));
    assert_eq!(gutter.style.fg, Some(p.accent));
}

#[test]
fn stream_separates_releases_and_ends_with_the_baseline() {
    let p = palette();
    let releases = [
        release("0.1.13", "2026-11-16", "### Added\n- One"),
        release("0.1.12", "2026-11-10", "- Two"),
    ];
    let lines = stream_lines(
        &Stream {
            releases: &releases,
            installed: "0.1.11",
            latest: "0.1.13",
            notes: NotesState::Loaded,
            callout: None,
        },
        30,
        &p,
    );
    let texts: Vec<String> = lines.iter().map(text_of).collect();
    assert_eq!(texts[0], " v0.1.13 · 2026-11-16");
    assert_eq!(texts[1], " ADDED");
    assert_eq!(texts[2], " • One");
    assert_eq!(texts[3], "");
    assert!(texts[4].starts_with(" ────"));
    assert_eq!(texts[5], "");
    assert_eq!(texts[6], " v0.1.12 · 2026-11-10");
    assert!(texts.last().unwrap().starts_with(" ── installed v0.1.11 "));
}

#[test]
fn writable_callout_offers_the_manual_chip_above_the_notes() {
    let p = palette();
    let lines = stream_lines(
        &Stream {
            releases: &[],
            installed: "0.1.11",
            latest: "0.1.13",
            notes: NotesState::Loading,
            callout: Some(Callout {
                writable: true,
                install_dir: "/home/user/.local/bin",
                chip: "backlot --upgrade",
                managed: None,
            }),
        },
        60,
        &p,
    );
    assert_eq!(
        text_of(&lines[0]),
        " ● updates this client · programs keep running"
    );
    assert_eq!(
        text_of(&lines[1]),
        " press enter to install, or run backlot --upgrade"
    );
    assert_eq!(text_of(&lines[2]), "");
    assert_eq!(text_of(&lines[3]), " ◐ loading release notes from GitHub…");
}

#[test]
fn read_only_callout_names_the_directory() {
    let p = palette();
    let lines = stream_lines(
        &Stream {
            releases: &[],
            installed: "0.1.11",
            latest: "0.1.13",
            notes: NotesState::Offline,
            callout: Some(Callout {
                writable: false,
                install_dir: "/usr/local/bin",
                chip: "BACKLOT_INSTALL_DIR=/usr/local/bin backlot --upgrade",
                managed: None,
            }),
        },
        60,
        &p,
    );
    assert_eq!(
        text_of(&lines[0]),
        " ● cannot update in place: /usr/local/bin is not writable"
    );
    assert_eq!(
        text_of(&lines[1]),
        " upgrade from a shell: BACKLOT_INSTALL_DIR=/usr/local/bin backlot --upgrade"
    );
    assert_eq!(text_of(&lines[3]), " release notes unavailable offline");
}

/// A hostile release body — OSC, CSI and C1 — is cleaned at parse so
/// the styled stream carries no control character into the buffer.
#[test]
fn a_hostile_body_renders_without_control_characters() {
    let json = r####"[
        {"tag_name": "v9.9.9", "draft": false, "prerelease": false,
         "published_at": "2026-01-01T00:00:00Z",
         "body": "- osc \u001b]52;c;AAAA\u0007 end\n- csi \u001b[2J end\n- c1 \u009b end"}
    ]"####;
    let releases = crate::update_notes::parse_releases(json, "0.1.11").unwrap();
    let lines = stream_lines(
        &Stream {
            releases: &releases,
            installed: "0.1.11",
            latest: "9.9.9",
            notes: NotesState::Loaded,
            callout: None,
        },
        60,
        &palette(),
    );
    for line in &lines {
        for span in &line.spans {
            assert!(
                !span.content.chars().any(char::is_control),
                "{:?}",
                span.content
            );
        }
    }
    let text: String = lines
        .iter()
        .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
        .collect();
    assert!(!text.contains("52"), "{text}");
    assert!(!text.contains("[2J"), "{text}");
}
