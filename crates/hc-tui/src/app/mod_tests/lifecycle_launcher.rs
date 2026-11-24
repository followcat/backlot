//! W4b part 4 — the launcher's `lifecycle` group: the new actions
//! appear under `Ctrl+B ?` and the filter reaches them.

use std::time::Instant;

use super::lifecycle_fixtures::*;

#[tokio::test]
async fn the_launcher_lists_the_lifecycle_actions() {
    let mut app = lifecycle_app();
    app.studio_index
        .note_archived_reply(None, Instant::now(), Ok(ARCHIVED_JSON));
    let labels: Vec<String> = app
        .help_groups()
        .into_iter()
        .flat_map(|g| g.rows)
        .map(|r| r.label)
        .collect();
    for want in [
        "Restart director · acme-shop",
        "Recast director · acme-shop",
        "Free work areas · old-shop",
        "Archived studios · Local",
    ] {
        assert!(
            labels.iter().any(|l| l == want),
            "{want} missing from {labels:?}"
        );
    }
    // The filter reaches them by name.
    app.help_state.query = crate::chrome::text_editor::TextEditor::new("archived", false);
    let filtered: Vec<String> = app
        .help_filtered_groups()
        .into_iter()
        .flat_map(|g| g.rows)
        .map(|r| r.label)
        .collect();
    assert!(
        filtered.iter().any(|l| l == "Archived studios · Local"),
        "{filtered:?}"
    );
}
