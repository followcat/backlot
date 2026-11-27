//! S3 §2 dialog chrome tests — each kind's title and wording, the
//! responsive width and the band geometry (the card never leaves
//! the band the app's `modal_band()` hands over, so a beside dock's
//! border is never crossed).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::{dialog_width, render};
use crate::chrome::palette::Palette;
use crate::studio::StudioKey;
use crate::studio_dialog::{BodyEditor, ReasonFor, ReviseDialog, StudioDialog};

fn text_of(buf: &Buffer, area: Rect) -> String {
    (area.y..area.y + area.height)
        .map(|y| {
            (area.x..area.x + area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn key(machine: Option<&str>) -> StudioKey {
    StudioKey {
        machine: machine.map(str::to_owned),
        id: "acme-shop".into(),
    }
}

fn reason_dialog() -> StudioDialog {
    StudioDialog::reason(
        "Pause task t1 · acme-shop · host-b".to_owned(),
        true,
        ReasonFor::TaskVerb {
            key: key(Some("host-b")),
            task: "t1".into(),
            verb: "pause",
        },
    )
}

#[test]
fn the_reason_dialog_shows_its_title_and_line() {
    let area = Rect::new(0, 0, 120, 36);
    let mut buf = Buffer::empty(area);
    let palette = Palette::catppuccin();
    let band = Rect::new(0, 0, 100, 36);
    let hits = render(&mut buf, band, &reason_dialog(), &palette).unwrap();
    let text = text_of(&buf, hits.area);
    assert!(
        text.contains("Pause task t1 · acme-shop · host-b"),
        "{text}"
    );
    assert!(text.contains("reason:"), "{text}");
    // R3-N4: only the button row carries `↵ submit` / `esc cancel`, no
    // duplicate plain-text hint line above the buttons.
    assert_eq!(text.matches("↵ submit").count(), 1, "{text}");
    assert_eq!(text.matches("esc cancel").count(), 1, "{text}");
}

#[test]
fn the_confirm_dialog_shows_the_spec_wording() {
    let area = Rect::new(0, 0, 120, 36);
    let mut buf = Buffer::empty(area);
    let palette = Palette::catppuccin();
    let band = Rect::new(0, 0, 100, 36);
    let d = StudioDialog::dir_override(&key(Some("host-b")), "actor-alice", true);
    let hits = render(&mut buf, band, &d, &palette).unwrap();
    let text = text_of(&buf, hits.area);
    assert!(
        text.contains("Answer actor-alice's prompt? · acme-shop · host-b"),
        "{text}"
    );
    assert!(text.contains("routed to the director"), "{text}");
}

#[test]
fn the_tell_dialog_shows_its_editor() {
    let area = Rect::new(0, 0, 120, 36);
    let mut buf = Buffer::empty(area);
    let palette = Palette::catppuccin();
    let band = Rect::new(0, 0, 100, 36);
    let d = StudioDialog::Tell(crate::studio_dialog::TellDialog {
        title: "Tell alice · acme-shop · Local".into(),
        key: key(None),
        role: "actor-alice".into(),
        body: BodyEditor::default(),
        field: crate::studio_dialog::DialogField::Body,
        error: None,
    });
    let hits = render(&mut buf, band, &d, &palette).unwrap();
    let text = text_of(&buf, hits.area);
    assert!(text.contains("Tell alice · acme-shop · Local"), "{text}");
    assert!(
        hits.body.height >= 4,
        "the 4-row editor: {} rows",
        hits.body.height
    );
}

#[test]
fn the_revise_dialog_shows_the_prefilled_body() {
    let area = Rect::new(0, 0, 120, 36);
    let mut buf = Buffer::empty(area);
    let palette = Palette::catppuccin();
    let band = Rect::new(0, 0, 100, 36);
    let d = StudioDialog::Revise(ReviseDialog {
        title: "Revise requirement (v2 → v3) · acme-shop · host-b".into(),
        key: key(Some("host-b")),
        body: Some(BodyEditor::new("the cart totals in EUR")),
        summary: crate::chrome::text_editor::TextEditor::new("", false),
        field: crate::studio_dialog::DialogField::Body,
        local: false,
        error: None,
        prefill_failed: false,
        confirm_overwrite: false,
        editor_gen: 0,
    });
    let hits = render(&mut buf, band, &d, &palette).unwrap();
    let text = text_of(&buf, hits.area);
    assert!(
        text.contains("Revise requirement (v2 → v3) · acme-shop · host-b"),
        "{text}"
    );
    assert!(text.contains("the cart totals in EUR"), "{text}");
}

#[test]
fn the_card_stays_inside_the_band_at_every_width() {
    let palette = Palette::catppuccin();
    for cols in [160u16, 120, 80, 60, 40] {
        let area = Rect::new(0, 0, cols, 36);
        let mut buf = Buffer::empty(area);
        // A beside dock shrinks the band the app hands over.
        let band = Rect::new(0, 0, cols.saturating_sub(30).max(20), 36);
        let hits = render(&mut buf, band, &reason_dialog(), &palette).unwrap();
        assert!(
            hits.area.x >= band.x && hits.area.x + hits.area.width <= band.x + band.width,
            "cols {cols}: {:?} escapes {band:?}",
            hits.area
        );
    }
}

#[test]
fn the_width_follows_the_spec_ladder() {
    assert_eq!(dialog_width(160), 64);
    assert_eq!(dialog_width(120), 64);
    assert_eq!(dialog_width(80), 72);
    assert_eq!(dialog_width(60), 56);
    // Under 60 the band's own width caps it.
    assert!(dialog_width(40) <= 36);
}

#[test]
fn archived_list_hint_and_buttons_sit_on_distinct_rows() {
    let entries = (1..=5)
        .map(|n| crate::studio_index::ArchivedStudio {
            name: format!("acme-shop-{n}"),
            stamp: format!("2026-01-0{n}T12:00Z"),
            studio: Some(format!("acme-shop-{n}")),
            repo: Some("/home/user/acme".into()),
            requirement: Some(format!("Build step {n}")),
        })
        .collect();
    let d = StudioDialog::Restore(crate::studio_dialog::RestoreDialog::new(None, entries));
    let palette = Palette::catppuccin();
    for band_cols in [120u16, 80] {
        let area = Rect::new(0, 0, band_cols, 36);
        let mut buf = Buffer::empty(area);
        let band = Rect::new(0, 0, band_cols, 36);
        let hits = render(&mut buf, band, &d, &palette).unwrap();
        assert_eq!(hits.area.width, dialog_width(band_cols) + 6);
        let rendered = text_of(&buf, hits.area);
        let rows: Vec<&str> = rendered.lines().collect();
        let hint_row = rows
            .iter()
            .position(|line| line.contains("restore as"))
            .unwrap_or_else(|| panic!("missing hint line at {band_cols}:\n{rendered}"));
        let button_row = rows
            .iter()
            .position(|line| line.contains("choose"))
            .unwrap_or_else(|| panic!("missing choose button at {band_cols}:\n{rendered}"));
        assert_ne!(
            hint_row, button_row,
            "hint and buttons overlapped at {band_cols}:\n{rendered}"
        );
        assert!(
            hint_row < button_row,
            "hint row ({hint_row}) should sit above button row ({button_row}):\n{rendered}"
        );
        assert!(
            rows[button_row].contains("esc cancel"),
            "cancel button corrupted at {band_cols}:\n{rendered}"
        );
    }
}

#[test]
fn design_sign_modal_wraps_150_char_run_verbatim_at_80_and_200_cols_with_three_choices() {
    use backlot_model::event::CheckDecl;

    let long_run = format!("check_{}_end", "x".repeat(140));
    assert_eq!(long_run.len(), 150);
    let checks = vec![
        CheckDecl {
            expect: Some("files_scanned=[1-9]".into()),
            ..CheckDecl::new("long_scan", &long_run, 45)
        },
        CheckDecl::new("lint", "cargo clippy --all-targets -- -D warnings", 120),
    ];
    let d = StudioDialog::design_sign(&key(Some("host-b")), false, Some("design/v1.md"), &checks);
    let palette = Palette::catppuccin();

    for (cols, rows) in [(80u16, 40u16), (200u16, 44u16)] {
        let area = Rect::new(0, 0, cols, rows);
        let mut buf = Buffer::empty(area);
        let hits = render(&mut buf, area, &d, &palette).unwrap();
        let rendered = text_of(&buf, hits.area);
        assert!(
            !rendered.contains('…'),
            "sign modal must not truncate checks with ellipsis at {cols}x{rows}:\n{rendered}"
        );
        // Strip border glyphs and join wrapped lines inside the modal to prove
        // every single character of the 150-char `run` renders verbatim.
        let unwrapped: String = rendered
            .lines()
            .map(|l| {
                l.trim()
                    .trim_matches(|c: char| {
                        matches!(
                            c,
                            '│' | '╭'
                                | '╮'
                                | '╰'
                                | '╯'
                                | '─'
                                | '║'
                                | '╔'
                                | '╗'
                                | '╚'
                                | '╝'
                                | '═'
                                | '╡'
                                | '╞'
                        )
                    })
                    .trim()
            })
            .collect::<Vec<_>>()
            .join("");
        assert!(
            unwrapped.contains(&long_run),
            "150-char `run` cut at {cols}x{rows}:\n{rendered}"
        );
        for want in [
            "long_scan (timeout 45s)",
            "expect: files_scanned=[1-9]",
            "lint (timeout 120s)",
            "cargo clippy --all-targets -- -D warnings",
            "Sign off and activate 2 checks",
            "Sign off without checks (--no-checks)",
            "Cancel",
        ] {
            assert!(
                rendered.contains(want),
                "sign modal missing `{want}` at {cols}x{rows}:\n{rendered}"
            );
        }
        assert_eq!(hits.choices.len(), 3, "expected 3 choice hit rects");
    }
}

#[test]
fn deco_standard_and_destructive_dialogs_and_herdr_fallback() {
    use crate::studio_dialog::{ConfirmFor, ReasonFor};
    use ratatui::style::Modifier;

    let palette = Palette::catppuccin();
    let elev = palette.elevation();
    let band = Rect::new(0, 0, 90, 26);

    // Non-destructive sign-off uses accent top/bottom, gilt sides, `‹ ↵ … ›` bold on accent.
    let sign = StudioDialog::design_sign(&key(Some("host-b")), false, Some("design/v1.md"), &[]);
    let mut sign_buf = Buffer::empty(band);
    let sign_hits = render(&mut sign_buf, band, &sign, &palette).unwrap();
    let sign_text = text_of(&sign_buf, sign_hits.area);
    assert!(sign_text.contains("╔══╡ "), "{sign_text}");
    assert!(
        !sign_text.contains('◉'),
        "sign off is non-destructive:\n{sign_text}"
    );
    assert!(sign_text.contains("‹ ↵ "), "{sign_text}");
    let bx = sign_hits.area.x + 1;
    let by = sign_hits.area.y + 1;
    assert_eq!(sign_buf[(bx, by)].symbol(), "╔");
    assert_eq!(sign_buf[(bx, by)].fg, palette.accent);
    assert_eq!(sign_buf[(bx, by + 1)].symbol(), "║");
    assert_eq!(sign_buf[(bx, by + 1)].fg, elev.gilt);
    assert_eq!(
        sign_buf[(sign_hits.send.x, sign_hits.send.y)].bg,
        palette.accent
    );
    assert!(
        sign_buf[(sign_hits.send.x + 2, sign_hits.send.y)]
            .modifier
            .contains(Modifier::BOLD)
    );

    // Destructive confirms use `palette.red` border (top + sides) + `◉` in title + red button.
    let destructive = [
        StudioDialog::confirm(
            "Delete studio acme-shop".into(),
            "Removes studio files.".into(),
            ConfirmFor::DeleteStudio {
                key: key(Some("host-b")),
                force: false,
                kept: vec![],
                token: None,
            },
        ),
        StudioDialog::reason(
            "Abandon task-1".into(),
            true,
            ReasonFor::TaskVerb {
                key: key(None),
                verb: "abandon",
                task: "task-1".into(),
            },
        ),
        StudioDialog::confirm(
            "Stop stage".into(),
            "Stops every running studio.".into(),
            ConfirmFor::StageStop { machine: None },
        ),
    ];
    for dialog in destructive {
        let mut buf = Buffer::empty(band);
        let hits = render(&mut buf, band, &dialog, &palette).unwrap();
        let text = text_of(&buf, hits.area);
        assert!(text.contains("╔══╡ ◉ "), "{text}");
        let x = hits.area.x + 1;
        let y = hits.area.y + 1;
        assert_eq!(buf[(x, y)].fg, palette.red, "{text}");
        assert_eq!(buf[(x, y + 1)].fg, palette.red, "{text}");
        assert_eq!(buf[(hits.send.x, hits.send.y)].bg, palette.red, "{text}");
    }

    // Herdr mode (`deco = false`) preserves rounded `╭─╮` borders.
    let del = StudioDialog::confirm(
        "Delete studio acme-shop".into(),
        "Removes studio files.".into(),
        ConfirmFor::DeleteStudio {
            key: key(None),
            force: false,
            kept: vec![],
            token: None,
        },
    );
    let mut herdr_buf = Buffer::empty(band);
    let herdr_hits =
        super::render_styled(&mut herdr_buf, band, &del, &palette, true, false).unwrap();
    assert_eq!(
        herdr_buf[(herdr_hits.area.x + 1, herdr_hits.area.y + 1)].symbol(),
        "╭"
    );
}
