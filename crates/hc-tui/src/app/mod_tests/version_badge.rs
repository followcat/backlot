//! The version badge in the top-left chrome: the running version
//! always, an update hint once the check reports a newer release —
//! in the sidebar header on wide layouts and in the mobile header's
//! right slot when the status keeps a readable width.
use super::*;

fn version() -> String {
    format!("v{}", env!("CARGO_PKG_VERSION"))
}

/// The running version's next patch — `0.1.10` → `0.1.11`: the hint
/// a real user of this build would see once a release lands.
fn next_patch() -> String {
    let current = env!("CARGO_PKG_VERSION");
    let (head, patch) = current.rsplit_once('.').expect("semver");
    format!(
        "{head}.{}",
        patch.parse::<u64>().expect("numeric patch") + 1
    )
}

fn header(b: &TestBackend) -> String {
    screen_lines(b)[0].clone()
}

fn drawn(app: &mut App, w: u16, h: u16) -> TestBackend {
    app.area = (w, h);
    draw_to(app, w, h)
}

#[tokio::test]
async fn the_running_version_sits_in_the_sidebar_header() {
    let mut app = app_with(&["t1"]);
    let backend = drawn(&mut app, 80, 24);
    let header = header(&backend);
    assert!(header.contains("spaces"), "the label stays: {header:?}");
    assert!(
        header.contains(&version()),
        "the running version is missing: {header:?}"
    );
    assert!(!header.contains('↑'), "no update is known: {header:?}");
    assert!(app.hit_map.update_badge.is_empty());
}

#[tokio::test]
async fn the_expanded_machines_header_shows_the_badge() {
    use super::endpoint_nav::{app_multi, spaced_term};
    use crate::sidebar::fixture::machine;
    let mut app = app_multi(
        vec![spaced_term("t1", "one"), spaced_term("host-a/t2", "two")],
        vec![machine("host-a", "connected")],
    );
    app.update_latest = Some("9.9.9".into());
    app.sidebar_width = 30;
    let backend = drawn(&mut app, 80, 24);
    let header = header(&backend);
    assert!(header.contains("machines"), "the label stays: {header:?}");
    assert!(
        header.contains(&version()),
        "the running version is missing: {header:?}"
    );
    assert!(header.contains("↑v9.9.9"), "the hint shows: {header:?}");
}

#[tokio::test]
async fn a_known_update_adds_an_accent_hint() {
    let mut app = app_with(&["t1"]);
    app.update_latest = Some(next_patch());
    let backend = drawn(&mut app, 80, 24);
    let header = header(&backend);
    let hint_x = header.find('↑').expect("the update hint is missing") as u16;
    let ver_x = header.find(&version()).expect("the version is missing") as u16;
    let buf = backend.buffer();
    assert_eq!(buf[(hint_x, 0)].fg, app.shell.palette.accent);
    assert_eq!(buf[(ver_x, 0)].fg, app.shell.palette.overlay0);
    // The click hit covers the hint, not the inert version text.
    let hit = app.hit_map.update_badge;
    assert!(hit.x <= hint_x && hint_x < hit.x + hit.width && hit.y == 0);
    assert!(!(hit.x <= ver_x && ver_x < hit.x + hit.width));
}

#[tokio::test]
async fn a_two_digit_pair_still_shows_the_hint() {
    // v0.1.10 → v0.1.11-class pairs: both sides carry two-digit
    // parts and the badge must still fit at the default width.
    let mut app = app_with(&["t1"]);
    app.update_latest = Some("0.11.0".into());
    let backend = drawn(&mut app, 80, 24);
    let header = header(&backend);
    assert!(header.contains(&version()), "the version: {header:?}");
    assert!(header.contains('↑'), "the hint dropped: {header:?}");
    assert!(!app.hit_map.update_badge.is_empty());
}

#[tokio::test]
async fn a_narrow_sidebar_drops_the_hint_first() {
    let mut app = app_with(&["t1"]);
    app.sidebar_width = 18;
    app.update_latest = Some("9.9.9".into());
    let backend = drawn(&mut app, 80, 24);
    let header = header(&backend);
    assert!(header.contains(&version()), "the version stays: {header:?}");
    assert!(!header.contains('↑'), "the hint drops first: {header:?}");
    // Nothing wrote over the divider column.
    assert_eq!(backend.buffer()[(17, 0)].symbol(), "│");
    assert!(app.hit_map.update_badge.is_empty());
}

#[tokio::test]
async fn the_mobile_header_shows_the_badge_when_it_fits() {
    let mut app = app_with(&["t1"]);
    app.update_latest = Some("9.9.9".into());
    let backend = drawn(&mut app, 50, 16);
    let header = header(&backend);
    assert!(header.contains(&version()), "mobile header: {header:?}");
    assert!(header.contains("↑v9.9.9"), "mobile hint: {header:?}");
}

#[tokio::test]
async fn a_tight_mobile_header_skips_the_badge() {
    let mut app = app_with(&["t1"]);
    app.update_latest = Some("9.9.9".into());
    let backend = drawn(&mut app, 20, 10);
    let header = header(&backend);
    assert!(
        !header.contains(&version()),
        "no clean place — nothing squeezed: {header:?}"
    );
    assert!(app.hit_map.update_badge.is_empty());
}

#[tokio::test]
async fn the_report_stores_the_latest_for_the_badge() {
    let mut app = app();
    app.on_update_check(Some(crate::update_check::CheckReport {
        latest: Some("9.9.9".into()),
        state: crate::update_state::UpdateCheckState::default(),
        path: None,
    }));
    assert_eq!(app.update_latest.as_deref(), Some("9.9.9"));
    assert!(app.notice.is_some(), "the toast still fires");
    assert_eq!(
        app.update_state.seen_toast_version.as_deref(),
        Some("9.9.9"),
        "the toast marks the version seen once"
    );
    // A quiet follow-up report keeps the badge — only a newer release
    // rewrites it.
    app.on_update_check(None);
    assert_eq!(app.update_latest.as_deref(), Some("9.9.9"));
}

#[tokio::test]
async fn a_seen_version_does_not_toast_again() {
    let mut app = app();
    let state = crate::update_state::UpdateCheckState {
        seen_toast_version: Some("9.9.9".into()),
        ..crate::update_state::UpdateCheckState::default()
    };
    app.on_update_check(Some(crate::update_check::CheckReport {
        latest: Some("9.9.9".into()),
        state,
        path: None,
    }));
    assert!(app.notice.is_none(), "already toasted: silent");
    assert_eq!(app.update_ready_version(), Some("9.9.9"), "still a hint");
}

#[tokio::test]
async fn a_skipped_version_hides_every_passive_surface() {
    let mut app = app();
    let state = crate::update_state::UpdateCheckState {
        skipped_version: Some("9.9.9".into()),
        ..crate::update_state::UpdateCheckState::default()
    };
    app.on_update_check(Some(crate::update_check::CheckReport {
        latest: Some("9.9.9".into()),
        state,
        path: None,
    }));
    assert!(app.notice.is_none(), "skipped: no toast");
    assert_eq!(app.update_ready_version(), None, "skipped: no badge");
    assert_eq!(
        app.update_latest.as_deref(),
        Some("9.9.9"),
        "the modal can still reach the skipped release"
    );
}

#[tokio::test]
async fn a_verified_install_waiting_for_restart_shows_the_green_hint() {
    let mut app = app_with(&["t1"]);
    app.update_latest = Some(next_patch());
    app.restart_ready = app.update_latest.clone();
    let backend = drawn(&mut app, 80, 24);
    let header = header(&backend);
    assert!(
        !header.contains('↑'),
        "the restart hint replaces the update hint: {header:?}"
    );
    let hint_x = header.find('↻').expect("the restart hint is missing") as u16;
    assert_eq!(backend.buffer()[(hint_x, 0)].fg, app.shell.palette.green);
}

#[tokio::test]
async fn navigate_mode_says_restart_ready_while_an_install_waits() {
    let mut app = app_with(&["t1"]);
    app.mode = Mode::Navigate;
    app.restart_ready = Some("0.1.13".into());
    let backend = drawn(&mut app, 80, 24);
    assert!(
        screen_lines(&backend)
            .iter()
            .any(|row| row.contains("restart ready"))
    );
}
