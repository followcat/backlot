//! U1/R3 `Alt+arrow` hardening, part 2: the unseated-studio focus
//! queue (R3-M4 — a studio whose stage is still seating its producer
//! queues the landing instead of failing with `has no live
//! terminal`), the `Ctrl+B ?` launcher's studio rows (R3-N6), the
//! `[keys] ""` hand-back to herdr, zero-studio fall-through and
//! `--plain`'s zero interception. Neutral fixtures only.

use super::studio::{alt, doc_key, two_studio_app};
use super::*;
use crate::help_launcher::Launch;
use crate::studio::StudioKey;
use hc_proto::watch::Document;

/// The fixture's document plus a new `beta-api` studio inserted
/// right after `acme-shop`; `producer_terminal` is `None` while its
/// producer is still being seated by the stage.
fn doc_with_beta_api(producer_terminal: Option<&str>) -> serde_json::Value {
    let producer = match producer_terminal {
        Some(terminal) => serde_json::json!({
            "role": "producer", "name": "producer", "terminal": terminal,
            "state": "live", "work": "working"
        }),
        None => serde_json::json!({
            "role": "producer", "name": "producer",
            "state": "live", "work": "working"
        }),
    };
    serde_json::json!({
        "schema": "backlot/1", "updated_ms": 3,
        "studios": [
            {
                "id": "acme-shop", "title": "Acme shop", "state": "live",
                "requirement_version": 1, "mode": "standard",
                "open_reviews": 0, "waiting": 0,
                "roles": [
                    { "role": "producer", "name": "producer", "terminal": "t1",
                      "state": "live", "work": "working" },
                    { "role": "actor-alice", "name": "alice", "terminal": "t4",
                      "state": "live", "work": "working", "task": "task-1" }
                ]
            },
            {
                "id": "beta-api", "title": "Beta api", "state": "live",
                "requirement_version": 1, "mode": "standard",
                "open_reviews": 0, "waiting": 0,
                "roles": [ producer ]
            },
            {
                "id": "night-owl", "title": "Night owl", "state": "live",
                "requirement_version": 1, "mode": "standard",
                "open_reviews": 0, "waiting": 0,
                "roles": [
                    { "role": "producer", "name": "owl-producer", "terminal": "t7",
                      "state": "live", "work": "waiting" }
                ]
            }
        ]
    })
}

fn publish(app: &mut App, value: serde_json::Value) {
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
}

// ---------- R3-M4: the unseated-studio focus queue ----------

#[tokio::test]
async fn an_unseated_studio_on_a_live_stage_queues_its_focus() {
    let mut app = two_studio_app();
    publish(&mut app, doc_with_beta_api(None));
    // Alt+Down selects beta-api's studio row; Alt+Right tries to enter
    // its cast and queues focus on the live stage.
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.studio.current, Some("beta-api".into()));
    alt(&mut app, hc_vt::Key::ArrowRight);
    assert_eq!(
        app.ns_pending_focus,
        Some(StudioKey::local("beta-api")),
        "the landing is queued for the next publish"
    );
    assert!(app.notice.is_none(), "no has-no-live-terminal toast");
    // The publish that seats the producer lands the focus.
    let mut seat = crate::sidebar::fixture::term("t60");
    seat.labels.insert("herdr.space".into(), "beta-api".into());
    seat.labels.insert("herdr.tab".into(), "producer".into());
    let terms: Vec<_> = app
        .view
        .inventory
        .terminals()
        .values()
        .cloned()
        .chain([seat])
        .collect();
    app.view.inventory_mut().replace(3, terms);
    publish(&mut app, doc_with_beta_api(Some("t60")));
    assert_eq!(app.ns_pending_focus, None);
    assert_eq!(app.focused.as_deref(), Some("t60"));
    assert_eq!(app.studio.current, Some("beta-api".into()));
}

#[tokio::test]
async fn a_queued_focus_leaves_the_overlays_when_it_lands() {
    let mut app = two_studio_app();
    app.area = (120, 24);
    publish(&mut app, doc_with_beta_api(None));
    alt(&mut app, hc_vt::Key::ArrowDown);
    alt(&mut app, hc_vt::Key::ArrowRight);
    assert_eq!(app.ns_pending_focus, Some(StudioKey::local("beta-api")));
    // The seat arrives while the Call Sheet is up — the landing
    // still hands the keys to the new pane.
    app.hollywood.mode = crate::hollywood::LayerMode::Expanded;
    let mut seat = crate::sidebar::fixture::term("t60");
    seat.labels.insert("herdr.space".into(), "beta-api".into());
    seat.labels.insert("herdr.tab".into(), "producer".into());
    let terms: Vec<_> = app
        .view
        .inventory
        .terminals()
        .values()
        .cloned()
        .chain([seat])
        .collect();
    app.view.inventory_mut().replace(3, terms);
    publish(&mut app, doc_with_beta_api(Some("t60")));
    assert_eq!(app.focused.as_deref(), Some("t60"));
    assert!(!app.hollywood.expanded(), "the sheet let go");
    let before = app.pane.input_backlog().0;
    app.on_input(
        crate::app::input::UiEvent::Key(term_key(
            hc_vt::Key::Char('a'),
            hc_vt::Mods::NONE,
            hc_vt::Action::Press,
        )),
        app.area.0,
    );
    assert!(app.pane.input_backlog().0 > before, "typing reaches t60");
}

#[tokio::test]
async fn a_dead_studio_still_reports_no_live_terminal() {
    // ghost's only role is closed and it has no shell — entering its roles
    // from its studio focus reports no live terminal.
    let mut app = two_studio_app();
    alt(&mut app, hc_vt::Key::ArrowDown);
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.studio.current, Some("ghost".into()));
    alt(&mut app, hc_vt::Key::ArrowRight);
    assert_eq!(app.ns_pending_focus, None, "nothing to seat — no queue");
    assert!(app.notice.is_some(), "the notice says so");
}

// ---------- R3-N6: the `?` launcher's studio rows ----------

#[tokio::test]
async fn the_launcher_lists_the_four_studio_keys_near_the_top() {
    // `next_agent = alt+down` in herdr — the shadowed action must
    // name itself on the `next_studio` row.
    let config: crate::config::Config =
        toml::from_str("[keys]\nnext_agent = 'alt+down'\n").unwrap();
    let mut app = two_studio_app();
    app.live = config.live_keybinds();
    let groups = app.help_groups();
    let studio = groups
        .iter()
        .position(|g| g.title == "studio & task actions")
        .expect("the studio actions group");
    assert!(studio <= 3, "the studio group sits near the top: {studio}");
    let rows = &groups[studio].rows;
    for (nav, hint) in [
        ("previous studio", "alt+up"),
        ("next studio", "alt+down"),
        ("previous role", "alt+left"),
        ("next role", "alt+right"),
    ] {
        let row = rows
            .iter()
            .find(|r| r.label.starts_with(nav))
            .unwrap_or_else(|| panic!("{nav} row missing: {rows:?}"));
        assert_eq!(row.hint, hint, "{nav} hint");
    }
    let next = rows
        .iter()
        .find(|r| r.label.starts_with("next studio"))
        .unwrap();
    assert!(
        next.label.contains("shadows herdr: next_agent"),
        "the shadow note names the herdr action: {}",
        next.label
    );
    // The same four keys are not duplicated in the reference tail.
    assert!(
        !groups.iter().any(|g| g.title == "studio"),
        "no second studio section near the bottom"
    );
}

#[tokio::test]
async fn enter_on_a_studio_key_row_runs_the_nav_and_closes() {
    let mut app = two_studio_app();
    app.help = true;
    let selected = app
        .help_rows()
        .iter()
        .position(|r| r.label.starts_with("next studio"))
        .expect("the next studio row");
    app.help_state.selected = selected;
    app.help_launcher_activate();
    assert!(!app.help, "the launcher closed");
    assert_eq!(app.studio.current, Some("night-owl".into()));
    assert_eq!(
        app.studio.focus,
        Some(crate::studio::StudioFocus::Studio(StudioKey::local(
            "night-owl"
        )))
    );
    assert_eq!(app.focused.as_deref(), Some("t1"));
}

#[tokio::test]
async fn an_unset_studio_key_lists_unset_and_stays_inert() {
    // `next_studio = ""` in backlot's config — the row shows `unset`
    // and Enter is a plain close, not a nav.
    let dir = crate::test_support::TestDir::new("studio-unset-row");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "[keys]\nnext_studio = \"\"\n").unwrap();
    let (config, notice) = crate::panel::load(&path);
    let mut app = two_studio_app();
    app.set_panel_config(config, notice, &path);
    let rows = app.help_groups();
    let row = rows
        .iter()
        .flat_map(|g| g.rows.iter())
        .find(|r| r.label.starts_with("next studio"))
        .expect("the row still lists");
    assert_eq!(row.hint, "unset");
    assert!(matches!(row.launch, Launch::Close), "unset is inert");
}

// ---------- hand-back to herdr ----------

/// herdr with `next_tab` on `alt+down` — the binding the studio key
/// shadows while bound, and releases while `""`.
fn live_with_alt_next_tab() -> crate::config::LiveKeybindConfig {
    let config: crate::config::Config = toml::from_str("[keys]\nnext_tab = 'alt+down'\n").unwrap();
    config.live_keybinds()
}

/// Two tabs in a user workspace on top of the studio fixture —
/// `next_tab`'s observable target.
fn notes_workspace(app: &mut App) {
    let mut t20 = crate::sidebar::fixture::term("t20");
    t20.labels.insert("herdr.space".into(), "notes".into());
    t20.labels.insert("herdr.tab".into(), "tab-1".into());
    let mut t21 = crate::sidebar::fixture::term("t21");
    t21.labels.insert("herdr.space".into(), "notes".into());
    t21.labels.insert("herdr.tab".into(), "tab-2".into());
    let terms: Vec<_> = app
        .view
        .inventory
        .terminals()
        .values()
        .cloned()
        .chain([t20, t21])
        .collect();
    app.view.inventory_mut().replace(2, terms);
    app.reconcile_published_view();
}

#[tokio::test]
async fn an_empty_keys_entry_hands_the_key_back_to_herdr() {
    let dir = crate::test_support::TestDir::new("studio-handback");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "[keys]\nnext_studio = \"\"\n").unwrap();
    let (config, notice) = crate::panel::load(&path);
    let mut app = two_studio_app();
    app.set_panel_config(config, notice, &path);
    app.live = live_with_alt_next_tab();
    notes_workspace(&mut app);
    app.focus_pane("t20".into());
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(
        app.studio.current,
        Some("acme-shop".into()),
        "no studio nav ran"
    );
    assert_eq!(
        app.focused.as_deref(),
        Some("t21"),
        "herdr's next_tab got the key"
    );
}

#[tokio::test]
async fn zero_studios_hands_all_four_keys_back_to_herdr() {
    // A good document with zero studios and the page dismissed —
    // the four keys are herdr's again.
    let mut app = two_studio_app();
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: serde_json::json!({
                "schema": "backlot/1", "updated_ms": 4, "studios": []
            }),
            ..Document::default()
        }
        .into(),
    );
    app.opening.settled = true;
    app.opening.dismissed = true;
    notes_workspace(&mut app);
    app.live = live_with_alt_next_tab();
    app.focus_pane("t20".into());
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(
        app.focused.as_deref(),
        Some("t21"),
        "with no studio the key is herdr's again"
    );
}

#[tokio::test]
async fn plain_mode_never_intercepts_the_four_keys() {
    // `--plain` with the studio document present: all four keys reach
    // the program untouched — byte-identical to herdr.
    let mut app = two_studio_app();
    app.set_plain(true);
    let before = app.pane.input_backlog().0;
    for key in [
        hc_vt::Key::ArrowUp,
        hc_vt::Key::ArrowDown,
        hc_vt::Key::ArrowLeft,
        hc_vt::Key::ArrowRight,
    ] {
        alt(&mut app, key);
    }
    assert_eq!(app.focused.as_deref(), Some("t1"), "no nav ran");
    assert_eq!(
        app.pane.input_backlog().0,
        before + 4,
        "all four keys reached the program"
    );
}

#[tokio::test]
async fn plain_mode_herdr_binding_on_the_same_key_wins() {
    // Two tabs in one workspace — herdr's `next_tab` cycles them.
    let mut app = app_with_tabs(&[("t1", "space-a", "tab-1"), ("t2", "space-a", "tab-2")]);
    app.live = live_with_alt_next_tab();
    app.focus_pane("t1".into());
    alt(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(
        app.focused.as_deref(),
        Some("t2"),
        "herdr's next_tab ran — zero studio interception"
    );
}

#[test]
fn raw_events_decode_csi_and_esc_alt_arrows() {
    for (bytes, expected) in [
        (&b"\x1b[1;3A"[..], hc_vt::Key::ArrowUp),
        (&b"\x1b[1;3B"[..], hc_vt::Key::ArrowDown),
        (&b"\x1b[1;3C"[..], hc_vt::Key::ArrowRight),
        (&b"\x1b[1;3D"[..], hc_vt::Key::ArrowLeft),
        (&b"\x1b\x1b[A"[..], hc_vt::Key::ArrowUp),
        (&b"\x1b\x1b[B"[..], hc_vt::Key::ArrowDown),
        (&b"\x1b\x1b[C"[..], hc_vt::Key::ArrowRight),
        (&b"\x1b\x1b[D"[..], hc_vt::Key::ArrowLeft),
    ] {
        let decoded = crate::raw_events::extract_one_event(bytes);
        assert!(
            matches!(
                decoded,
                Some((
                    crate::raw_events::Decoded::Ui(crate::app::input::UiEvent::Key(ref k)),
                    _
                )) if k.input.key == expected && k.input.mods.contains(hc_vt::Mods::ALT)
            ),
            "bytes {bytes:?} decoded as {decoded:?}"
        );
    }
}
