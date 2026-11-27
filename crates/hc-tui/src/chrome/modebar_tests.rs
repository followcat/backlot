use super::*;
use ratatui::buffer::Buffer;

fn bar_text(buf: &Buffer, area: Rect) -> String {
    let y = area.bottom() - 1;
    let mut out = String::new();
    let mut x = area.x;
    while x < area.right() {
        let sym = buf[(x, y)].symbol();
        out.push_str(sym);
        let w = crate::chrome::text::display_width(sym).max(1) as u16;
        x = x.saturating_add(w);
    }
    out.trim_end().to_string()
}

#[test]
fn plaques_focus_locked_resize_and_plain_bypass() {
    let pal = Palette::backlot();
    let elev = pal.elevation();
    let keys = crate::config::Config::default().live_keybinds();
    let area = Rect::new(0, 0, 120, 4);

    // FOCUS plaque (`accent` bg, `stage_well` fg, BOLD, `·` dividers on `stage_well`).
    let mut fbuf = Buffer::empty(area);
    render_mode_bar(
        &mut fbuf,
        area,
        ClientShellMode::Terminal,
        false,
        None,
        None,
        false,
        false,
        false,
        Some("FOCUS: lot › acme-shop"),
        &keys,
        &pal,
        true,
    );
    let ftext = bar_text(&fbuf, area);
    let y = area.bottom() - 1;
    assert!(ftext.starts_with("╡ FOCUS: LOT · acme-shop ╞"), "{ftext:?}");
    assert!(ftext.contains("Alt+L lot · Alt+Y call sheet"), "{ftext:?}");
    assert_eq!(fbuf[(0, y)].bg, pal.accent);
    assert_eq!(fbuf[(0, y)].fg, elev.stage_well);
    assert!(fbuf[(0, y)].modifier.contains(Modifier::BOLD));
    assert_eq!(fbuf[(119, y)].bg, elev.stage_well);

    // LOCKED & ERROR plaques (`red` bg).
    for (locked, err, tag) in [
        (true, None, "╡ LOCKED ╞"),
        (false, Some("down"), "╡ ERROR ╞"),
    ] {
        let mut lbuf = Buffer::empty(area);
        render_mode_bar(
            &mut lbuf,
            area,
            ClientShellMode::Terminal,
            locked,
            None,
            err,
            false,
            false,
            false,
            None,
            &keys,
            &pal,
            true,
        );
        let ltext = bar_text(&lbuf, area);
        assert!(ltext.starts_with(tag), "{ltext:?}");
        assert_eq!(lbuf[(0, y)].bg, pal.red);
        assert_eq!(lbuf[(0, y)].fg, elev.stage_well);
    }

    // RESIZE plaque (`mauve` bg).
    let mut rbuf = Buffer::empty(area);
    render_mode_bar(
        &mut rbuf,
        area,
        ClientShellMode::Resize,
        false,
        None,
        None,
        false,
        false,
        false,
        None,
        &keys,
        &pal,
        true,
    );
    let rtext = bar_text(&rbuf, area);
    assert!(rtext.starts_with("╡ RESIZE ╞"), "{rtext:?}");
    assert_eq!(rbuf[(0, y)].bg, pal.mauve);
    assert_eq!(rbuf[(0, y)].fg, elev.stage_well);

    // CJK studio focus truncates cleanly within tight bar width.
    let narrow = Rect::new(0, 0, 28, 2);
    let mut cbuf = Buffer::empty(narrow);
    render_mode_bar(
        &mut cbuf,
        narrow,
        ClientShellMode::Terminal,
        false,
        None,
        None,
        false,
        false,
        false,
        Some("FOCUS: lot › 中文工作室长名称"),
        &keys,
        &pal,
        true,
    );
    let ctext = bar_text(&cbuf, narrow);
    assert!(UnicodeWidthStr::width(ctext.as_str()) <= 28, "{ctext:?}");

    // `--plain` / `source = "herdr"` (`studio_chrome = false`): untouched Herdr tag on `panel_bg`.
    let mut pbuf = Buffer::empty(area);
    render_mode_bar(
        &mut pbuf,
        area,
        ClientShellMode::Terminal,
        false,
        None,
        None,
        false,
        false,
        false,
        Some("FOCUS: lot › acme-shop"),
        &keys,
        &pal,
        false,
    );
    let ptext = bar_text(&pbuf, area);
    assert!(ptext.starts_with(" FOCUS: lot › acme-shop "), "{ptext:?}");
    assert!(!ptext.contains('╡') && !ptext.contains('·'), "{ptext:?}");
    assert_eq!(pbuf[(119, y)].bg, pal.panel_bg);
    if std::env::var_os("BABYLON_LOT_DUMP").is_some() {
        let locked_s = {
            let mut lb = Buffer::empty(area);
            render_mode_bar(
                &mut lb,
                area,
                ClientShellMode::Terminal,
                true,
                None,
                None,
                false,
                false,
                false,
                None,
                &keys,
                &pal,
                true,
            );
            bar_text(&lb, area)
        };
        eprintln!("=== MODEBAR PLAQUES ===\n{ftext}\n{locked_s}\n{rtext}");
    }
}
