use super::*;
use crate::terminal::Terminal;

fn term(id: &str, origin: &str, n: u64) -> Terminal {
    serde_json::from_value(serde_json::json!({
        "address":id,"labels":{},"argv":["fixture"],"cwd":"/home/user/acme-shop",
        "state":"live","pid":100,"created_ms":1,"screen":"exact","cols":80,"rows":24,
        "reported":true,"status":{"state":"idle","app":"fixture"},
        "activity":{"core_id":origin,"epoch":"boot","scope":1,"revision":n,
            "records":{"":{"state_ordinal":n,"completion_ordinal":null}}}
    }))
    .unwrap()
}

fn ingest(receipts: &mut ActivityReceipts, terms: Vec<Terminal>) {
    let mut inventory = Inventory::default();
    inventory.replace(1, terms);
    receipts.ingest(&inventory, &[]);
}

fn ingest_with_traversal(
    receipts: &mut ActivityReceipts,
    terms: Vec<Terminal>,
    traversal: &[&str],
) {
    let mut inventory = Inventory::default();
    inventory.replace(1, terms);
    let traversal: Vec<String> = traversal.iter().map(|a| (*a).to_string()).collect();
    receipts.ingest(&inventory, &traversal);
}

fn term_named(id: &str) -> Terminal {
    serde_json::from_value(serde_json::json!({
        "address":id,"labels":{},"argv":["fixture"],"cwd":"/home/user/acme-shop",
        "state":"live","pid":100,"created_ms":1,"screen":"exact","cols":80,"rows":24,
        "reported":true,"status":{"state":"idle","app":"fixture"},
        "records":{"build":{"state":"idle","app":"build"}},
        "activity":{"core_id":"a","epoch":"boot","scope":1,"revision":1,
            "records":{"":{"state_ordinal":30,"completion_ordinal":30},
                "build":{"state_ordinal":20,"completion_ordinal":20}}}
    }))
    .unwrap()
}

#[test]
fn every_domain_boundary_reallocates_the_display_receipt() {
    for boundary in 0..3 {
        let mut receipts = ActivityReceipts::default();
        let a = term("a", "a", 100);
        let b = term("b", "b", 5);
        ingest(&mut receipts, vec![a.clone(), b.clone()]);
        let mut a = a;
        match boundary {
            0 => a.activity.as_mut().unwrap().epoch = "new-boot".into(),
            1 => a.activity.as_mut().unwrap().scope = 2,
            _ => a.activity.as_mut().unwrap().core_id = "other".into(),
        }
        ingest(&mut receipts, vec![a.clone(), b.clone()]);
        // A genuine boundary is a replacement: the retired domain's rank is
        // not preserved; the agent draws a fresh receipt above the current
        // maximum, like any other changed fact.
        assert_eq!(
            (receipts.rank("a"), receipts.rank("b")),
            (3, 2),
            "{boundary}"
        );
        a.activity
            .as_mut()
            .unwrap()
            .records
            .get_mut("")
            .unwrap()
            .state_ordinal = Some(101);
        ingest(&mut receipts, vec![a, b]);
        assert_eq!(receipts.rank("a"), 4, "{boundary}");
    }
}

#[test]
fn activity_loss_and_regain_are_boundaries_too() {
    let mut receipts = ActivityReceipts::default();
    let a = term("a", "a", 100);
    ingest(&mut receipts, vec![a.clone()]);
    let mut lost = a.clone();
    lost.activity = None;
    ingest(&mut receipts, vec![lost]);
    assert_eq!(receipts.rank("a"), 2);
    let mut regained = a.clone();
    regained.activity.as_mut().unwrap().epoch = "new-boot".into();
    ingest(&mut receipts, vec![regained]);
    assert_eq!(receipts.rank("a"), 3);
}

#[test]
fn simultaneous_boundaries_sort_only_comparable_new_domain_slots() {
    let mut receipts = ActivityReceipts::default();
    let a = term("a", "a", 50);
    let b = term("b", "b", 1);
    let c = term("c", "a", 10);
    ingest(&mut receipts, vec![a.clone(), b.clone(), c.clone()]);
    // First sight already sorts the comparable pair into their own slots.
    assert_eq!(
        (receipts.rank("a"), receipts.rank("b"), receipts.rank("c")),
        (3, 2, 1)
    );
    let mut a = a;
    let mut b = b;
    let mut c = c;
    a.activity.as_mut().unwrap().epoch = "boot-2".into();
    b.activity.as_mut().unwrap().epoch = "boot-2".into();
    c.activity.as_mut().unwrap().epoch = "boot-2".into();
    ingest(&mut receipts, vec![a, b, c]);
    // Every boundary reallocates above the current maximum. b is a foreign
    // domain and keeps its incoming slot; the two new-domain members sort
    // within their own slots by the new domain's ordinals (10 before 50).
    assert_eq!(
        (receipts.rank("a"), receipts.rank("b"), receipts.rank("c")),
        (6, 5, 4)
    );
}

#[test]
fn a_first_sight_into_an_already_ingested_domain_has_no_boundary_baseline() {
    let mut receipts = ActivityReceipts::default();
    let a = term("a", "a", 1);
    ingest(&mut receipts, vec![a.clone()]);
    // The boundary into boot-2 is ingested on its own publish first.
    let mut a = a;
    a.activity.as_mut().unwrap().epoch = "boot-2".into();
    ingest(&mut receipts, vec![a.clone()]);
    // A newcomer first sights inside boot-2 while the domain is already
    // ingested — its completions are current facts, never boundary baseline.
    let mut t2 = term("t2", "a", 20);
    t2.activity.as_mut().unwrap().epoch = "boot-2".into();
    ingest(&mut receipts, vec![a, t2]);
    assert!(receipts.epoch_completions("a").is_some());
    assert!(receipts.epoch_completions("t2").is_none());
}

#[test]
fn a_new_agent_in_an_observed_epoch_is_ineligible_whatever_its_scope() {
    // Scopes are per terminal/incarnation — several live agents of one
    // core legitimately hold different scopes. Eligibility keys on
    // (core_id, epoch) only, matching the presentation's epoch key.
    let mut receipts = ActivityReceipts::default();
    let x = term("x", "a", 1);
    let mut y = term("y", "a", 2);
    y.activity.as_mut().unwrap().scope = 2;
    ingest(&mut receipts, vec![x.clone(), y.clone()]);
    // A newcomer into the same (core, epoch) — even a fresh scope — is
    // never boundary content once the epoch has been ingested.
    let mut z = term("z", "a", 3);
    z.activity.as_mut().unwrap().scope = 3;
    ingest(&mut receipts, vec![x, y, z]);
    assert!(receipts.epoch_completions("z").is_none());
}

#[test]
fn a_same_domain_loss_and_regain_still_changes_the_ingestion_boundary_token() {
    // The token distinguishes Some(A) -> None -> Some(A) even when the
    // consumed lifetime looks identical; same-domain updates never
    // re-establish it.
    let mut receipts = ActivityReceipts::default();
    let a = term("a", "a", 100);
    ingest(&mut receipts, vec![a.clone()]);
    let first = receipts.epoch_token("a").unwrap();
    let mut lost = a.clone();
    lost.activity = None;
    ingest(&mut receipts, vec![lost]);
    let second = receipts.epoch_token("a").unwrap();
    ingest(&mut receipts, vec![a.clone()]);
    let third = receipts.epoch_token("a").unwrap();
    assert!(first < second && second < third);
    let mut a = a;
    a.activity
        .as_mut()
        .unwrap()
        .records
        .get_mut("")
        .unwrap()
        .state_ordinal = Some(101);
    ingest(&mut receipts, vec![a]);
    assert_eq!(receipts.epoch_token("a").unwrap(), third);
}

#[test]
fn boundary_receipts_allocate_in_traversal_order_not_inventory_order() {
    let mut receipts = ActivityReceipts::default();
    let mut a = term("a", "a", 1);
    let mut b = term("b", "b", 1);
    // Unknown new-domain ordinals leave nothing to ordinal-sort inside a
    // domain; only the traversal can order the replacement.
    a.activity
        .as_mut()
        .unwrap()
        .records
        .get_mut("")
        .unwrap()
        .state_ordinal = None;
    b.activity
        .as_mut()
        .unwrap()
        .records
        .get_mut("")
        .unwrap()
        .state_ordinal = None;
    ingest(&mut receipts, vec![a.clone(), b.clone()]);
    a.activity.as_mut().unwrap().epoch = "boot-2".into();
    b.activity.as_mut().unwrap().epoch = "boot-2".into();
    // The resolved workspace/tab/layout traversal reads [b, a], opposite of
    // the inventory's arrival order.
    ingest_with_traversal(&mut receipts, vec![a, b], &["b", "a"]);
    assert!(receipts.rank("b") < receipts.rank("a"));
}

#[test]
fn untraversed_boundaries_keep_incoming_order_after_traversed_ones() {
    let mut receipts = ActivityReceipts::default();
    let a = term("a", "a", 1);
    let b = term("b", "b", 1);
    let c = term("c", "b", 1);
    ingest(&mut receipts, vec![a.clone(), b.clone(), c.clone()]);
    let mut a = a;
    let mut b = b;
    let mut c = c;
    a.activity.as_mut().unwrap().epoch = "n2".into();
    b.activity.as_mut().unwrap().epoch = "n2".into();
    c.activity.as_mut().unwrap().epoch = "n2".into();
    // Traversal knows only b; a and c fall back to their incoming order.
    ingest_with_traversal(&mut receipts, vec![a, b, c], &["b"]);
    assert!(receipts.rank("b") < receipts.rank("a"));
    assert!(receipts.rank("a") < receipts.rank("c"));
}

#[test]
fn a_withdrawn_named_record_leaves_the_boundary_baseline() {
    let mut receipts = ActivityReceipts::default();
    let a = term_named("a");
    ingest(&mut receipts, vec![a.clone()]);
    assert!(
        receipts
            .epoch_completions("a")
            .unwrap()
            .contains_key("build")
    );
    // The record withdraws while the surviving clock stays equal — the
    // baseline must still drop it on this authoritative ingest.
    let mut a = a;
    a.records.remove("build");
    a.activity.as_mut().unwrap().records.remove("build");
    ingest(&mut receipts, vec![a.clone()]);
    assert!(
        !receipts
            .epoch_completions("a")
            .unwrap()
            .contains_key("build")
    );
    // A same-domain clock bump keeps the baseline pruned on the clone path.
    a.activity
        .as_mut()
        .unwrap()
        .records
        .get_mut("")
        .unwrap()
        .state_ordinal = Some(31);
    ingest(&mut receipts, vec![a]);
    assert!(
        !receipts
            .epoch_completions("a")
            .unwrap()
            .contains_key("build")
    );
}

#[test]
fn a_withdrawn_root_leaves_the_boundary_baseline_while_a_named_record_survives() {
    // The root id tracks `term.status` presence, named ids track `records`
    // membership — a surviving named record must not retain the root.
    let mut receipts = ActivityReceipts::default();
    let a = term_named("a");
    ingest(&mut receipts, vec![a.clone()]);
    assert_eq!(receipts.epoch_completions("a").unwrap().len(), 2);
    let mut a = a;
    a.status = None;
    // The surviving named record keeps the clock equal, so the unchanged
    // path is the one that must drop the root baseline entry.
    ingest(&mut receipts, vec![a]);
    let baseline = receipts.epoch_completions("a").unwrap();
    assert!(!baseline.contains_key(""));
    assert!(baseline.contains_key("build"));
}

#[test]
fn repeated_core_replacement_on_one_endpoint_keeps_one_current_slot() {
    // Bounded means one current (core_id, epoch) slot per endpoint —
    // cycling origins on a single endpoint must not accumulate history.
    let mut receipts = ActivityReceipts::default();
    for n in 0..5u64 {
        let mut a = term("a", "a", n);
        a.address = "host-b/a".into();
        a.activity.as_mut().unwrap().core_id = format!("core-{n}");
        ingest(&mut receipts, vec![a]);
        assert_eq!(receipts.endpoint_epochs.len(), 1, "cycle {n}");
        assert_eq!(
            receipts.endpoint_epochs["host-b"].0,
            format!("core-{n}"),
            "cycle {n}"
        );
    }
    // The slot holds the CURRENT origin only: a newcomer matching it is
    // ineligible; a long-replaced core is not pinned by history either.
    let mut t2 = term("t2", "a", 9);
    t2.address = "host-b/t2".into();
    t2.activity.as_mut().unwrap().core_id = "core-4".into();
    ingest(&mut receipts, vec![t2]);
    assert!(receipts.epoch_completions("host-b/t2").is_none());
}

#[test]
fn a_removed_endpoint_releases_its_origin_slot() {
    let mut receipts = ActivityReceipts::default();
    let mut remote = term("a", "a", 1);
    remote.address = "host-b/a".into();
    ingest(&mut receipts, vec![remote, term("t1", "b", 1)]);
    assert_eq!(receipts.endpoint_epochs.len(), 2);
    // `machine.list` no longer carries host-b — its slot retires. The
    // local endpoint is always live, and an unknown lifecycle (`None`)
    // retires nothing.
    receipts.sync_machines(None);
    assert_eq!(receipts.endpoint_epochs.len(), 2);
    receipts.sync_machines(Some(&[]));
    assert_eq!(receipts.endpoint_epochs.len(), 1);
    assert!(receipts.endpoint_epochs.contains_key(""));
}

#[test]
fn an_ordinary_reconnect_without_a_boundary_never_reallocates() {
    let mut receipts = ActivityReceipts::default();
    let a = term("a", "a", 1);
    let b = term("b", "b", 2);
    ingest(&mut receipts, vec![a.clone(), b.clone()]);
    let mut a = a;
    a.status.as_mut().unwrap().state = "done".into();
    a.activity.as_mut().unwrap().revision = u64::MAX;
    a.turn = u64::MAX;
    a.status_ms = Some(u64::MAX);
    ingest(&mut receipts, vec![b, a]);
    assert_eq!((receipts.rank("a"), receipts.rank("b")), (1, 2));
}
