use super::*;

fn clock(origin: &str, epoch: &str, scope: u64, n: u64) -> Option<Clock> {
    Some(Clock {
        domain: (origin.into(), epoch.into(), scope),
        ordinal: Some(n),
    })
}

#[test]
fn core_order_wins_over_arrival_and_vector_order_within_one_domain() {
    let mut order = ActivityOrder::default();
    order.ranks(vec![("newer".into(), clock("core-a", "boot", 1, 500))]);
    let ranks = order.ranks(vec![
        ("newer".into(), clock("core-a", "boot", 1, 500)),
        ("late-older".into(), clock("core-a", "boot", 1, 3)),
    ]);
    assert!(ranks["newer"] > ranks["late-older"]);
}

#[test]
fn counters_from_other_origins_epochs_or_scopes_are_not_global() {
    for (origin, epoch, scope) in [
        ("core-b", "boot", 1),
        ("core-a", "new-boot", 1),
        ("core-a", "boot", 2),
    ] {
        let mut order = ActivityOrder::default();
        order.ranks(vec![(
            "huge-old".into(),
            clock("core-a", "boot", 1, u64::MAX),
        )]);
        let ranks = order.ranks(vec![
            ("huge-old".into(), clock("core-a", "boot", 1, u64::MAX)),
            ("fresh".into(), clock(origin, epoch, scope, 1)),
        ]);
        assert!(ranks["fresh"] > ranks["huge-old"]);
    }
}

#[test]
fn merging_incomparable_receipts_and_origin_constraints_has_no_comparator_cycle() {
    let mut order = ActivityOrder::default();
    order.ranks(vec![("a-newer".into(), clock("a", "boot", 1, 20))]);
    order.ranks(vec![
        ("a-newer".into(), clock("a", "boot", 1, 20)),
        ("b".into(), clock("b", "boot", 1, 1)),
    ]);
    let ranks = order.ranks(vec![
        ("a-newer".into(), clock("a", "boot", 1, 20)),
        ("a-late".into(), clock("a", "boot", 1, 10)),
        ("b".into(), clock("b", "boot", 1, 1)),
    ]);
    assert!(ranks["a-newer"] > ranks["a-late"]);
    assert_eq!(
        ranks
            .values()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
    let same = order.ranks(vec![
        ("b".into(), clock("b", "boot", 1, 1)),
        ("a-late".into(), clock("a", "boot", 1, 10)),
        ("a-newer".into(), clock("a", "boot", 1, 20)),
    ]);
    assert_eq!(ranks, same);
}

#[test]
fn unknown_history_and_receipt_exhaustion_keep_deterministic_ties() {
    let mut order = ActivityOrder {
        receipt: u64::MAX,
        ..Default::default()
    };
    let ranks = order.ranks(vec![("b".into(), None), ("a".into(), None)]);
    assert!(ranks["b"] > ranks["a"]);
    assert_eq!(
        ranks,
        order.ranks(vec![("a".into(), None), ("b".into(), None)])
    );
}

#[test]
fn lifetime_reset_keeps_receipt_baseline_until_a_fresh_clock_change() {
    let mut order = ActivityOrder::default();
    order.ranks(vec![("a".into(), clock("a", "old", 1, 100))]);
    order.ranks(vec![
        ("a".into(), clock("a", "old", 1, 100)),
        ("b".into(), clock("b", "boot", 1, 5)),
    ]);
    let reset = order.ranks(vec![
        ("a".into(), clock("a", "new", 2, 100)),
        ("b".into(), clock("b", "boot", 1, 5)),
    ]);
    assert!(reset["b"] > reset["a"]);
    let fresh = order.ranks(vec![
        ("a".into(), clock("a", "new", 2, 101)),
        ("b".into(), clock("b", "boot", 1, 5)),
    ]);
    assert!(fresh["a"] > fresh["b"]);
}
