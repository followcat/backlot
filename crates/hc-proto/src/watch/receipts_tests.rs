use super::*;
use crate::terminal::{Activity, RecordClock, Status};

pub(crate) fn term(id: &str, origin: &str, n: u64) -> Terminal {
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

#[test]
fn late_a20_b_a10_has_the_reference_receipts_one_two_three() {
    let mut receipts = ActivityReceipts::default();
    let a20 = term("a20", "a", 20);
    let b = term("b", "b", 1);
    ingest(&mut receipts, vec![a20.clone()]);
    ingest(&mut receipts, vec![a20.clone(), b.clone()]);
    ingest(&mut receipts, vec![a20, b, term("a10", "a", 10)]);
    assert_eq!(
        (
            receipts.rank("a20"),
            receipts.rank("b"),
            receipts.rank("a10")
        ),
        (1, 2, 3)
    );
}

#[test]
fn a_comparable_batch_sorts_without_comparing_foreign_counters() {
    let mut receipts = ActivityReceipts::default();
    ingest(
        &mut receipts,
        vec![term("a20", "a", 20), term("a10", "a", 10)],
    );
    assert_eq!((receipts.rank("a10"), receipts.rank("a20")), (1, 2));
    let mut receipts = ActivityReceipts::default();
    ingest(
        &mut receipts,
        vec![term("z", "z", u64::MAX), term("a", "a", 1)],
    );
    assert_eq!((receipts.rank("z"), receipts.rank("a")), (1, 2));
}

#[test]
fn incomparable_epoch_scope_and_unknown_slots_keep_incoming_order() {
    for boundary in 0..3 {
        let mut first = term("z", "a", u64::MAX);
        let second = term("a", "a", 1);
        match boundary {
            0 => first.activity.as_mut().unwrap().epoch = "other".into(),
            1 => first.activity.as_mut().unwrap().scope = 2,
            _ => {
                first.activity = None;
            }
        }
        let mut receipts = ActivityReceipts::default();
        ingest(&mut receipts, vec![first, second]);
        assert_eq!((receipts.rank("z"), receipts.rank("a")), (1, 2));
    }
}

#[test]
fn mixed_snapshot_keeps_domain_slots_and_sorts_only_comparable_members() {
    let mut receipts = ActivityReceipts::default();
    ingest(
        &mut receipts,
        vec![
            term("a20", "a", 20),
            term("b", "b", 1),
            term("a10", "a", 10),
        ],
    );
    assert_eq!(
        (
            receipts.rank("a10"),
            receipts.rank("b"),
            receipts.rank("a20")
        ),
        (1, 2, 3)
    );
}

#[test]
fn a_domain_without_an_ordinal_has_unknown_history_and_keeps_its_slot() {
    let mut unknown = term("unknown", "a", 1);
    unknown
        .activity
        .as_mut()
        .unwrap()
        .records
        .get_mut("")
        .unwrap()
        .state_ordinal = None;
    let mut receipts = ActivityReceipts::default();
    ingest(&mut receipts, vec![term("known", "a", 20), unknown]);
    assert_eq!((receipts.rank("known"), receipts.rank("unknown")), (1, 2));
}

#[test]
fn unchanged_reconnect_metadata_and_readiness_aliases_keep_receipts() {
    let mut receipts = ActivityReceipts::default();
    let a = term("a", "a", 1);
    let b = term("b", "b", 2);
    ingest(&mut receipts, vec![a.clone(), b.clone()]);
    let mut a = a;
    a.status.as_mut().unwrap().state = "done".into();
    a.status.as_mut().unwrap().title = "metadata".into();
    a.activity.as_mut().unwrap().revision = u64::MAX;
    a.turn = u64::MAX;
    a.status_ms = Some(u64::MAX);
    ingest(&mut receipts, vec![b, a]);
    assert_eq!((receipts.rank("a"), receipts.rank("b")), (1, 2));
}

#[test]
fn pruning_reuses_current_max_and_same_batch_prunes_after_allocation() {
    let mut receipts = ActivityReceipts::default();
    let a = term("a", "a", 1);
    ingest(&mut receipts, vec![a.clone(), term("b", "b", 1)]);
    ingest(&mut receipts, vec![a.clone()]);
    ingest(&mut receipts, vec![a.clone(), term("c", "c", 1)]);
    assert_eq!(receipts.rank("c"), 2);
    ingest(&mut receipts, vec![a, term("d", "d", 1)]);
    assert_eq!(receipts.rank("d"), 3);
    assert_eq!(receipts.rank("b"), 0);
    assert_eq!(receipts.rank("c"), 0);
    assert_eq!(receipts.observed.len(), 2);
    ingest(&mut receipts, vec![]);
    ingest(&mut receipts, vec![term("new", "a", 100)]);
    assert_eq!(receipts.rank("new"), 1);
}

#[test]
fn a_domain_boundary_reallocates_and_fresh_work_still_advances() {
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
        assert_eq!((receipts.rank("a"), receipts.rank("b")), (3, 2));
        a.activity
            .as_mut()
            .unwrap()
            .records
            .get_mut("")
            .unwrap()
            .state_ordinal = Some(101);
        ingest(&mut receipts, vec![a, b]);
        assert_eq!(receipts.rank("a"), 4);
    }
}

#[test]
fn saturation_keeps_equal_ranks_instead_of_inventing_a_tie_break() {
    let mut receipts = ActivityReceipts::default();
    let a = term("a", "a", 1);
    ingest(&mut receipts, vec![a.clone()]);
    receipts.observed.get_mut("a").unwrap().receipt = u64::MAX;
    ingest(
        &mut receipts,
        vec![a.clone(), term("z", "z", 1), term("b", "b", 1)],
    );
    assert_eq!(
        (receipts.rank("a"), receipts.rank("z"), receipts.rank("b")),
        (u64::MAX, u64::MAX, u64::MAX)
    );
    ingest(&mut receipts, vec![term("b", "b", 1), a, term("z", "z", 1)]);
    assert_eq!(receipts.rank("b"), u64::MAX);
}

#[test]
fn shells_finished_and_lost_reporters_leave_no_receipt_cache() {
    let mut receipts = ActivityReceipts::default();
    for variation in 0..3 {
        let mut a = term("a", "a", 1);
        match variation {
            0 => {
                a.reported = false;
                a.status = None;
            }
            1 => a.state = "finished".into(),
            _ => {
                a.pid = None;
                a.reason = Some("holder_lost".into());
            }
        }
        ingest(&mut receipts, vec![a]);
        assert!(receipts.observed.is_empty());
    }
}

#[test]
fn named_clocks_count_only_while_the_record_is_present() {
    let mut receipts = ActivityReceipts::default();
    let mut a = term("a", "a", 1);
    let activity: &mut Activity = a.activity.as_mut().unwrap();
    activity.records.insert(
        "build".into(),
        RecordClock {
            state_ordinal: Some(10),
            completion_ordinal: Some(10),
        },
    );
    ingest(&mut receipts, vec![a.clone()]);
    a.records.insert(
        "build".into(),
        Status {
            state: "idle".into(),
            ..Status::default()
        },
    );
    ingest(&mut receipts, vec![a.clone()]);
    assert_eq!(receipts.rank("a"), 2);
    a.records.remove("build");
    ingest(&mut receipts, vec![a]);
    assert_eq!(receipts.rank("a"), 3);
}
