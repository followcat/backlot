//! Lot folding, closed fold, done fold, and row-selection tests.
//! Render and height-agreement tests live in `fold_render_tests.rs`.
//! Neutral fixtures only (`acme-shop`, `blog`, `producer`, `director`,
//! `writer`, `actor-a`, `actor-b`).

use super::*;
use crate::panel::rows::{Row, Target, rows};
use crate::panel::{Panel, PanelMode};
use crate::studio::StudioKey;
use backlot_model::summary::Summary;

pub(super) fn panel(mode: PanelMode) -> Panel {
    Panel {
        mode,
        ..Panel::new()
    }
}

pub(super) fn two_studio_summary() -> Summary {
    serde_json::from_value(serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 60_000,
        "studios": [
            {
                "id": "acme-shop",
                "title": "Acme shop",
                "state": "live",
                "requirement_version": 2,
                "mode": "standard",
                "open_reviews": 0,
                "waiting": 0,
                "tasks_done": 3,
                "tasks_total": 4,
                "next": "deliver t4 — actor-a",
                "usage": {
                    "input_tokens": 12_400,
                    "output_tokens": 3_100,
                    "cost_micros_usd": 80_000
                },
                "roles": [
                    { "role": "producer", "name": "producer", "terminal": "t1", "runner": "claude", "model": "sonnet", "state": "live", "work": "waiting" },
                    { "role": "director", "name": "director", "terminal": "t2", "runner": "claude", "model": "sonnet", "state": "live", "work": "waiting" },
                    { "role": "actor-t4-c1", "name": "actor-a", "terminal": "t3", "runner": "claude", "model": "sonnet", "state": "live", "work": "working", "task": "t4", "doing": "building checkout", "since_ms": 30_000 },
                    { "role": "writer-c1", "name": "writer", "state": "closed", "resumable": true },
                    { "role": "actor-t1-c1", "name": "actor-a", "state": "closed", "resumable": true },
                    { "role": "actor-t2-c1", "name": "actor-b", "state": "closed", "resumable": true },
                    { "role": "actor-t1-c2", "name": "actor-a", "state": "closed", "resumable": false },
                    { "role": "actor-t3-c1", "name": "actor-a", "state": "closed", "resumable": true },
                    { "role": "actor-t3-c2", "name": "actor-b", "state": "closed", "resumable": false }
                ],
                "tasks": [
                    { "id": "t1", "title": "First", "status": "accepted", "requirement_version": 2, "priority": "normal", "rework_rounds": 0, "rework_cap": 0 },
                    { "id": "t2", "title": "Second", "status": "accepted", "requirement_version": 2, "priority": "normal", "rework_rounds": 0, "rework_cap": 0 },
                    { "id": "t3", "title": "Third", "status": "accepted", "requirement_version": 2, "priority": "normal", "rework_rounds": 0, "rework_cap": 0 },
                    { "id": "t4", "title": "Fourth", "status": "running", "requirement_version": 2, "priority": "normal", "rework_rounds": 0, "rework_cap": 0, "actor_name": "actor-a" }
                ],
                "exchange": [
                    { "seq": 1, "who": "user", "text": "build the checkout flow" },
                    { "seq": 2, "who": "producer", "text": "casting t1 through t4" },
                    { "seq": 3, "who": "user", "text": "focus on tests first" },
                    { "seq": 4, "who": "producer", "text": "t1..t3 accepted by director" },
                    { "seq": 5, "who": "producer", "text": "actor-a working on t4" }
                ]
            },
            {
                "id": "blog",
                "title": "Blog",
                "state": "live",
                "requirement_version": 1,
                "mode": "standard",
                "open_reviews": 1,
                "waiting": 1,
                "tasks_done": 1,
                "tasks_total": 2,
                "next": "review t1 — director",
                "usage": {
                    "input_tokens": 4_200,
                    "output_tokens": 900,
                    "cost_micros_usd": 20_000
                },
                "roles": [
                    { "role": "producer", "name": "producer", "terminal": "t4", "runner": "claude", "model": "sonnet", "state": "live", "work": "waiting" },
                    { "role": "director", "name": "director", "terminal": "t5", "runner": "claude", "model": "sonnet", "state": "live", "work": "blocked", "attention": "owed", "attention_class": "needs_you", "attention_user": false, "attention_detail": "answer review" },
                    { "role": "writer", "name": "writer", "state": "standby", "runner": "claude", "model": "sonnet" },
                    { "role": "actor-t1-c1", "name": "actor-b", "terminal": "t6", "runner": "claude", "model": "sonnet", "state": "live", "work": "working", "task": "t1", "doing": "writing draft", "since_ms": 40_000 },
                    { "role": "actor-t0-c1", "name": "actor-a", "state": "closed", "resumable": true }
                ],
                "tasks": [
                    { "id": "t0", "title": "Init", "status": "accepted", "requirement_version": 1, "priority": "normal", "rework_rounds": 0, "rework_cap": 0 },
                    { "id": "t1", "title": "Post", "status": "delivered", "requirement_version": 1, "priority": "normal", "rework_rounds": 0, "rework_cap": 0 }
                ],
                "exchange": [
                    { "seq": 1, "who": "user", "text": "start blog" },
                    { "seq": 2, "who": "producer", "text": "director reviewing t1" }
                ]
            }
        ]
    }))
    .unwrap()
}

pub(super) fn single_studio_summary() -> Summary {
    let mut s = two_studio_summary();
    s.studios.truncate(1);
    s
}

pub(super) fn studio_rows<'a>(all: &'a [Row<'a>], studio_id: &str) -> Vec<&'a Row<'a>> {
    let mut out = Vec::new();
    let mut inside = false;
    for row in all {
        match row {
            Row::BlockTop { studio, .. } if studio.studio.id == studio_id => {
                inside = true;
                out.push(row);
            }
            Row::BlockBottom(studio) if inside && studio.studio.id == studio_id => {
                out.push(row);
                break;
            }
            Row::Stacked(studio) if studio.studio.id == studio_id => {
                out.push(row);
                break;
            }
            _ if inside => out.push(row),
            _ => {}
        }
    }
    out
}

#[test]
fn cursor_in_first_studio_folds_second_to_header_live_roles_and_bottom() {
    let summary = two_studio_summary();
    let list = machines(Some(&summary));
    let mut p = panel(PanelMode::Expanded);
    p.selected = Some(Target::Person("acme-shop".into(), "producer".into()));

    let all = rows(&list, |_| true, |_| true, &p, p.mode, None, 100);
    let blog = studio_rows(&all, "blog");
    let tags: Vec<&str> = blog
        .iter()
        .map(|r| match r {
            Row::BlockTop { .. } => "BlockTop",
            Row::Person { .. } => "Person",
            Row::BlockBottom(_) => "BlockBottom",
            Row::Owed { .. } => "Owed",
            Row::Usage { .. } => "Usage",
            Row::Task { .. } => "Task",
            Row::AcceptedFold { .. } => "AcceptedFold",
            Row::Talk { .. } => "Talk",
            Row::Input { .. } => "Input",
            _ => "Other",
        })
        .collect();
    assert_eq!(
        tags,
        vec![
            "BlockTop",
            "Person",
            "Person",
            "Person",
            "Person",
            "BlockBottom"
        ],
        "unselected blog must fold to header + 4 live roles + bottom: {blog:?}"
    );
    let blog_roles: Vec<&str> = blog
        .iter()
        .filter_map(|r| match r {
            Row::Person { role, .. } => Some(role.role.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        blog_roles,
        vec!["producer", "director", "writer", "actor-t1-c1"]
    );
    let acme = studio_rows(&all, "acme-shop");
    assert!(
        acme.iter().any(|r| matches!(r, Row::Input { .. })),
        "{acme:?}"
    );
    assert!(
        acme.iter().any(|r| matches!(r, Row::Talk { .. })),
        "{acme:?}"
    );
    assert!(
        acme.iter()
            .any(|r| matches!(r, Row::ClosedFold { open: false, .. })),
        "{acme:?}"
    );
}

#[test]
fn moving_cursor_to_second_studio_folds_first_and_unfolds_second() {
    let summary = two_studio_summary();
    let list = machines(Some(&summary));
    let mut p = panel(PanelMode::Expanded);
    p.selected = Some(Target::Studio("blog".into()));

    let all = rows(&list, |_| true, |_| true, &p, p.mode, None, 100);
    let acme = studio_rows(&all, "acme-shop");
    let acme_tags: Vec<&str> = acme
        .iter()
        .map(|r| match r {
            Row::BlockTop { .. } => "BlockTop",
            Row::Person { .. } => "Person",
            Row::BlockBottom(_) => "BlockBottom",
            _ => "Other",
        })
        .collect();
    assert_eq!(
        acme_tags,
        vec!["BlockTop", "Person", "Person", "Person", "BlockBottom"],
        "acme-shop must fold when cursor is in blog: {acme:?}"
    );
    let blog = studio_rows(&all, "blog");
    assert!(
        blog.iter().any(|r| matches!(r, Row::Input { .. })),
        "{blog:?}"
    );
    assert!(
        blog.iter().any(|r| matches!(r, Row::Talk { .. })),
        "{blog:?}"
    );
    assert!(
        blog.iter().any(|r| matches!(r, Row::Person { .. })),
        "unfolded blog keeps Person rows: {blog:?}"
    );
}

#[test]
fn single_studio_never_folds() {
    let summary = single_studio_summary();
    let list = machines(Some(&summary));
    let p = panel(PanelMode::Expanded);

    let all = rows(&list, |_| true, |_| true, &p, p.mode, None, 100);
    let acme = studio_rows(&all, "acme-shop");
    assert!(
        acme.iter().any(|r| matches!(r, Row::Owed { .. })),
        "{acme:?}"
    );
    assert!(
        acme.iter().any(|r| matches!(r, Row::Input { .. })),
        "{acme:?}"
    );
    assert!(
        acme.iter().any(|r| matches!(r, Row::Talk { .. })),
        "{acme:?}"
    );
}

#[test]
fn no_cursor_keeps_current_studio_full_and_first_when_neither() {
    let summary = two_studio_summary();
    let list = machines(Some(&summary));
    let p = panel(PanelMode::Expanded);
    let cur = StudioKey::local("blog");

    // No cursor, `▸` is blog -> blog is full, acme-shop is folded.
    let all = rows(&list, |_| true, |_| true, &p, p.mode, Some(&cur), 100);
    assert!(
        studio_rows(&all, "blog")
            .iter()
            .any(|r| matches!(r, Row::Input { .. }))
    );
    assert!(
        !studio_rows(&all, "acme-shop")
            .iter()
            .any(|r| matches!(r, Row::Input { .. }))
    );

    // Neither cursor nor current -> first studio (acme-shop) is full, blog is folded.
    let all = rows(&list, |_| true, |_| true, &p, p.mode, None, 100);
    assert!(
        studio_rows(&all, "acme-shop")
            .iter()
            .any(|r| matches!(r, Row::Input { .. }))
    );
    assert!(
        !studio_rows(&all, "blog")
            .iter()
            .any(|r| matches!(r, Row::Input { .. }))
    );
}

#[test]
fn manual_expand_collapse_and_cramped_stacking_win() {
    let summary = two_studio_summary();
    let list = machines(Some(&summary));

    // Manual expand keeps unselected blog full, but talk stays on selected acme-shop only.
    let mut p = panel(PanelMode::Expanded);
    p.selected = Some(Target::Studio("acme-shop".into()));
    p.expanded_blocks.insert("blog".into());
    let all = rows(&list, |_| true, |_| true, &p, p.mode, None, 100);
    let blog = studio_rows(&all, "blog");
    assert!(
        blog.iter().any(|r| matches!(r, Row::Input { .. })),
        "{blog:?}"
    );
    assert!(
        !blog.iter().any(|r| matches!(r, Row::Talk { .. })),
        "talk budgets go to selected block only: {blog:?}"
    );

    // Manual collapse keeps studio as a one-line Stacked row.
    let mut p = panel(PanelMode::Expanded);
    p.selected = Some(Target::Studio("acme-shop".into()));
    p.collapsed_blocks.insert("blog".into());
    let all = rows(&list, |_| true, |_| true, &p, p.mode, None, 100);
    let blog = studio_rows(&all, "blog");
    assert_eq!(blog.len(), 1);
    assert!(matches!(blog[0], Row::Stacked(_)), "{blog:?}");

    // When even folded blocks don't fit body_height, unselected studios fall back to Stacked.
    let mut p = panel(PanelMode::Expanded);
    p.selected = Some(Target::Studio("acme-shop".into()));
    let all = rows(&list, |_| true, |_| true, &p, p.mode, None, 12);
    let blog = studio_rows(&all, "blog");
    assert_eq!(blog.len(), 1);
    assert!(matches!(blog[0], Row::Stacked(_)), "{blog:?}");
}
