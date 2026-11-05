// Derived from herdr src/layout.rs (Apache-2.0).
use super::*;

#[test]
fn split_paths_preserve_preorder_geometry_and_resize_targets() {
    let ids = [
        PaneId::alloc(),
        PaneId::alloc(),
        PaneId::alloc(),
        PaneId::alloc(),
    ];
    let mut layout = TileLayout::from_saved(
        Node::Split {
            direction: Direction::Horizontal,
            ratio: 0.5,
            first: Box::new(split_node(ids[0], Direction::Vertical, ids[1], 0.5)),
            second: Box::new(split_node(ids[2], Direction::Vertical, ids[3], 0.25)),
        },
        ids[0],
    );
    let area = Rect::new(3, 7, 120, 80);
    let splits = layout.splits(area);
    assert_eq!(splits.len(), 3);
    for (split, (path, direction, pos, rect)) in splits.iter().zip([
        (vec![], Direction::Horizontal, 63, area),
        (
            vec![false],
            Direction::Vertical,
            47,
            Rect::new(3, 7, 60, 80),
        ),
        (
            vec![true],
            Direction::Vertical,
            27,
            Rect::new(63, 7, 60, 80),
        ),
    ]) {
        assert_eq!(
            (split.path.clone(), split.direction, split.pos, split.area),
            (path, direction, pos, rect)
        );
    }
    assert!(layout.set_ratio_at(&splits[2].path, 0.5));
    let resized = layout.splits(area);
    assert_eq!(resized[1].pos, splits[1].pos);
    assert_eq!(resized[2].pos, 47);
}

#[test]
fn rejected_splits_and_insertions_preserve_layout_and_focus_history() {
    let (mut layout, root) = TileLayout::new();
    let second = layout.split_focused(Direction::Horizontal);
    let absent = PaneId::alloc();
    let before = pane_rects(&layout);
    let splits = split_snapshot(&layout);
    assert!(layout
        .split_pane(absent, Direction::Vertical, 0.5)
        .is_none());
    for (target, moved) in [(absent, PaneId::alloc()), (root, second), (root, root)] {
        assert!(!layout.insert_pane_near(target, moved, Direction::Vertical, 0.3, true));
    }
    assert_eq!(pane_rects(&layout), before);
    assert_eq!(split_snapshot(&layout), splits);
    assert_eq!(layout.focused(), second);
    assert_eq!(layout.prev_focus, Some(root));
    assert!(layout.close_focused());
    assert_eq!(layout.focused(), root);
}

#[test]
fn splitting_a_deep_leaf_preserves_other_branches_and_focus() {
    let (mut layout, root) = TileLayout::new();
    let right = layout.split_pane(root, Direction::Horizontal, 0.6).unwrap();
    let bottom_left = layout.split_pane(root, Direction::Vertical, 0.4).unwrap();
    let right_rect = pane_rect(&layout, right);
    let focus = layout.focused();
    let new = layout
        .split_pane(bottom_left, Direction::Horizontal, f32::NAN)
        .unwrap();
    assert_eq!(layout.pane_ids(), [root, bottom_left, new, right]);
    assert_eq!(pane_rect(&layout, right), right_rect);
    assert_eq!(layout.focused(), focus);
    assert_eq!(
        split_snapshot(&layout),
        [
            (Direction::Horizontal, 0.6),
            (Direction::Vertical, 0.4),
            (Direction::Horizontal, 0.5)
        ]
    );
}

#[test]
fn panes_after_split_predicts_the_real_split_geometry() {
    let area = Rect::new(3, 1, 121, 37);
    let (mut layout, root) = TileLayout::new();
    let right = layout.split_pane(root, Direction::Horizontal, 0.6).unwrap();
    let bottom_left = layout.split_pane(root, Direction::Vertical, 0.4).unwrap();
    for (target, direction, ratio) in [
        (bottom_left, Direction::Horizontal, 0.5),
        (right, Direction::Vertical, 0.3),
        (root, Direction::Horizontal, f32::NAN),
    ] {
        let (predicted, new_index) = layout
            .panes_after_split(area, target, direction, ratio)
            .unwrap();
        let new_id = layout.split_pane(target, direction, ratio).unwrap();
        let actual: Vec<_> = layout
            .panes(area)
            .into_iter()
            .map(|info| info.rect)
            .collect();
        assert_eq!(
            predicted.iter().map(|info| info.rect).collect::<Vec<_>>(),
            actual
        );
        assert_eq!(layout.pane_ids()[new_index], new_id);
    }
    assert!(layout
        .panes_after_split(area, PaneId::alloc(), Direction::Vertical, 0.5)
        .is_none());
}

#[test]
#[ignore = "manual BSP allocation and traversal scaling profile"]
fn bsp_layout_profile() {
    use std::hint::black_box;
    use std::time::{Duration, Instant};
    fn build(count: usize, balanced: bool) -> TileLayout {
        let (mut layout, root) = TileLayout::new();
        let mut leaves = std::collections::VecDeque::from([root]);
        for _ in 1..count {
            let target = leaves.pop_front().unwrap();
            let new = layout
                .split_pane(target, Direction::Horizontal, 0.5)
                .unwrap();
            if balanced {
                leaves.push_back(target);
            }
            leaves.push_back(new);
        }
        layout
    }
    fn measure(mut operation: impl FnMut()) -> f64 {
        for _ in 0..32 {
            operation();
        }
        let mut samples = Vec::new();
        for _ in 0..7 {
            let start = Instant::now();
            let mut iterations = 0;
            while start.elapsed() < Duration::from_millis(20) {
                operation();
                iterations += 1;
            }
            samples.push(start.elapsed().as_secs_f64() * 1e6 / f64::from(iterations));
        }
        samples.sort_by(f64::total_cmp);
        samples[3]
    }
    for count in [1, 15, 128, 512] {
        for balanced in [true, false] {
            let layout = build(count, balanced);
            let splits = measure(|| {
                black_box(layout.splits(Rect::new(0, 0, 120, 40)));
            });
            let build = measure(|| {
                black_box(build(count, balanced));
            });
            println!("bsp panes={count} balanced={balanced} splits_us={splits:.3} build_us={build:.3}");
        }
    }
}

fn pane(id: u32) -> PaneId {
    PaneId::from_raw(id)
}

fn sample_layout() -> TileLayout {
    TileLayout::from_saved(
        Node::Split {
            direction: Direction::Horizontal,
            ratio: 0.3,
            first: Box::new(Node::Pane(pane(1))),
            second: Box::new(Node::Split {
                direction: Direction::Vertical,
                ratio: 0.6,
                first: Box::new(Node::Pane(pane(2))),
                second: Box::new(Node::Split {
                    direction: Direction::Horizontal,
                    ratio: 0.4,
                    first: Box::new(Node::Pane(pane(3))),
                    second: Box::new(Node::Pane(pane(4))),
                }),
            }),
        },
        pane(2),
    )
}

fn pane_rects(layout: &TileLayout) -> Vec<(PaneId, Rect)> {
    layout
        .panes(Rect::new(0, 0, 100, 40))
        .into_iter()
        .map(|info| (info.id, info.rect))
        .collect()
}

fn pane_rect(layout: &TileLayout, pane_id: PaneId) -> Rect {
    pane_rects(layout)
        .into_iter()
        .find_map(|(id, rect)| (id == pane_id).then_some(rect))
        .expect("pane should exist")
}

fn split_snapshot(layout: &TileLayout) -> Vec<(Direction, f32)> {
    fn collect(node: &Node, out: &mut Vec<(Direction, f32)>) {
        match node {
            Node::Pane(_) => {}
            Node::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                out.push((*direction, *ratio));
                collect(first, out);
                collect(second, out);
            }
        }
    }

    let mut out = Vec::new();
    collect(layout.root(), &mut out);
    out
}

#[test]
fn swap_panes_exchanges_leaf_ids_without_changing_cells() {
    let mut layout = sample_layout();
    let before_rects = pane_rects(&layout);
    let before_splits = split_snapshot(&layout);

    assert!(layout.swap_panes(pane(2), pane(4)));

    assert_eq!(layout.pane_count(), 4);
    assert_eq!(split_snapshot(&layout), before_splits);
    assert_eq!(layout.focused(), pane(2));

    let after_rects = pane_rects(&layout);
    assert_eq!(after_rects[0], before_rects[0]);
    assert_eq!(after_rects[1], (pane(4), before_rects[1].1));
    assert_eq!(after_rects[2], before_rects[2]);
    assert_eq!(after_rects[3], (pane(2), before_rects[3].1));
}

#[test]
fn swap_panes_is_noop_for_same_or_missing_pane() {
    let mut layout = sample_layout();
    let before_rects = pane_rects(&layout);
    let before_splits = split_snapshot(&layout);
    let before_focus = layout.focused();

    assert!(!layout.swap_panes(pane(2), pane(2)));
    assert!(!layout.swap_panes(pane(2), pane(99)));
    assert!(!layout.swap_panes(pane(99), pane(2)));

    assert_eq!(pane_rects(&layout), before_rects);
    assert_eq!(split_snapshot(&layout), before_splits);
    assert_eq!(layout.focused(), before_focus);
}

#[test]
fn insert_existing_pane_near_target_preserves_existing_ids_and_focuses_moved_pane() {
    let (mut layout, root) = TileLayout::new();
    let moved = pane(99);

    assert!(layout.insert_pane_near(root, moved, Direction::Horizontal, 0.25, true));

    assert_eq!(layout.pane_count(), 2);
    assert_eq!(layout.pane_ids(), vec![root, moved]);
    assert_eq!(layout.focused(), moved);
    let splits = split_snapshot(&layout);
    assert_eq!(splits, vec![(Direction::Horizontal, 0.25)]);
    assert_eq!(pane_rect(&layout, root), Rect::new(0, 0, 25, 40));
    assert_eq!(pane_rect(&layout, moved), Rect::new(25, 0, 75, 40));
}

#[test]
fn split_focused_with_ratio_sets_new_split_ratio() {
    let (mut layout, root) = TileLayout::new();
    layout.focus_pane(root);

    layout.split_focused_with_ratio(Direction::Horizontal, 0.333);

    let splits = split_snapshot(&layout);
    assert_eq!(splits.len(), 1);
    assert_eq!(splits[0].0, Direction::Horizontal);
    assert!((splits[0].1 - 0.333).abs() < f32::EPSILON);
}

#[test]
fn resize_pane_preserves_focus_and_reports_change() {
    let mut layout = sample_layout();
    let original_focus = layout.focused();

    assert!(layout.resize_pane(pane(1), NavDirection::Right, 0.05, Rect::new(0, 0, 100, 40),));

    assert_eq!(layout.focused(), original_focus);
    let split = split_snapshot(&layout)[0];
    assert_eq!(split.0, Direction::Horizontal);
    assert!((split.1 - 0.35).abs() < f32::EPSILON);
}

#[test]
fn resize_second_child_toward_split_decreases_ratio() {
    let (mut layout, root) = TileLayout::new();
    let right = layout.split_focused(Direction::Horizontal);
    layout.focus_pane(root);

    assert!(layout.resize_pane(right, NavDirection::Left, 0.05, Rect::new(0, 0, 100, 40),));

    let split = split_snapshot(&layout)[0];
    assert_eq!(split.0, Direction::Horizontal);
    assert!((split.1 - 0.45).abs() < f32::EPSILON);
    assert_eq!(layout.focused(), root);
}
