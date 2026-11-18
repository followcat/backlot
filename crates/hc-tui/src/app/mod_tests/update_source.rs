//! The update card on a package-managed or dev-build binary: no
//! `↵ update`, the channel's own callout, and skip and close still
//! work (issue #44).

use super::*;
use crate::chrome::update_modal::Phase;
use crate::install_source::InstallSource;
use hc_vt::{Action, Key, Mods};

fn key(app: &mut App, key: Key) {
    app.on_input(
        input::UiEvent::Key(term_key(key, Mods::NONE, Action::Press)),
        120,
    );
}

fn phase(app: &App) -> Option<Phase> {
    app.update_modal.as_ref().map(|modal| modal.phase)
}

fn managed_app(source: InstallSource) -> App {
    let mut app = app();
    app.update_latest = Some("9.9.9".into());
    app.running_exe = Some(std::path::PathBuf::from(
        "/home/user/.local/share/mise/installs/github-followcat-backlot/0.1.6/backlot",
    ));
    app.set_install_source(source);
    app
}

#[tokio::test]
async fn a_mise_managed_card_offers_no_update_and_ignores_enter_and_u() {
    let mut app = managed_app(InstallSource::Mise {
        tool: "github:followcat/backlot".into(),
    });
    app.update_state.releases = vec![crate::update_state::CachedRelease {
        version: "9.9.9".into(),
        date: "2026-01-01".into(),
        body: "release note body here".into(),
    }];
    app.open_update();
    let modal = app.update_modal.as_ref().unwrap();
    assert!(!modal.writable, "a managed dir is never writable here");
    assert_eq!(
        modal.managed.as_deref(),
        Some("managed by mise: update it there")
    );
    assert_eq!(modal.chip, "mise upgrade --bump github:followcat/backlot");
    key(&mut app, Key::Enter);
    key(&mut app, Key::Char('u'));
    assert!(app.install.is_none(), "no installer runs");
    assert_eq!(phase(&app), Some(Phase::Opened));
    let view = crate::app::update_modal::view(
        app.update_modal.as_ref().unwrap(),
        &app.update_state,
        app.update_latest.as_deref(),
        false,
        app.install.as_ref(),
    );
    let crate::chrome::update_modal::Body::Notes(stream) = view.body else {
        panic!("the release notes still show")
    };
    assert!(stream.releases.iter().any(|r| r.version == "9.9.9"));
}

#[tokio::test]
async fn a_dev_build_card_skips_and_closes_but_never_installs() {
    let mut app = managed_app(InstallSource::DevBuild);
    app.open_update();
    let modal = app.update_modal.as_ref().unwrap();
    assert!(!modal.writable);
    assert_eq!(
        modal.managed.as_deref(),
        Some("dev build: rebuild it from your checkout")
    );
    assert_eq!(modal.chip, "", "a dev build names no command");
    key(&mut app, Key::Char('u'));
    assert!(app.install.is_none());
    key(&mut app, Key::Char('s'));
    assert!(app.update_modal.is_none(), "skip closes the card");
    assert_eq!(app.update_state.skipped_version.as_deref(), Some("9.9.9"));

    let mut app = managed_app(InstallSource::DevBuild);
    app.open_update();
    key(&mut app, Key::Escape);
    assert!(app.update_modal.is_none());
    assert_eq!(app.update_state.skipped_version, None, "esc just closes");
}
