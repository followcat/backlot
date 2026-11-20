//! The U1 row model: bordered blocks, dividers between groups only,
//! stacking when the blocks stop fitting, manual overrides and the
//! collapsed strip.
use super::*;
use crate::panel::rows::{Target, rows};
use crate::panel::rows_fit::mark_counts;
use crate::panel::{Panel, PanelMode};
use crate::studio::StudioKey;
use crate::studio::glyphs::Person;

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
    let current = current.map(StudioKey::local);
    rows(
        &machines(summary),
        |_| stage,
        |_| true,
        panel,
        panel.mode,
        current.as_ref(),
        body_height,
    )
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
    assert_eq!(t.first(), Some(&Target::None));
    // The top border is the only `Studio` row — the title and the
    // bottom border are the `Target::None` rows (A2.2); S2 dropped
    // the group dividers.
    assert_eq!(
        t.iter()
            .filter(|x| **x == Target::Studio("acme-shop".into()))
            .count(),
        1,
        "{t:?}"
    );
    assert_eq!(
        t.iter().filter(|x| **x == Target::None).count(),
        2,
        "title + bottom: {t:?}"
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
        vec![Target::None, Target::Block("acme-shop".into())],
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
    assert_eq!(t, vec![Target::None, Target::Block("acme-shop".into())]);
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
fn collapsed_strip_lists_the_studio_and_its_busy_roles() {
    let summary = summary();
    let t = targets(Some(&summary), true, &panel(PanelMode::Collapsed), None, 0);
    // Every strip row — the studio line and its active/blocked role
    // lines — shares the studio's Strip target (A4.3). The fixture's
    // busy roles: producer (working), alice (attention), bob
    // (dir-prompt) and erin (blocked upstream? — whoever works).
    assert_eq!(t[0], Target::None);
    assert!(
        t[1..]
            .iter()
            .all(|x| *x == Target::Strip("acme-shop".into())),
        "{t:?}"
    );
    assert!(t.len() > 2, "the strip is never blank: {t:?}");
}

#[test]
fn mark_counts_follow_the_mark_order() {
    let summary = summary();
    let counts = mark_counts(&summary.studios[0]);
    assert_eq!(
        counts,
        vec![
            (Person::DirPrompt, 1),
            (Person::Lost, 1),
            (Person::Working, 3),
            (Person::Idle, 2),
            (Person::Resumable, 1),
        ]
    );
}

#[test]
fn missing_document_is_title_plus_no_studios() {
    assert_eq!(
        targets(None, true, &panel(PanelMode::Expanded), None, 0),
        vec![Target::None, Target::None]
    );
}

#[test]
fn a_dead_stage_moves_the_stage_word_inside_the_block() {
    let summary = summary();
    let alive = targets(
        Some(&summary),
        true,
        &panel(PanelMode::Expanded),
        Some("acme-shop"),
        100,
    );
    let dead = targets(
        Some(&summary),
        false,
        &panel(PanelMode::Expanded),
        Some("acme-shop"),
        100,
    );
    // No top `stage not running` row — the owed subheader inside the
    // block carries it now (S2 §2): one extra row under the block's
    // top border — the clickable `Stage` target U6c gave it — every
    // other row untouched.
    assert_eq!(dead.len(), alive.len() + 1, "{dead:?}");
    assert_eq!(dead[0], Target::None);
    assert_eq!(dead[1], alive[1]);
    assert_eq!(
        dead[2],
        Target::Stage(None),
        "the owed line joins: {dead:?}"
    );
    assert_eq!(&dead[3..], &alive[2..]);
}
