//! Closed-fold, done-fold, height-agreement, and 36x40 render tests.

use super::fold_tests::{panel, single_studio_summary, studio_rows, two_studio_summary};
use super::*;
use crate::chrome::palette::Palette;
use crate::panel::config::StudioKeys;
use crate::panel::data::studios;
use crate::panel::rows::{Row, Target, rows};
use crate::panel::rows_fit::{block_height, talk_budgets};
use crate::panel::rows_fold::{BlockState, block_states, folded_block_height};
use crate::panel::{Panel, PanelMode, render};
use crate::studio::StudioKey;
use backlot_model::summary::Summary;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

fn render_lines(
    summary: &Summary,
    panel: &Panel,
    current: Option<&str>,
    width: u16,
    height: u16,
) -> Vec<String> {
    let area = Rect::new(0, 0, width, height);
    let mut backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    let current = current.map(StudioKey::local);
    let list = machines(Some(summary));
    let palette = Palette::from_name("catppuccin").unwrap();
    let keys = StudioKeys::default();
    terminal
        .draw(|frame| {
            render(
                frame.buffer_mut(),
                area,
                &list,
                |_| true,
                |_| true,
                panel,
                panel.mode,
                current.as_ref(),
                current.as_ref(),
                2,
                None,
                &keys,
                None,
                false,
                false,
                true,
                &Default::default(),
                &palette,
                60_000,
            );
        })
        .unwrap();
    backend = terminal.backend().clone();
    let buf = backend.buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

#[test]
fn closed_fold_groups_duplicates_in_first_seen_order_and_fits_width() {
    let summary = single_studio_summary();
    let list = machines(Some(&summary));
    let p = panel(PanelMode::Expanded);

    // Closed seats are hidden behind one fold row; no closed Person rows emit.
    let all = rows(&list, |_| true, |_| true, &p, p.mode, None, 100);
    assert!(
        !all.iter()
            .any(|r| matches!(r, Row::Person { role, .. } if role.state == "closed")),
        "closed roles must fold by default: {all:?}"
    );
    assert!(
        all.iter()
            .any(|r| matches!(r, Row::ClosedFold { open: false, .. })),
        "{all:?}"
    );

    // Wide dock shows full `□ 6 closed · writer actor-a×3 actor-b×2`.
    let wide = render_lines(&summary, &p, Some("acme-shop"), 48, 35).join("\n");
    assert!(
        wide.contains("□ 6 closed · writer actor-a×3 actor-b×2"),
        "expected full closed line:\n{wide}"
    );

    // 36-col dock cuts at a name boundary with `…` (never mid-name).
    let narrow = render_lines(&summary, &p, Some("acme-shop"), 36, 35).join("\n");
    let closed_row = narrow
        .lines()
        .find(|l| l.contains("□ 6 closed"))
        .unwrap_or_else(|| panic!("missing closed fold row:\n{narrow}"));
    assert!(closed_row.contains("writer actor-a×3"), "{closed_row:?}");
    assert!(closed_row.contains("…"), "{closed_row:?}");
    assert!(!closed_row.contains("actor-b"), "{closed_row:?}");

    // 0 closed roles -> no closed line.
    let mut no_closed = single_studio_summary();
    no_closed.studios[0].roles.retain(|r| r.state != "closed");
    let out = render_lines(&no_closed, &p, Some("acme-shop"), 48, 35).join("\n");
    assert!(!out.contains("closed"), "{out}");
}

#[test]
fn done_fold_triggers_at_two_and_not_at_one() {
    let p = panel(PanelMode::Expanded);

    // 1 done task -> task row directly, no fold line.
    let mut one_done = single_studio_summary();
    one_done.studios[0]
        .tasks
        .retain(|t| t.id == "t1" || t.id == "t4");
    let one_list = machines(Some(&one_done));
    let one_rows = rows(&one_list, |_| true, |_| true, &p, p.mode, None, 100);
    assert!(
        !one_rows
            .iter()
            .any(|r| matches!(r, Row::AcceptedFold { .. })),
        "1 done task must not fold: {one_rows:?}"
    );
    assert!(
        one_rows
            .iter()
            .any(|r| matches!(r, Row::Task { task, .. } if task.id == "t1")),
        "1 done task renders as Task row"
    );

    // 2 done tasks -> folds to `✓ 2 done · t1 t2`.
    let mut two_done = single_studio_summary();
    two_done.studios[0].tasks.retain(|t| t.id != "t3");
    let two_list = machines(Some(&two_done));
    let two_rows = rows(&two_list, |_| true, |_| true, &p, p.mode, None, 100);
    assert!(
        two_rows
            .iter()
            .any(|r| matches!(r, Row::AcceptedFold { open: false, .. })),
        "2 done tasks must fold: {two_rows:?}"
    );
    let two_text = render_lines(&two_done, &p, Some("acme-shop"), 36, 35).join("\n");
    assert!(two_text.contains("✓ 2 done · t1 t2"), "{two_text}");

    // 3 done tasks -> `✓ 3 done · t1 t2 t3`.
    let three_text =
        render_lines(&single_studio_summary(), &p, Some("acme-shop"), 36, 35).join("\n");
    assert!(three_text.contains("✓ 3 done · t1 t2 t3"), "{three_text}");
}

#[test]
fn height_functions_agree_with_emitted_rows() {
    let summary = two_studio_summary();
    let list = machines(Some(&summary));
    let refs = studios(&list);
    let mut p = panel(PanelMode::Expanded);
    p.selected = Some(Target::Studio("acme-shop".into()));

    for body_height in [12u16, 17, 25, 100] {
        for open_folds in [false, true] {
            if open_folds {
                p.accepted_folds.insert("acme-shop".into());
                p.closed_folds.insert("acme-shop".into());
            } else {
                p.accepted_folds.clear();
                p.closed_folds.clear();
            }
            let owed = |_: crate::panel::data::StudioRef<'_>| true;
            let states = block_states(&refs, &p, None, body_height, &owed);
            let budgets = talk_budgets(&refs, &states, &p, None, body_height, 0, &owed);
            let all = rows(&list, |_| true, |_| true, &p, p.mode, None, body_height);

            for studio in &refs {
                let key = studio.key();
                let emitted = studio_rows(&all, &studio.studio.id);
                let talk_count = emitted
                    .iter()
                    .filter(|r| matches!(r, Row::Talk { .. }))
                    .count();
                assert_eq!(talk_count, budgets.get(&key).copied().unwrap_or(0));
                match states[&key] {
                    BlockState::Full => {
                        let expected = block_height(
                            *studio,
                            &p,
                            owed(*studio),
                            p.accepted_open(&key),
                            p.closed_open(&key),
                        ) + talk_count;
                        assert_eq!(
                            emitted.len(),
                            expected,
                            "body={body_height}, open={open_folds}"
                        );
                    }
                    BlockState::Folded => {
                        assert_eq!(
                            emitted.len(),
                            folded_block_height(studio.studio),
                            "body={body_height}, open={open_folds}"
                        );
                    }
                    BlockState::Stacked => {
                        assert_eq!(emitted.len(), 1, "body={body_height}, open={open_folds}");
                    }
                }
            }
        }
    }
}

#[test]
fn screenshot_case_renders_selected_full_and_unselected_folded_at_36x40() {
    let summary = two_studio_summary();
    let mut p = panel(PanelMode::Expanded);
    p.focused = true;
    p.selected = Some(Target::Input("acme-shop".into()));
    let lines = render_lines(&summary, &p, Some("acme-shop"), 36, 40);
    let text = lines.join("\n");

    // Selected studio `acme-shop`: full block with live role, done fold, closed fold, talk, and `›`.
    assert!(text.contains("✓ 3 done · t1 t2 t3"), "{text}");
    assert!(text.contains("□ 6 closed · writer actor-a×3"), "{text}");
    assert!(text.contains("actor-a working on t4"), "{text}");
    assert!(text.contains("tell producer"), "{text}");

    // Unselected studio `blog`: header with `!1`, live roles with `needs you`, no owed/talk/input.
    assert!(text.contains("!1"), "{text}");
    assert!(text.contains("needs you"), "{text}");
    assert!(!text.contains("review t1"), "{text}");
    assert!(!text.contains("start blog"), "{text}");
    assert!(!text.contains("actor-a×1"), "{text}");

    if let Some(out_path) = std::env::var_os("LOT_FOLD_SHOT_OUT") {
        if let Some(parent) = std::path::Path::new(&out_path).parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&out_path, format!("{text}\n")).unwrap();
    }
}
