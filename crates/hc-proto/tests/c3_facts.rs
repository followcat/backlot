//! C3 snapshot and event contract. No native core or local Git probing.

mod common;

use common::inventory::{ev, terminal};
use common::minimal_terminal;
use hc_proto::events::ServerNotification;
use hc_proto::inventory::Inventory;
use serde_json::{Value, json};

fn repository() -> Value {
    json!({
        "core_id":"origin-a", "cwd":"/home/user/acme-shop/src", "state":"ready",
        "auto_label":"acme-shop", "branch":"main",
        "ahead_behind":{"ahead":2,"behind":1},
        "checkout":{
            "repository_key":"opaque-repo", "checkout_key":"opaque-checkout",
            "repo_name":"acme-shop", "checkout_name":"acme-shop",
            "repo_root":"/home/user/acme-shop", "is_linked_worktree":false,"is_bare":false
        }
    })
}

fn activity(epoch: &str, scope: u64) -> Value {
    json!({"core_id":"origin-a", "epoch":epoch, "scope":scope, "revision":42,
        "records":{"":{"state_ordinal":42,"completion_ordinal":41},
            "build/test":{"state_ordinal":40,"completion_ordinal":null}}})
}

fn inventory() -> Inventory {
    let mut record = minimal_terminal();
    record["repository"] = repository();
    record["activity"] = activity("boot-a", 1);
    let mut inventory = Inventory::unfiltered();
    inventory.replace(1, vec![terminal(record)]);
    inventory
}

#[test]
fn snapshots_keep_origin_facts_and_nullable_clocks() {
    let inventory = inventory();
    let record = inventory.get("t7").unwrap();
    let repo = record.repository.as_ref().unwrap();
    assert_eq!(
        repo.checkout.as_ref().unwrap().repo_root,
        "/home/user/acme-shop"
    );
    assert_eq!(repo.ahead_behind.as_ref().unwrap().ahead, 2);
    let clock = record.activity.as_ref().unwrap();
    assert_eq!(clock.records[""].completion_ordinal, Some(41));
    assert_eq!(clock.records["build/test"].completion_ordinal, None);
    assert!(!record.extra.contains_key("repository"));
    assert!(!record.extra.contains_key("activity"));
}

#[test]
fn repository_events_replace_including_unknowns_not_merge() {
    let mut inventory = inventory();
    let replacement = json!({"core_id":"origin-b","cwd":"/home/user/plain",
        "state":"pending","auto_label":null,"checkout":null,"branch":null,
        "ahead_behind":null});
    let effect = inventory.apply(&ev(
        2,
        "repository",
        Some("t7"),
        json!({"repository":replacement}),
    ));
    assert_eq!(effect.changed, ["t7"]);
    let repo = inventory.get("t7").unwrap().repository.as_ref().unwrap();
    assert_eq!(repo.core_id, "origin-b");
    assert_eq!(repo.cwd, "/home/user/plain");
    assert_eq!(repo.branch, None);
    assert_eq!(repo.checkout, None);
    assert_eq!(repo.ahead_behind, None);
    inventory.apply(&ev(3, "repository", Some("t7"), json!({"repository":null})));
    assert_eq!(inventory.get("t7").unwrap().repository, None);
}

#[test]
fn status_atomically_replaces_activity_and_named_records() {
    let mut inventory = inventory();
    let mut clock = activity("boot-b", 2);
    clock["records"] = json!({"":{"state_ordinal":null,"completion_ordinal":null}});
    inventory.apply(&ev(
        2,
        "status",
        Some("t7"),
        json!({
            "status":{"state":"idle"},"records":{},"reported":true,"activity":clock
        }),
    ));
    let record = inventory.get("t7").unwrap();
    assert_eq!(record.status.as_ref().unwrap().state, "idle");
    assert!(record.reported);
    let activity = record.activity.as_ref().unwrap();
    assert_eq!(activity.epoch, "boot-b");
    assert_eq!(activity.scope, 2);
    assert_eq!(activity.records.len(), 1);
    assert_eq!(activity.records[""].completion_ordinal, None);
    inventory.apply(&ev(
        3,
        "status",
        Some("t7"),
        json!({
            "status":null,"records":{},"reported":false,"activity":null
        }),
    ));
    assert_eq!(inventory.get("t7").unwrap().activity, None);
    assert_eq!(inventory.get("t7").unwrap().status, None);
}

#[test]
fn standalone_activity_replaces_epoch_scope_and_resets() {
    let mut inventory = inventory();
    inventory.apply(&ev(
        2,
        "activity",
        Some("t7"),
        json!({"activity":activity("boot-b", u64::MAX)}),
    ));
    let record = inventory.get("t7").unwrap();
    assert_eq!(record.activity.as_ref().unwrap().scope, u64::MAX);
    assert_eq!(record.activity.as_ref().unwrap().epoch, "boot-b");
    assert!(record.repository.is_some());
    inventory.apply(&ev(3, "activity", Some("t7"), json!({"activity":null})));
    assert_eq!(inventory.get("t7").unwrap().activity, None);
}

#[test]
fn older_status_without_activity_preserves_it_like_the_core_mirror() {
    let mut inventory = inventory();
    let old = inventory.get("t7").unwrap().activity.clone();
    inventory.apply(&ev(
        2,
        "status",
        Some("t7"),
        json!({
            "status":null,"records":{},"reported":false
        }),
    ));
    assert_eq!(inventory.get("t7").unwrap().activity, old);
    // A reconnect snapshot with unsupported facts replaces the old fields.
    inventory.replace(2, vec![terminal(minimal_terminal())]);
    assert_eq!(inventory.get("t7").unwrap().repository, None);
    assert_eq!(inventory.get("t7").unwrap().activity, None);
}

#[test]
fn malformed_c3_events_never_partially_mutate_or_advance() {
    let mut broken_clock = activity("boot-b", 2);
    broken_clock["records"][""]["completion_ordinal"] = json!(-1);
    let mut broken_repo = repository();
    broken_repo["ahead_behind"]["ahead"] = json!("2");
    for event in [
        ev(2, "repository", Some("t7"), json!({})),
        ev(2, "repository", None, json!({"repository":repository()})),
        ev(
            2,
            "repository",
            Some("t7"),
            json!({"repository":broken_repo}),
        ),
        ev(2, "activity", Some("t7"), json!({"activity":{"scope":1}})),
        ev(2, "activity", Some("t7"), json!({})),
        ev(
            2,
            "activity",
            None,
            json!({"activity":activity("boot-b",2)}),
        ),
        ev(
            2,
            "status",
            Some("t7"),
            json!({"status":{"state":"done"},
            "records":{},"reported":true,"activity":broken_clock}),
        ),
    ] {
        let mut inventory = inventory();
        let before = inventory.get("t7").unwrap().clone();
        let effect = inventory.apply(&event);
        assert!(effect.malformed && effect.resync, "{event:?}");
        assert_eq!(inventory.seq(), 1);
        assert_eq!(inventory.get("t7").unwrap(), &before);
    }
}

#[test]
fn c3_events_require_a_data_member() {
    for kind in ["repository", "activity"] {
        let value = json!({"jsonrpc":"2.0","method":"event","params":{"seq":2,"type":kind,"terminal":"t7"}});
        let notification = serde_json::from_value(value).unwrap();
        assert!(ServerNotification::from_notification(&notification).is_err());
    }
}

#[test]
fn nullable_fact_members_must_be_present_in_complete_replacements() {
    for member in ["auto_label", "checkout", "branch", "ahead_behind"] {
        let mut repo = repository();
        repo.as_object_mut().unwrap().remove(member);
        let mut inv = inventory();
        let before = inv.get("t7").unwrap().clone();
        let effect = inv.apply(&ev(2, "repository", Some("t7"), json!({"repository":repo})));
        assert!(effect.malformed && effect.resync, "{member}");
        assert_eq!(inv.get("t7").unwrap(), &before);
        assert_eq!(inv.seq(), 1);
    }
    for member in ["state_ordinal", "completion_ordinal"] {
        let mut clock = activity("boot-b", 2);
        clock["records"][""].as_object_mut().unwrap().remove(member);
        for (kind, data) in [
            ("activity", json!({"activity":clock})),
            (
                "status",
                json!({"status":{"state":"done"}, "records":{},
                "reported":true,"activity":clock}),
            ),
        ] {
            let mut inv = inventory();
            let before = inv.get("t7").unwrap().clone();
            let effect = inv.apply(&ev(2, kind, Some("t7"), data));
            assert!(effect.malformed && effect.resync, "{kind} {member}");
            assert_eq!(inv.get("t7").unwrap(), &before);
            assert_eq!(inv.seq(), 1);
        }
    }
}
