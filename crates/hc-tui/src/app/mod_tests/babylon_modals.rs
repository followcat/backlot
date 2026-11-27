//! End-to-end Babylon wave-4 (`S5`) integration checks across `--plain`,
//! `[theme] source = "herdr"`, and Backlot studio mode.

use std::time::{Duration, Instant};

use super::*;
use crate::app::action_tracker::ActionToast;
use crate::app::dialogs::{Body, Target};
use crate::chrome::dialogs::Confirm;
use crate::new_studio::{NewStudio, Step};
use crate::panel::PanelMode;
use crate::studio::StudioKey;
use crate::studio_dialog::StudioDialog;

fn themed_app() -> (App, crate::test_support::TestDir, std::path::PathBuf) {
    let dir = crate::test_support::TestDir::new("babylon-modals-app");
    let herdr_path = dir.join("herdr.toml");
    let own_path = dir.join("own.toml");
    std::fs::write(&herdr_path, "[theme]\nname = \"catppuccin\"\n").unwrap();
    std::fs::write(&own_path, "").unwrap();
    let mut app = app_with(&["t1"]);
    app.shell.local_config_path = herdr_path;
    app.shell.own_theme = crate::own_theme::Shell::at(own_path.clone());
    app.area = (120, 36);
    app.opening.dismissed = true;
    (app, dir, own_path)
}

#[tokio::test]
async fn plain_and_source_herdr_keep_rounded_popups_and_toasts() {
    let (mut app, _dir, own_path) = themed_app();

    // 1. `--plain`: Herdr Confirm stays rounded (`╭─╮`, `" ↵ confirm "`).
    app.set_plain(true);
    app.open_dialog(
        Target::MachineRemove {
            machine: "host-a".into(),
        },
        Body::Confirm(Confirm {
            title: "Remove machine?".into(),
            detail: "host-a".into(),
        }),
    );
    let plain_text = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(
        plain_text.contains('╭'),
        "--plain keeps rounded:\n{plain_text}"
    );
    assert!(
        !plain_text.contains("╔══╡") && !plain_text.contains('◉'),
        "--plain never draws Deco border:\n{plain_text}"
    );
    app.dialog = None;

    // 2. `[theme] source = "herdr"` with `!plain`: keeps rounded popups & `┌─┐` toast.
    std::fs::write(&own_path, "[theme]\nsource = \"herdr\"\n").unwrap();
    app.set_plain(false);
    app.reload_config();
    assert!(!app.shell.studio_chrome);

    app.open_dialog(
        Target::MachineRemove {
            machine: "host-b".into(),
        },
        Body::Confirm(Confirm {
            title: "Remove machine?".into(),
            detail: "host-b".into(),
        }),
    );
    let herdr_confirm = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(herdr_confirm.contains('╭'), "{herdr_confirm}");
    assert!(!herdr_confirm.contains("╔══╡"), "{herdr_confirm}");
    app.dialog = None;

    app.studio_dialog = Some(StudioDialog::delete_studio_force(
        &StudioKey {
            machine: None,
            id: "acme-shop".into(),
        },
        &[],
    ));
    let herdr_studio = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(herdr_studio.contains('╭'), "{herdr_studio}");
    assert!(!herdr_studio.contains("╔══╡"), "{herdr_studio}");
    app.studio_dialog = None;

    let mut ns = NewStudio::new();
    ns.step = Step::IdTitle;
    app.new_studio = Some(ns);
    let herdr_ns = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(herdr_ns.contains('╭'), "{herdr_ns}");
    assert!(!herdr_ns.contains("✦ NEW STUDIO"), "{herdr_ns}");
    app.new_studio = None;

    app.action_toast = Some(ActionToast {
        title: "finished acme-shop".into(),
        body: String::new(),
        fail: false,
        command: None,
        start_stage: None,
        deadline: Instant::now() + Duration::from_secs(4),
    });
    let herdr_toast = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(herdr_toast.contains('┌'), "{herdr_toast}");
    assert!(!herdr_toast.contains("THE END"), "{herdr_toast}");
}

#[tokio::test]
async fn studio_mode_draws_deco_modals_the_end_toast_and_empty_lot_plaque() {
    let (mut app, _dir, own_path) = themed_app();
    std::fs::write(&own_path, "[theme]\nname = \"backlot\"\n").unwrap();
    app.set_plain(false);
    app.reload_config();
    assert!(app.shell.studio_chrome);

    // Empty Lot dock (height >= 5) renders centered 3-line plaque.
    app.panel.mode = PanelMode::Expanded;
    let empty_text = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(empty_text.contains("─── ✦ ───"), "{empty_text}");
    assert!(empty_text.contains("no studios on the lot"), "{empty_text}");
    assert!(
        empty_text.contains("n new studio · alt+y hollywood"),
        "{empty_text}"
    );

    // Destructive StudioDialog renders Deco red border + `◉`.
    app.studio_dialog = Some(StudioDialog::delete_studio_force(
        &StudioKey {
            machine: None,
            id: "acme-shop".into(),
        },
        &[],
    ));
    let del_text = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(
        del_text.contains("╔══╡ ◉ Force delete studio?"),
        "{del_text}"
    );
    assert!(del_text.contains("‹ ↵ / y confirm ›"), "{del_text}");
    app.studio_dialog = None;

    // Sign-off StudioDialog (with checks) renders Deco accent + gilt border + `‹ ↵ sign + checks ›`.
    let checks = [backlot_model::event::CheckDecl::new(
        "test",
        "cargo test",
        30,
    )];
    app.studio_dialog = Some(StudioDialog::design_sign(
        &StudioKey {
            machine: Some("host-b".into()),
            id: "beta-api".into(),
        },
        false,
        Some("design/v1.md"),
        &checks,
    ));
    let sign_text = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(sign_text.contains("╔══╡ "), "{sign_text}");
    assert!(sign_text.contains("‹ ↵ sign + checks ›"), "{sign_text}");
    app.studio_dialog = None;

    // Finishing studio action toast renders `╔═╡ ● <studio> · THE END ╞═╗`.
    app.action_toast = Some(ActionToast {
        title: "finished acme-shop".into(),
        body: String::new(),
        fail: false,
        command: None,
        start_stage: None,
        deadline: Instant::now() + Duration::from_secs(4),
    });
    let toast_text = screen_lines(&draw_to(&mut app, 120, 36)).join("\n");
    assert!(
        toast_text.contains("╔═╡ ● acme-shop · THE END ╞═╗"),
        "{toast_text}"
    );
}

#[test]
#[ignore = "prints verbatim snapshots for the wave-4 report"]
fn report_snapshots() {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    let p = crate::chrome::palette::Palette::backlot();

    // 1. Opening card 49x16
    let open_area = Rect::new(0, 0, 55, 18);
    let mut open_buf = Buffer::empty(open_area);
    crate::opening::render_opening(
        &mut open_buf,
        open_area,
        &p,
        crate::opening::OpeningOpts {
            hollywood: false,
            stage_running: false,
        },
        None,
    );
    println!(
        "=== OPENING CARD (55x18) ===\n{}",
        dump_rect(&open_buf, open_area)
    );

    // 2. Destructive DeleteStudio & DesignSign dialogs
    let band = Rect::new(0, 0, 74, 18);
    let del = StudioDialog::delete_studio_force(
        &StudioKey {
            machine: Some("host-b".into()),
            id: "acme-shop".into(),
        },
        &["wt-1".into()],
    );
    let mut del_buf = Buffer::empty(band);
    let dh = crate::chrome::studio_dialogs::render(&mut del_buf, band, &del, &p).unwrap();
    println!(
        "=== DESTRUCTIVE CONFIRM ===\n{}",
        dump_rect(&del_buf, dh.area)
    );

    let checks = [backlot_model::event::CheckDecl::new(
        "test",
        "cargo test",
        30,
    )];
    let sign = StudioDialog::design_sign(
        &StudioKey {
            machine: Some("host-b".into()),
            id: "beta-api".into(),
        },
        false,
        Some("design/v1.md"),
        &checks,
    );
    let mut sign_buf = Buffer::empty(Rect::new(0, 0, 76, 22));
    let sh =
        crate::chrome::studio_dialogs::render(&mut sign_buf, Rect::new(0, 0, 76, 22), &sign, &p)
            .unwrap();
    println!(
        "=== DESIGN SIGN DIALOG ===\n{}",
        dump_rect(&sign_buf, sh.area)
    );

    // 3. Action toasts
    let t_area = Rect::new(0, 0, 56, 7);
    for (label, title, body, chip) in [
        (
            "TOAST SUCCESS",
            "priority task-1 → high",
            Some("recorded on disk · stage stopped"),
            Some(crate::chrome::notice_render::ToastChip::StartStage),
        ),
        (
            "TOAST FAIL",
            "hollywood switch failed",
            Some("quota exceeded"),
            Some(crate::chrome::notice_render::ToastChip::CopyCmd),
        ),
        ("TOAST THE END", "finished acme-shop", None, None),
    ] {
        let mut tb = Buffer::empty(t_area);
        let (tr, _) = crate::chrome::notice_render::render_action_toast(
            &mut tb, t_area, title, body, chip, 0, &p, true,
        );
        println!("=== {label} ===\n{}", dump_rect(&tb, tr));
    }

    // 4. New Studio wizard at 72x22 (Step 2)
    let w_area = Rect::new(0, 0, 72, 22);
    let mut wb = Buffer::empty(w_area);
    let mut ns = NewStudio::new();
    ns.step = Step::IdTitle;
    crate::new_studio::render(&mut wb, &ns, &p, w_area);
    println!(
        "=== NEW STUDIO WIZARD (72 cols, step 2) ===\n{}",
        dump_rect(&wb, w_area)
    );

    // 5. Empty Lot plaques (tall & short)
    for (name, h) in [
        ("EMPTY LOT TALL (body>=5)", 9u16),
        ("EMPTY LOT SHORT (body<5)", 5u16),
    ] {
        let la = Rect::new(0, 0, 36, h);
        let mut lb = Buffer::empty(la);
        crate::panel::render(
            &mut lb,
            la,
            &[],
            |_| true,
            |_| true,
            &crate::panel::Panel::new(),
            PanelMode::Expanded,
            None,
            None,
            0,
            None,
            &crate::panel::StudioKeys::default(),
            None,
            false,
            false,
            true,
            &Default::default(),
            &p,
            1,
        );
        println!("=== {name} ===\n{}", dump_rect(&lb, la));
    }
}

fn dump_rect(buf: &ratatui::buffer::Buffer, area: ratatui::layout::Rect) -> String {
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
