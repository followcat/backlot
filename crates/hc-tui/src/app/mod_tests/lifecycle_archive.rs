//! W4b part 3 — the archived studios: the `Archived (n) …` tree row
//! and the restore list's dry run before the real copy.

use std::time::Instant;

use super::delete_studio::drain_dialog_fetch;
use super::lifecycle_fixtures::*;
use super::*;
use crate::chrome::render::StudioRowAct;
use crate::chrome::snapshot::ClientEndpointId;
use crate::chrome::studio_tree::Row;
use crate::studio_dialog::StudioDialog;
use hc_vt::{Key, Mods};

#[tokio::test]
async fn a_non_empty_archive_adds_the_row_and_restore_runs_dry_then_real() {
    let mut app = lifecycle_app();
    let recorded = scripted(&mut app);
    app.studio_index
        .note_archived_reply(None, Instant::now(), Ok(ARCHIVED_JSON));
    let tree = app.studio_sidebar_tree();
    let rows = tree.get(&ClientEndpointId::Local);
    assert!(
        rows.iter()
            .any(|row| matches!(row, Row::Archived { count: 1 })),
        "the tree row: {rows:?}"
    );
    let text = screen_lines(&draw_to(&mut app, 120, 60)).join("\n");
    assert!(text.contains("Archived (1) …"), "{text}");
    let hit = app
        .hit_map
        .studio_rows
        .iter()
        .find(|hit| hit.act == StudioRowAct::OpenArchived)
        .expect("the archived row hit")
        .rect;
    app.on_mouse(crate::app::input::MouseInput {
        kind: crate::app::input::MouseEventKind::Down(crate::app::input::MouseButton::Left),
        col: hit.x + 1,
        row: hit.y,
        mods: Mods::NONE,
        sgr_position: None,
    });
    let Some(StudioDialog::Restore(d)) = &app.studio_dialog else {
        panic!("the archived list");
    };
    assert_eq!(d.entries.len(), 1);
    assert_eq!(d.entries[0].requirement.as_deref(), Some("the shop"));

    // Entry → `Restore as old-shop`, renamed to `old-shop-2`.
    key(&mut app, Key::Enter);
    for c in "-2".chars() {
        key(&mut app, Key::Char(c));
    }
    key(&mut app, Key::Enter); // dry run
    drain_dialog_fetch(&mut app).await;
    assert!(
        recorded.lock().unwrap().iter().any(|a| a
            == &argv(&[
                "studio",
                "restore",
                "old-shop-20260101T120000Z",
                "--name",
                "old-shop-2",
                "--dry-run",
                "--as",
                "user"
            ])),
        "the dry run used the chosen name: {:?}",
        recorded.lock().unwrap()
    );
    key(&mut app, Key::Enter); // the real restore
    assert_eq!(
        recorded.lock().unwrap().last().unwrap(),
        &argv(&[
            "studio",
            "restore",
            "old-shop-20260101T120000Z",
            "--name",
            "old-shop-2",
            "--as",
            "user"
        ])
    );
}
