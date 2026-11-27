//! Babylon 26-column machines sidebar tests for marking the current studio's
//! hosting machine (`Local ✦`, `<remote> ✦`, no current, long name dropping `✦`
//! without shifting the right-side status dot, `--plain`, `source = "herdr"`,
//! and hit-map invariance).

use super::*;
use ratatui::style::Modifier;

fn find_machine_y(buf: &ratatui::buffer::Buffer, h: u16, label: &str) -> u16 {
    (0..h)
        .find(|&y| {
            let row: String = (0..26).map(|x| buf[(x, y)].symbol()).collect();
            row.contains(label)
        })
        .unwrap_or_else(|| panic!("missing machine row `{label}`"))
}

fn sidebar_lines(buf: &ratatui::buffer::Buffer, h: u16) -> Vec<String> {
    (0..h)
        .map(|y| {
            let mut out = String::new();
            let mut x = 0u16;
            while x < 26 {
                let sym = buf[(x, y)].symbol();
                out.push_str(sym);
                x = x.saturating_add(crate::chrome::text::display_width(sym).max(1) as u16);
            }
            out
        })
        .collect()
}

fn studio_doc(id: &str, title: &str, updated_ms: u64) -> hc_proto::watch::Document {
    hc_proto::watch::Document {
        value: serde_json::json!({
            "schema": "backlot/1",
            "updated_ms": updated_ms,
            "studios": [{
                "id": id,
                "title": title,
                "state": "live",
                "requirement_version": 1,
                "mode": "standard",
                "open_reviews": 0,
                "waiting": 0,
                "roles": [{
                    "role": "producer",
                    "name": "producer",
                    "terminal": "t1",
                    "state": "live",
                    "work": "waiting"
                }]
            }]
        }),
        ..Default::default()
    }
}

fn app_with_remote_name(remote_name: &str) -> App {
    let spaced = |addr: &str, space: &str| {
        let mut t = crate::sidebar::fixture::term(addr);
        t.labels.insert("herdr.space".into(), space.into());
        t.labels.insert("herdr.tab".into(), "main".into());
        t
    };
    let mut stage = spaced("t9", "backlot");
    stage.name = Some("backlot-stage".into());
    let mut remote_stage = spaced(&format!("{remote_name}/t9"), "backlot");
    remote_stage.name = Some("backlot-stage".into());
    let mut app = super::endpoint_nav::app_multi(
        vec![
            spaced("t1", "acme-shop"),
            stage,
            spaced(&format!("{remote_name}/t1"), "gamma-ops"),
            remote_stage,
        ],
        vec![crate::sidebar::fixture::machine(remote_name, "connected")],
    );
    app.set_plain(false);
    app.view.documents_mut().insert(
        hc_proto::watch::DocKey {
            machine: None,
            name: "backlot".into(),
        },
        studio_doc("acme-shop", "Acme shop", 1).into(),
    );
    app.view.documents_mut().insert(
        hc_proto::watch::DocKey {
            machine: Some(remote_name.into()),
            name: "backlot".into(),
        },
        studio_doc("gamma-ops", "Gamma ops", 2).into(),
    );
    app.focused = Some(format!("{remote_name}/t1"));
    app.reconcile_published_view();
    app.hollywood.mode = crate::hollywood::LayerMode::Hidden;
    app
}

#[tokio::test]
async fn sidebar_26_cols_marks_local_remote_none_and_drops_star_on_long_name() {
    // 1. Current studio on `Local`: `Local` in accent bold + ` ✦` in accent; `host-b` unchanged.
    let mut app = super::studio_machines::two_machine_studio_app();
    app.area = (120, 24);
    let accent = app.shell.palette.accent;
    let text_fg = app.shell.palette.text;
    let green = app.shell.palette.green;

    let b_local = draw_to(&mut app, 120, 24);
    let buf = b_local.buffer();
    let ly = find_machine_y(buf, 24, "▾ Local");
    let hy = find_machine_y(buf, 24, "▾ host-b");
    let local_lines = sidebar_lines(buf, 24);
    assert!(
        local_lines[ly as usize].starts_with(" ▾ Local ✦"),
        "{}",
        local_lines[ly as usize]
    );
    assert!(
        !local_lines[hy as usize].contains('✦'),
        "{}",
        local_lines[hy as usize]
    );
    assert_eq!(buf[(1, ly)].fg, text_fg);
    for x in 3..8 {
        assert_eq!(buf[(x, ly)].fg, accent, "Local name fg x={x}");
        assert!(buf[(x, ly)].modifier.contains(Modifier::BOLD));
    }
    assert_eq!(buf[(9, ly)].symbol(), "✦");
    assert_eq!(buf[(9, ly)].fg, accent);
    assert_eq!(buf[(3, hy)].fg, text_fg);
    assert_eq!(buf[(24, hy)].symbol(), "●");
    assert_eq!(buf[(24, hy)].fg, green);
    let local_hits = app.hit_map.machines.clone();

    // 2. Current studio on remote `host-b`: `host-b` in accent bold + ` ✦`; `Local` unchanged.
    app.focused = Some("host-b/t1".into());
    app.reconcile_published_view();
    assert_eq!(
        app.studio.current,
        Some(crate::studio::StudioKey {
            machine: Some("host-b".into()),
            id: "beta-tools".into(),
        })
    );
    let b_remote = draw_to(&mut app, 120, 24);
    let rbuf = b_remote.buffer();
    let remote_lines = sidebar_lines(rbuf, 24);
    assert!(
        !remote_lines[ly as usize].contains('✦'),
        "{}",
        remote_lines[ly as usize]
    );
    assert!(
        remote_lines[hy as usize].starts_with(" ▾ host-b ✦"),
        "{}",
        remote_lines[hy as usize]
    );
    assert_eq!(rbuf[(3, ly)].fg, text_fg);
    for x in 3..9 {
        assert_eq!(rbuf[(x, hy)].fg, accent, "host-b name fg x={x}");
        assert!(rbuf[(x, hy)].modifier.contains(Modifier::BOLD));
    }
    assert_eq!(rbuf[(10, hy)].symbol(), "✦");
    assert_eq!(rbuf[(10, hy)].fg, accent);
    assert_eq!(rbuf[(24, hy)].symbol(), "●");
    assert_eq!(rbuf[(24, hy)].fg, green);
    // Hit maps remain identical (`rect`, `status_badge`, `collapse_toggle`, `endpoint_id`).
    assert_eq!(app.hit_map.machines.len(), local_hits.len());
    for (a, b) in local_hits.iter().zip(app.hit_map.machines.iter()) {
        assert_eq!(a.rect, b.rect);
        assert_eq!(a.status_badge, b.status_badge);
        assert_eq!(a.collapse_toggle, b.collapse_toggle);
        assert_eq!(a.endpoint_id, b.endpoint_id);
    }

    // 3. No current studio at all: neither machine gets `✦` or `accent`.
    let mut empty_app = app_with(&[]);
    empty_app.area = (120, 24);
    empty_app.set_plain(false);
    empty_app.studio.current = None;
    let b_none = draw_to(&mut empty_app, 120, 24);
    let nbuf = b_none.buffer();
    let none_lines = sidebar_lines(nbuf, 24);
    let n_ly = find_machine_y(nbuf, 24, "▾ Local");
    assert!(!none_lines[n_ly as usize].contains('✦'));
    assert_eq!(nbuf[(3, n_ly)].fg, empty_app.shell.palette.text);

    // 4. Long machine name where ` ✦` drops: keeps bold `accent` name and right `●` dot at col 24.
    let long_name = "remote-build-host-1"; // 19 cols: +2 = 21 > 20 available beside `●`
    let mut long_app = app_with_remote_name(long_name);
    long_app.area = (120, 24);
    let b_long = draw_to(&mut long_app, 120, 24);
    let lbuf = b_long.buffer();
    let long_lines = sidebar_lines(lbuf, 24);
    let l_hy = find_machine_y(lbuf, 24, "▾ remote-build-host-1");
    assert!(
        !long_lines[l_hy as usize].contains('✦'),
        "{}",
        long_lines[l_hy as usize]
    );
    assert_eq!(lbuf[(3, l_hy)].fg, long_app.shell.palette.accent);
    assert!(lbuf[(3, l_hy)].modifier.contains(Modifier::BOLD));
    assert_eq!(lbuf[(24, l_hy)].symbol(), "●");
    assert_eq!(lbuf[(24, l_hy)].fg, long_app.shell.palette.green);
    assert_eq!(
        long_app.hit_map.machines[1].status_badge,
        local_hits[1].status_badge
    );

    // Boundary: 18-char remote name still fits ` ✦` beside `●`; 24-char truncates with `…`.
    let fit_name = "remote-build-hos18"; // 18 cols: +2 = 20 <= 20
    let mut fit_app = app_with_remote_name(fit_name);
    fit_app.area = (120, 24);
    let fbuf = draw_to(&mut fit_app, 120, 24);
    let f_hy = find_machine_y(fbuf.buffer(), 24, "▾ remote-build-hos18");
    let f_line = &sidebar_lines(fbuf.buffer(), 24)[f_hy as usize];
    assert!(f_line.starts_with(" ▾ remote-build-hos18 ✦"), "{f_line:?}");
    assert_eq!(fbuf.buffer()[(24, f_hy)].symbol(), "●");

    let trunc_name = "very-long-remote-machine-x"; // 26 cols -> truncates to 20 with `…`
    let mut trunc_app = app_with_remote_name(trunc_name);
    trunc_app.area = (120, 24);
    let tbuf = draw_to(&mut trunc_app, 120, 24);
    let t_hy = find_machine_y(tbuf.buffer(), 24, "▾ very-long-remote-ma…");
    let tline = &sidebar_lines(tbuf.buffer(), 24)[t_hy as usize];
    assert!(!tline.contains('✦'), "{tline:?}");
    assert_eq!(tbuf.buffer()[(24, t_hy)].symbol(), "●");

    if std::env::var_os("BABYLON_MARQUEE_DUMP").is_some() {
        eprintln!(
            "=== SIDEBAR 26-COL LOCAL ===\n{}\n=== SIDEBAR 26-COL REMOTE ===\n{}\n=== SIDEBAR 26-COL NONE ===\n{}\n=== SIDEBAR 26-COL LONG NAME ===\n{}",
            local_lines[..10].join("\n"),
            remote_lines[..10].join("\n"),
            none_lines[..10].join("\n"),
            long_lines[..10].join("\n")
        );
    }
}

#[tokio::test]
async fn sidebar_26_cols_plain_and_source_herdr_unchanged() {
    let dir = crate::test_support::TestDir::new("sidebar-marquee-herdr");
    let herdr_path = dir.join("herdr.toml");
    let own_path = dir.join("own.toml");
    std::fs::write(&herdr_path, "[theme]\nname = \"catppuccin\"\n").unwrap();
    std::fs::write(&own_path, "[theme]\nsource = \"herdr\"\n").unwrap();

    // `source = "herdr"`: `studio_chrome == false` -> no `✦`, no `accent` on `Local`.
    let mut app = super::studio_machines::two_machine_studio_app();
    app.area = (120, 24);
    app.shell.local_config_path = herdr_path;
    app.shell.own_theme = crate::own_theme::Shell::at(own_path);
    app.reload_config();
    assert!(!app.shell.studio_chrome);
    let h_backend = draw_to(&mut app, 120, 24);
    let hbuf = h_backend.buffer();
    let h_ly = find_machine_y(hbuf, 24, "▾ Local");
    let h_line = &sidebar_lines(hbuf, 24)[h_ly as usize];
    assert!(!h_line.contains('✦'), "{h_line:?}");
    assert_eq!(hbuf[(3, h_ly)].fg, app.shell.palette.text);

    // `--plain`: no `✦`, no `accent`.
    let mut plain_app = super::studio_machines::two_machine_studio_app();
    plain_app.area = (120, 24);
    plain_app.set_plain(true);
    let p_backend = draw_to(&mut plain_app, 120, 24);
    let pbuf = p_backend.buffer();
    let p_ly = find_machine_y(pbuf, 24, "▾ Local");
    let p_line = &sidebar_lines(pbuf, 24)[p_ly as usize];
    assert!(!p_line.contains('✦'), "{p_line:?}");
    assert_eq!(pbuf[(3, p_ly)].fg, plain_app.shell.palette.text);
}
