//! The pure row model: order, counts, collapse, the title text and
//! the collapsed-mode working rows.
use super::*;
use crate::panel::rows::{GROUPS, Row, Target, rows, title_text};
use crate::panel::{Panel, PanelMode};

fn panel(mode: PanelMode) -> Panel {
    Panel {
        mode,
        ..Panel::new()
    }
}

fn targets(summary: Option<&Summary>, stage: bool, panel: &Panel) -> Vec<Target> {
    rows(summary, stage, panel)
        .iter()
        .map(|r| r.target())
        .collect()
}

#[test]
fn expanded_rows_keep_summary_order_and_counts() {
    let summary = summary();
    let panel = panel(PanelMode::Expanded);
    assert_eq!(
        targets(Some(&summary), true, &panel),
        vec![
            Target::Title,
            Target::Studio("acme-shop".into()),
            Target::Group("acme-shop".into(), "producer".into()),
            Target::Person("acme-shop".into(), "producer".into()),
            Target::Group("acme-shop".into(), "director".into()),
            Target::Person("acme-shop".into(), "director".into()),
            Target::Group("acme-shop".into(), "writer".into()),
            Target::Person("acme-shop".into(), "writer".into()),
            Target::Group("acme-shop".into(), "actors".into()),
            Target::Person("acme-shop".into(), "actor-alice".into()),
            Target::Person("acme-shop".into(), "actor-erin".into()),
            Target::Person("acme-shop".into(), "actor-bob".into()),
            Target::Person("acme-shop".into(), "actor-carol".into()),
            Target::Person("acme-shop".into(), "actor-dave".into()),
        ]
    );
    // The actors group header carries its people count.
    let all = rows(Some(&summary), true, &panel);
    let actors = all.iter().find_map(|r| match r {
        Row::Group {
            group: "actors",
            count,
            collapsed,
            ..
        } => Some((*count, *collapsed)),
        _ => None,
    });
    assert_eq!(actors, Some((5, false)));
    // An empty group is skipped entirely.
    assert!(
        !all.iter().any(|r| matches!(
            r,
            Row::Group {
                group: g,
                ..
            } if !GROUPS.contains(g)
        )),
        "unknown group leaked"
    );
}

#[test]
fn collapsing_one_group_hides_only_its_people() {
    let summary = summary();
    let mut panel = panel(PanelMode::Expanded);
    panel
        .collapsed_groups
        .insert(("acme-shop".into(), "actors".into()));
    let all = rows(Some(&summary), true, &panel);
    let collapsed = all.iter().any(|r| {
        matches!(
            r,
            Row::Group {
                group: "actors",
                collapsed: true,
                count: 5,
                ..
            }
        )
    });
    assert!(collapsed, "actors header should show collapsed");
    assert!(
        !all.iter()
            .any(|r| matches!(r, Row::Person { role, .. } if role.name == "alice")),
        "a collapsed group's people are hidden"
    );
    // The producer's row survives.
    assert!(
        all.iter()
            .any(|r| matches!(r, Row::Person { role, .. } if role.role == "producer"))
    );
}

#[test]
fn collapsed_mode_lists_one_row_per_working_group() {
    let summary = summary();
    assert_eq!(
        targets(Some(&summary), true, &panel(PanelMode::Collapsed)),
        vec![
            Target::Title,
            Target::WorkingGroup("producer".into()),
            Target::WorkingGroup("actors".into()),
        ]
    );
    // The working counts feed `actors 2` style labels at draw time.
    let all = rows(Some(&summary), true, &panel(PanelMode::Collapsed));
    let working = all
        .iter()
        .filter_map(|r| match r {
            Row::WorkingGroup { group, working } => Some((*group, *working)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(working, vec![("producer", 1), ("actors", 2)]);
}

#[test]
fn collapsed_mode_with_nothing_working_shows_idle() {
    let mut value = summary_json();
    for role in value["studios"][0]["roles"].as_array_mut().unwrap() {
        role["work"] = serde_json::Value::Null;
    }
    let summary: Summary = serde_json::from_value(value).unwrap();
    assert_eq!(
        targets(Some(&summary), true, &panel(PanelMode::Collapsed)),
        vec![Target::Title, Target::Idle]
    );
}

#[test]
fn title_names_working_groups_or_idle() {
    let summary = summary();
    assert_eq!(title_text(Some(&summary)), "working: producer, actors 2");
    let mut value = summary_json();
    for role in value["studios"][0]["roles"].as_array_mut().unwrap() {
        role["work"] = serde_json::Value::Null;
    }
    let summary: Summary = serde_json::from_value(value).unwrap();
    assert_eq!(title_text(Some(&summary)), "idle");
    assert_eq!(title_text(None), "idle");
}

#[test]
fn missing_document_is_title_plus_no_studios() {
    assert_eq!(
        targets(None, true, &panel(PanelMode::Expanded)),
        vec![Target::Title, Target::Empty]
    );
}

#[test]
fn a_dead_stage_adds_one_line_and_keeps_the_rows() {
    let summary = summary();
    assert_eq!(
        targets(Some(&summary), false, &panel(PanelMode::Expanded)),
        [
            vec![Target::Title, Target::StageDown],
            targets(Some(&summary), true, &panel(PanelMode::Expanded))[1..].to_vec(),
        ]
        .concat()
    );
}
