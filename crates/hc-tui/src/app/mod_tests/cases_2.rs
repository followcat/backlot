use super::*;

#[tokio::test]
async fn host_erase_key_rewrites_only_measured_ctrl_h() {
    let mut app = app();
    let raw = |ch: char, vt: &[u8]| {
        input::TerminalKey::new(hc_vt::KeyInput {
            key: hc_vt::Key::Char(ch),
            mods: hc_vt::Mods::NONE,
            action: hc_vt::Action::Press,
        })
        .with_vt_bytes(vt.to_vec())
    };
    // Not measured → the raw byte travels untouched.
    let k = app.host_erase_key(raw('\x08', &[0x08]));
    assert_eq!(k.input.key, hc_vt::Key::Char('\x08'));
    app.set_host_erase_byte(Some(0x08));
    let k = app.host_erase_key(raw('\x08', &[0x08]));
    assert_eq!(k.input.key, hc_vt::Key::Backspace);
    assert_eq!(k.input.mods, hc_vt::Mods::NONE);
    assert_eq!(k.vt_bytes, vec![0x08]);
    let k = app.host_erase_key(raw('\x08', &[0x1b, 0x08]));
    assert_eq!(k.input.key, hc_vt::Key::Backspace);
    assert_eq!(k.input.mods, hc_vt::Mods::ALT);
    assert_eq!(k.vt_bytes, vec![0x1b, 0x08]);
    // Other bytes pass through even on a ^H host.
    let k = app.host_erase_key(raw('\x7f', &[0x7f]));
    assert_eq!(k.input.key, hc_vt::Key::Char('\x7f'));
    // A different measured byte leaves the rewrite inert.
    app.set_host_erase_byte(Some(0x7f));
    let k = app.host_erase_key(raw('\x08', &[0x08]));
    assert_eq!(k.input.key, hc_vt::Key::Char('\x08'));
}
#[tokio::test]
async fn capability_probe_drives_key_release_reporting() {
    use crate::host::HostReply;
    let mut legacy = app();
    assert!(!legacy.host_reports_key_releases);
    // Legacy host: flags say 0 → releases stay off even with DA.
    legacy.on_host_reply(HostReply::KeyboardFlags(0));
    legacy.on_host_reply(HostReply::PrimaryDeviceAttributes);
    assert!(!legacy.host_reports_key_releases);
    // Flags arriving after DA are ignored (ordering rule).
    legacy.on_host_reply(HostReply::KeyboardFlags(1));
    assert!(!legacy.host_reports_key_releases);
    // Kitty host: flags bit0 then DA → releases on.
    let mut app2 = app();
    app2.on_host_reply(HostReply::KeyboardFlags(0b1011));
    app2.on_host_reply(HostReply::PrimaryDeviceAttributes);
    assert!(app2.host_reports_key_releases);
    // DA alone proves nothing about flags.
    let mut app3 = app();
    app3.on_host_reply(HostReply::PrimaryDeviceAttributes);
    assert!(!app3.host_reports_key_releases);
}
#[tokio::test]
async fn armed_prefix_keeps_plain_text_out_of_text_commit() {
    let mut app = app();
    app.host_reports_all_keys = true;
    app.host_reports_key_releases = true;
    let q = {
        let mut k = input::TerminalKey::new(hc_vt::KeyInput {
            key: hc_vt::Key::Char('q'),
            mods: hc_vt::Mods::NONE,
            action: hc_vt::Action::Press,
        });
        k.text_commit = Some("q".into());
        k.vt_bytes = b"q".to_vec();
        k
    };
    assert_eq!(app.host_text_commit(&q).as_deref(), Some("q"));
    // Arm the prefix: the same plain key is now a binding
    // candidate, never a commit.
    app.on_key(
        input::TerminalKey::new(hc_vt::KeyInput {
            key: hc_vt::Key::Char('b'),
            mods: hc_vt::Mods::CTRL,
            action: hc_vt::Action::Press,
        }),
        80,
    );
    assert!(app.prefix.armed());
    assert_eq!(app.host_text_commit(&q), None);
}
#[tokio::test]
async fn orphan_release_drops_without_reaching_pane() {
    let mut app = app();
    let f9_up = term_key(hc_vt::Key::F(9), hc_vt::Mods::NONE, hc_vt::Action::Release);
    assert!(!app.release_pane_lease(&f9_up));
    assert_eq!(app.pane.input_backlog(), (0, 0));
    assert!(app.pane_leases.is_empty());
}
#[tokio::test]
async fn non_char_release_never_uses_sole_text_fallback() {
    let mut app = app();
    let mut press = term_key(
        hc_vt::Key::Char('a'),
        hc_vt::Mods::NONE,
        hc_vt::Action::Press,
    );
    press.text_commit = Some("a".into());
    app.pane_leases.insert_forwarded(
        key_leases::InputLeaseKey::new(0, &press),
        app.pane.generation(),
        press,
    );
    // F9-up is not Char: sole 'a' lease stays, nothing is taken.
    let f9_up = term_key(hc_vt::Key::F(9), hc_vt::Mods::NONE, hc_vt::Action::Release);
    assert!(app.take_pane_lease(&f9_up).is_none());
    assert_eq!(app.pane_leases.len(), 1);
}
#[tokio::test]
async fn dead_route_release_drops_never_replays_live() {
    let mut app = app();
    let live_route = app.pane.generation();
    let dead_route = live_route + 7;
    let press = term_key(
        hc_vt::Key::Char('a'),
        hc_vt::Mods::NONE,
        hc_vt::Action::Press,
    );
    app.pane_leases
        .insert_forwarded(key_leases::InputLeaseKey::new(0, &press), dead_route, press);
    let a_up = term_key(
        hc_vt::Key::Char('a'),
        hc_vt::Mods::NONE,
        hc_vt::Action::Release,
    );
    assert!(app.release_pane_lease(&a_up)); // the lease matched
    // …but its wire is dead: undeliverable, and NOT requeued on the
    // live pane's pending input.
    assert_eq!(app.pane.input_backlog(), (0, 0));
    assert!(app.pane_leases.is_empty());
}
#[tokio::test]
async fn route_without_an_attach_rejects_release() {
    let mut app = app();
    let route = app.pane.generation();
    let press = term_key(
        hc_vt::Key::Char('a'),
        hc_vt::Mods::NONE,
        hc_vt::Action::Press,
    );
    app.pane_leases
        .insert_forwarded(key_leases::InputLeaseKey::new(0, &press), route, press);
    let a_up = term_key(
        hc_vt::Key::Char('a'),
        hc_vt::Mods::NONE,
        hc_vt::Action::Release,
    );
    assert!(app.release_pane_lease(&a_up));
    assert_eq!(app.pane.input_backlog(), (0, 0));
}
#[tokio::test]
async fn lease_route_prefers_actual_dispatch() {
    let mut app = app();
    let press = term_key(
        hc_vt::Key::Char('a'),
        hc_vt::Mods::NONE,
        hc_vt::Action::Press,
    );
    let mut press = press;
    press.dispatch_id = 1;
    app.pane.dispatched.push_back((1, 99));
    app.pane_leases
        .insert_forwarded(key_leases::InputLeaseKey::new(0, &press), 1, press);
    assert_eq!(
        app.lease_route(app.pane_leases.forwarded().next().unwrap()),
        99
    );
}
