//! The U1 row model: bordered blocks, dividers between groups only,
//! stacking when the blocks stop fitting, manual overrides and the
//! collapsed strip.
use super::*;
use crate::panel::rows::{Mark, Target, mark_counts, rows};
use crate::panel::{Panel, PanelMode};

fn panel(mode: PanelMode) -> Panel {
    Panel {
        mode,
        ..Panel::new()
    }
}

fn targets(
    summary: Option<&Summary>,
    stage: bool,
    panel: &Panel,
    current: Option<&str>,
    body_height: u16,
) -> Vec<Target> {
    rows(summary, stage, panel, current, body_height)
        .iter()
        .map(|r| r.target())
        .collect()
}

fn people(targets: &[Target]) -> Vec<String> {
    targets
        .iter()
        .filter_map(|t| match t {
            Target::Person(_, role) => Some(role.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn expanded_block_lists_people_with_frame_rows_and_group_dividers() {
    let summary = summary();
    let t = targets(
        Some(&summary),
        true,
        &panel(PanelMode::Expanded),
        Some("acme-shop"),
        100,
    );
    assert_eq!(t.first(), Some(&Target::Title));
    // Studio targets: top border + 3 dividers (4 non-empty groups) +
    // bottom border = 5.
    assert_eq!(
        t.iter()
            .filter(|x| **x == Target::Studio("acme-shop".into()))
            .count(),
        5,
        "{t:?}"
    );
    assert_eq!(
        people(&t),
        vec![
            "producer",
            "director",
            "writer",
            "actor-alice",
            "actor-erin",
            "actor-bob",
            "actor-carol",
            "actor-dave"
        ]
    );
    // A person's extra line shares the Person target — attention.
    let mut value = summary_json();
    value["studios"][0]["roles"][3]["attention"] = serde_json::json!("needs permission");
    let summary: Summary = serde_json::from_value(value).unwrap();
    let t2 = targets(
        Some(&summary),
        true,
        &panel(PanelMode::Expanded),
        Some("acme-shop"),
        100,
    );
    assert_eq!(
        t2.iter()
            .filter(|x| **x == Target::Person("acme-shop".into(), "actor-alice".into()))
            .count(),
        2,
        "attention adds a second row with the same target: {t2:?}"
    );
}

#[test]
fn stacking_kicks_in_when_blocks_do_not_fit_and_keeps_current_open() {
    let summary = summary();
    // Body too short for the expanded block → only the current studio
    // stays expanded, everything else is a `╶` line.
    let t = targets(
        Some(&summary),
        true,
        &panel(PanelMode::Expanded),
        Some("acme-shop"),
        5,
    );
    assert!(
        t.iter().any(|x| matches!(x, Target::Person(..))),
        "current stays expanded: {t:?}"
    );
    // No current → the block stacks to a single Block row.
    let t = targets(Some(&summary), true, &panel(PanelMode::Expanded), None, 5);
    assert_eq!(
        t,
        vec![Target::Title, Target::Block("acme-shop".into())],
        "{t:?}"
    );
}

#[test]
fn space_overrides_the_automatic_block_state() {
    let summary = summary();
    // Manual collapse wins over the everything-fits expansion.
    let mut p = panel(PanelMode::Expanded);
    p.collapsed_blocks.insert("acme-shop".into());
    let t = targets(Some(&summary), true, &p, Some("acme-shop"), 100);
    assert_eq!(t, vec![Target::Title, Target::Block("acme-shop".into())]);
    // Manual expand wins over the stacking collapse.
    let mut p = panel(PanelMode::Expanded);
    p.expanded_blocks.insert("acme-shop".into());
    let t = targets(Some(&summary), true, &p, None, 5);
    assert!(
        t.iter().any(|x| matches!(x, Target::Person(..))),
        "manual expand reopens the block: {t:?}"
    );
}

#[test]
fn collapsed_strip_lists_one_row_per_studio() {
    let summary = summary();
    assert_eq!(
        targets(Some(&summary), true, &panel(PanelMode::Collapsed), None, 0),
        vec![Target::Title, Target::Strip("acme-shop".into())]
    );
}

#[test]
fn mark_counts_follow_the_mark_order() {
    let summary = summary();
    let counts = mark_counts(&summary.studios[0]);
    assert_eq!(
        counts,
        vec![
            (Mark::Working, 3),
            (Mark::Waiting, 2),
            (Mark::Blocked, 1),
            (Mark::Lost, 1),
            (Mark::Closed, 1),
        ]
    );
}

#[test]
fn missing_document_is_title_plus_no_studios() {
    assert_eq!(
        targets(None, true, &panel(PanelMode::Expanded), None, 0),
        vec![Target::Title, Target::Empty]
    );
}

#[test]
fn a_dead_stage_adds_one_line_and_keeps_the_rows() {
    let summary = summary();
    let alive = targets(
        Some(&summary),
        true,
        &panel(PanelMode::Expanded),
        Some("acme-shop"),
        100,
    );
    assert_eq!(
        targets(
            Some(&summary),
            false,
            &panel(PanelMode::Expanded),
            Some("acme-shop"),
            100,
        ),
        [vec![Target::Title, Target::StageDown], alive[1..].to_vec()].concat()
    );
}
