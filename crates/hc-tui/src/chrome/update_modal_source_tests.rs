//! The update card on a package-managed or dev-build binary at the two
//! design sizes: the channel's own callout and command, no `↵ update`,
//! and never the `hc-tui --upgrade` line (issue #44).

use super::tests::{card, draw, releases};
use super::{Body, ModalView, Phase};
use crate::chrome::release_notes::{Callout, NotesState, Stream};
use crate::install_source::InstallSource;

fn managed_sources() -> [InstallSource; 5] {
    [
        InstallSource::Mise {
            tool: "github:followcat/hc-tui".into(),
        },
        InstallSource::Homebrew,
        InstallSource::Nix,
        InstallSource::Cargo,
        InstallSource::DevBuild,
    ]
}

fn check(source: &InstallSource, width: u16, height: u16) {
    let rel = releases();
    let phrase = source.card_phrase().expect("a managed source has a phrase");
    let chip = source.card_chip().unwrap_or_default();
    let view = ModalView {
        phase: Phase::Opened,
        installed: "0.1.11",
        latest: "0.1.13",
        writable: false,
        body: Body::Notes(Stream {
            releases: &rel,
            installed: "0.1.11",
            latest: "0.1.13",
            notes: NotesState::Loaded,
            callout: Some(Callout {
                writable: false,
                install_dir: "",
                chip: &chip,
                managed: Some(&phrase),
            }),
        }),
        scroll: 0,
    };
    let (buf, _) = draw(width, height, &view);
    let text = card(&buf).join("\n");
    let case = format!("{source:?} at {width}x{height}");
    assert!(text.contains(&phrase), "{case}: {text}");
    if !chip.is_empty() {
        assert!(text.contains(&chip), "{case}: {text}");
    }
    assert!(
        text.contains("s skip") && text.contains("esc close"),
        "{case}: {text}"
    );
    assert!(!text.contains("↵ update"), "{case}: {text}");
    assert!(!text.contains("hc-tui --upgrade"), "{case}: {text}");
}

#[test]
fn managed_cards_name_their_channel_at_80x24() {
    for source in managed_sources() {
        check(&source, 80, 24);
    }
}

#[test]
fn managed_cards_name_their_channel_at_60x24() {
    for source in managed_sources() {
        check(&source, 60, 24);
    }
}
