//! The update card's behaviour: which card opens for each state, the
//! keys it owns, skip, and the once-only what's-new after an upgrade.
use super::*;
use crate::chrome::update_modal::{Button, Phase};
use crate::update_state::CachedRelease;
use hc_vt::{Action, Key, Mods};

fn key(app: &mut App, key: Key) {
    app.on_input(
        input::UiEvent::Key(term_key(key, Mods::NONE, Action::Press)),
        120,
    );
}

/// A scratch directory standing in for the install dir; removed on drop.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(label: &str) -> Scratch {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "hctui-card-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn notes_for(version: &str) -> CachedRelease {
    CachedRelease {
        version: version.into(),
        date: "2026-11-16".into(),
        body: "### Added\n- **Waiting status:** show `waiting`\n".into(),
    }
}

/// An app with a newer release offered and an install dir that does
/// not exist, so no probe ever touches the real filesystem.
fn offered() -> App {
    let mut app = app();
    app.update_latest = Some("9.9.9".into());
    app.running_exe = Some(std::path::PathBuf::from("/nonexistent-hc-dir/backlot"));
    app
}

fn phase(app: &App) -> Option<Phase> {
    app.update_modal.as_ref().map(|modal| modal.phase)
}

#[tokio::test]
async fn an_offered_update_opens_the_notes_card_and_s_skips_it() {
    let mut app = offered();
    app.open_update();
    assert_eq!(phase(&app), Some(Phase::Opened));
    key(&mut app, Key::Char('s'));
    assert!(app.update_modal.is_none(), "skip closes the card");
    assert_eq!(app.update_state.skipped_version.as_deref(), Some("9.9.9"));
    assert_eq!(app.update_ready_version(), None, "the badge and dot clear");
    assert_eq!(
        app.update_latest.as_deref(),
        Some("9.9.9"),
        "still reachable"
    );
}

#[tokio::test]
async fn esc_and_q_close_the_card_without_skipping() {
    for close in [Key::Escape, Key::Char('q')] {
        let mut app = app();
        app.update_latest = Some("9.9.9".into());
        app.open_update();
        key(&mut app, close);
        assert!(app.update_modal.is_none());
        assert_eq!(app.update_state.skipped_version, None);
    }
}

#[tokio::test]
async fn with_nothing_newer_and_no_notes_a_quiet_notice_says_up_to_date() {
    let mut app = app();
    app.open_update();
    assert!(app.update_modal.is_none());
    let screen = screen_lines(&draw_to(&mut app, 80, 24)).join("\n");
    assert!(screen.contains("is up to date"), "{screen}");
}

#[tokio::test]
async fn the_scroll_keys_stay_inside_the_notes() {
    let mut app = offered();
    app.update_state.releases = vec![notes_for("9.9.9"), notes_for("9.9.8"), notes_for("9.9.7")];
    app.open_update();
    app.area = (80, 24);
    draw_to(&mut app, 80, 24);
    let max = app.update_modal.as_ref().unwrap().render.max_scroll;
    assert!(max > 0, "three releases overflow the card");
    key(&mut app, Key::Char('G'));
    assert_eq!(app.update_modal.as_ref().unwrap().scroll, max);
    key(&mut app, Key::Char('j'));
    assert_eq!(
        app.update_modal.as_ref().unwrap().scroll,
        max,
        "no overscroll"
    );
    key(&mut app, Key::Char('g'));
    assert_eq!(app.update_modal.as_ref().unwrap().scroll, 0);
    key(&mut app, Key::ArrowDown);
    assert_eq!(app.update_modal.as_ref().unwrap().scroll, 1);
}

#[tokio::test]
async fn the_card_owns_the_keys_meant_for_the_pane() {
    let mut app = offered();
    app.open_update();
    key(&mut app, Key::Enter);
    assert!(app.update_modal.is_some(), "enter is not a pane key here");
    assert_eq!(phase(&app), Some(Phase::Opened));
}

#[tokio::test]
async fn a_writable_install_dir_offers_update_and_a_read_only_one_hides_it() {
    let writable = Scratch::new("writable");
    let mut app = offered();
    app.running_exe = Some(writable.0.join("backlot"));
    app.open_update();
    let modal = app.update_modal.as_ref().unwrap();
    assert!(modal.writable);
    assert_eq!(modal.chip, "backlot --upgrade");
    draw_to(&mut app, 80, 24);
    let buttons = &app.update_modal.as_ref().unwrap().render.buttons;
    assert!(buttons.iter().any(|(_, kind)| *kind == Button::Update));

    let mut app = offered();
    app.running_exe = Some(std::path::PathBuf::from("/nonexistent-hc-dir/backlot"));
    app.open_update();
    let modal = app.update_modal.as_ref().unwrap();
    assert!(!modal.writable);
    assert_eq!(
        modal.chip,
        "BACKLOT_INSTALL_DIR=/nonexistent-hc-dir backlot --upgrade"
    );
    draw_to(&mut app, 80, 24);
    let buttons = &app.update_modal.as_ref().unwrap().render.buttons;
    assert!(
        buttons.iter().all(|(_, kind)| *kind != Button::Update),
        "no in-client update on a read-only directory"
    );
}

#[tokio::test]
async fn a_binary_in_a_home_bin_targets_that_directory_and_quotes_it() {
    let mut app = offered();
    app.running_exe = Some(std::path::PathBuf::from("/home/user/bin/backlot"));
    app.open_update();
    let modal = app.update_modal.as_ref().unwrap();
    assert_eq!(modal.install_text, "/home/user/bin");
    assert_eq!(
        modal.chip,
        "BACKLOT_INSTALL_DIR=/home/user/bin backlot --upgrade"
    );

    let mut app = offered();
    app.running_exe = Some(std::path::PathBuf::from("/home/user/it's/backlot"));
    app.open_update();
    assert_eq!(
        app.update_modal.as_ref().unwrap().chip,
        "BACKLOT_INSTALL_DIR='/home/user/it'\\''s' backlot --upgrade"
    );
}

#[tokio::test]
async fn after_an_upgrade_the_what_s_new_card_opens_once() {
    let mut app = app();
    app.update_state.releases = vec![notes_for(env!("CARGO_PKG_VERSION"))];
    app.maybe_open_whats_new();
    assert_eq!(phase(&app), Some(Phase::WhatsNew));
    key(&mut app, Key::Escape);
    assert!(app.update_modal.is_none());
    assert_eq!(
        app.update_state.show_on_startup_version.as_deref(),
        Some(env!("CARGO_PKG_VERSION"))
    );
    app.maybe_open_whats_new();
    assert!(app.update_modal.is_none(), "shown once per version");
}

#[tokio::test]
async fn the_what_s_new_card_never_starts_an_install() {
    let mut app = app();
    app.update_state.releases = vec![notes_for(env!("CARGO_PKG_VERSION"))];
    app.open_update();
    assert_eq!(phase(&app), Some(Phase::WhatsNew));
    key(&mut app, Key::Char('u'));
    key(&mut app, Key::Char('s'));
    assert_eq!(
        phase(&app),
        Some(Phase::WhatsNew),
        "read-only: u and s do nothing"
    );
    assert!(app.install.is_none(), "no installer run");
    key(&mut app, Key::Enter);
    assert!(
        app.update_modal.is_none(),
        "enter closes the read-only card"
    );
    assert!(app.install.is_none());
}

fn press(app: &mut App, key: Key, mods: Mods) {
    app.on_input(input::UiEvent::Key(term_key(key, mods, Action::Press)), 120);
}

/// The default prefix (`ctrl+b`) then one key.
fn prefix_then(app: &mut App, key: Key) {
    press(app, Key::Char('b'), Mods::CTRL);
    press(app, key, Mods::NONE);
}

fn click(app: &mut App, point: (u16, u16)) {
    app.on_input(
        input::UiEvent::Mouse(input::MouseInput {
            kind: crate::app::input::MouseEventKind::Down(crate::app::input::MouseButton::Left),
            col: point.0,
            row: point.1,
            mods: Mods::NONE,
            sgr_position: None,
        }),
        120,
    );
}

fn centre(rect: ratatui::layout::Rect) -> (u16, u16) {
    (rect.x + rect.width / 2, rect.y)
}

#[tokio::test]
async fn prefix_u_opens_the_card_for_an_offered_update() {
    let mut app = offered();
    prefix_then(&mut app, Key::Char('u'));
    assert_eq!(phase(&app), Some(Phase::Opened));
}

#[tokio::test]
async fn a_user_binding_on_prefix_u_wins_over_the_built_in_update() {
    let config: crate::config::Config = toml::from_str("[keys]\nnew_tab = 'prefix+u'\n").unwrap();
    let mut app = offered();
    app.live = config.live_keybinds();
    prefix_then(&mut app, Key::Char('u'));
    assert!(app.update_modal.is_none(), "the user's chord, not the card");
}

#[tokio::test]
async fn a_rebound_update_leaves_prefix_u_free() {
    let config: crate::config::Config = toml::from_str("[keys]\nupdate = 'prefix+x'\n").unwrap();
    let mut app = offered();
    app.live = config.live_keybinds();
    prefix_then(&mut app, Key::Char('u'));
    assert!(app.update_modal.is_none());
    prefix_then(&mut app, Key::Char('x'));
    assert_eq!(phase(&app), Some(Phase::Opened));
}

#[tokio::test]
async fn clicking_the_startup_toast_opens_the_card() {
    let mut app = offered();
    app.maybe_toast_update();
    let screen = draw_to(&mut app, 80, 24);
    assert!(
        screen_lines(&screen)
            .join("\n")
            .contains("Update available")
    );
    let rect = app.update_toast_hit;
    assert!(rect.width > 0, "the toast card has a hit rectangle");
    click(&mut app, centre(rect));
    assert_eq!(phase(&app), Some(Phase::Opened));
}

#[tokio::test]
async fn clicking_the_version_badge_hint_opens_the_card() {
    let mut app = offered();
    draw_to(&mut app, 80, 24);
    let badge = app.hit_map.update_badge;
    assert!(!badge.is_empty(), "an offered update shows the hint");
    click(&mut app, centre(badge));
    assert_eq!(phase(&app), Some(Phase::Opened));
}
