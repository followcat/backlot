//! Tests for dropping queued mail addressed to permanently closed
//! actors (`actor_closed`) while preserving paused/resuming actors and
//! fixed-role mail.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use backlot_model::event::{EventKind, RESTARTED, Verdict};
use backlot_model::state::{StudioState, replay};
use backlot_model::summary::{studio_exchange, summary_with_exchange};
use backlot_model::testkit::{
    actor, cast, delivered, ev, reviewed, studio_kind, task_created, task_state,
};
use backlot_model::{EventBy, NewEvent};
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use hc_proto::frame::{Frame, Kind};
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, DuplexStream};

use crate::daemon::Stage;
use crate::deliver::outbox;
use crate::nudge::queued_for;
use crate::reconcile::LiveTerminal;
use crate::render::status;
use crate::store::{self, Pending};
use crate::test_support::TestDir;

fn tmp(label: &str) -> TestDir {
    TestDir::new(&format!("backlot-closed-actor-{label}"))
}

async fn serve_noop(stream: DuplexStream) {
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = Reader::new(read);
    let _hello = reader.read_line().await.unwrap().unwrap();
    let hi = json!({
        "jsonrpc": "2.0", "id": 0,
        "result": {
            "generation": 1, "version": "fake-core 0.0", "framed": true,
            "core": {"id": "core-fake-1", "name": "fake-core"},
            "methods": ["terminal.list", "terminal.send"], "features": [],
        }
    });
    write.write_all(format!("{hi}\n").as_bytes()).await.unwrap();
    while let Ok(Some(frame)) = reader.read_frame().await {
        if frame.channel != 0 || frame.kind != Kind::Data {
            continue;
        }
        let req: Value = serde_json::from_slice(&frame.payload).unwrap();
        let body = json!({ "jsonrpc": "2.0", "id": req["id"], "result": {} });
        write
            .write_all(&Frame::data(0, serde_json::to_vec(&body).unwrap()).encode())
            .await
            .unwrap();
    }
}

async fn make_stage() -> Stage {
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    tokio::spawn(serve_noop(core_side));
    let (read, write) = tokio::io::split(client_side);
    let client = Client::open(Box::pin(read), Box::pin(write), None)
        .await
        .unwrap();
    Stage::for_test(client)
}

fn append_ev(dir: &Path, role: &str, kind: EventKind) -> u64 {
    store::append(dir, move |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(EventBy::role(role), kind),
            files: Vec::new(),
        })
    })
    .unwrap()
    .seq
}

fn seed_base_with_actor(dir: &Path) -> u64 {
    append_ev(dir, "user", studio_kind());
    append_ev(
        dir,
        "producer",
        EventKind::ReqVersion {
            version: 1,
            file: "req/v1.md".into(),
            summary: "s".into(),
        },
    );
    append_ev(dir, "director", task_created("t1", "feature", 1));
    append_ev(dir, "director", cast("t1", "actor-a", "a"));
    append_ev(
        dir,
        "stage",
        EventKind::RoleStarted {
            role: "actor-t1-c1".into(),
            terminal: Some("term-t1-1".into()),
            actor: Some(actor("a")),
            session_id: Some("sess-t1-1".into()),
            reason: None,
        },
    );
    append_ev(
        dir,
        "director",
        EventKind::MessageQueued {
            to: "actor-t1-c1".into(),
            file: Some("messages/6-note.md".into()),
            purpose: None,
            source_seq: None,
        },
    )
}

fn drops(dir: &Path) -> Vec<(u64, String)> {
    store::load_events(dir)
        .unwrap()
        .0
        .into_iter()
        .filter_map(|e| match e.kind {
            EventKind::MessageDropped { target, reason } => Some((target, reason)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn drops_mail_when_actor_is_closed_exited_finished_or_recast() {
    // 1. Accepted task + RoleClosed + no live terminal -> dropped `actor_closed`.
    let dir = tmp("closed");
    let msg_seq = seed_base_with_actor(&dir);
    let deliv_seq = append_ev(&dir, "actor-t1-c1", delivered("t1", 1, "a", 1));
    let deliv_notice_seq = append_ev(
        &dir,
        "stage",
        EventKind::MessageQueued {
            to: "director".into(),
            file: Some(format!("messages/{deliv_seq}-delivered.md")),
            purpose: None,
            source_seq: Some(deliv_seq),
        },
    );
    append_ev(
        &dir,
        "stage",
        EventKind::MessageDelivered {
            target: deliv_notice_seq,
        },
    );
    append_ev(
        &dir,
        "director",
        reviewed("t1", 1, Verdict::Accepted, vec![]),
    );
    append_ev(
        &dir,
        "stage",
        EventKind::RoleClosed {
            role: "actor-t1-c1".into(),
            reason: Some("no longer desired".into()),
        },
    );
    let stage = make_stage().await;
    let state = store::load_state(&dir).unwrap();
    let msgs_before = state.messages.len();
    stage
        .deliver("acme-shop", &dir, &state, &[], 10_000)
        .await
        .unwrap();
    assert_eq!(drops(&dir), vec![(msg_seq, "actor_closed".into())]);
    let after = store::load_state(&dir).unwrap();
    assert_eq!(
        after.messages.len(),
        msgs_before,
        "dropping actor mail must not queue any new notice"
    );

    // 2. Exited (`RoleLost`) on terminal task + no live terminal -> dropped `actor_closed`.
    let dir_lost = tmp("exited");
    let lost_seq = seed_base_with_actor(&dir_lost);
    append_ev(
        &dir_lost,
        "director",
        task_state(
            "t1",
            backlot_model::event::TaskStateChange::Abandoned,
            "cut",
        ),
    );
    append_ev(
        &dir_lost,
        "stage",
        EventKind::RoleLost {
            role: "actor-t1-c1".into(),
            reason: Some("terminal gone".into()),
        },
    );
    let state_lost = store::load_state(&dir_lost).unwrap();
    stage
        .deliver("acme-shop", &dir_lost, &state_lost, &[], 10_000)
        .await
        .unwrap();
    assert_eq!(drops(&dir_lost), vec![(lost_seq, "actor_closed".into())]);

    // 3. Recast to actor-t1-c2 (live on bl-acme-shop-t1 at a new address) still drops mail to closed c1.
    let dir_recast = tmp("recast");
    let c1_seq = seed_base_with_actor(&dir_recast);
    append_ev(
        &dir_recast,
        "stage",
        EventKind::RoleClosed {
            role: "actor-t1-c1".into(),
            reason: Some("no longer desired".into()),
        },
    );
    append_ev(&dir_recast, "director", cast("t1", "actor-b", "b"));
    append_ev(
        &dir_recast,
        "stage",
        EventKind::RoleStarted {
            role: "actor-t1-c2".into(),
            terminal: Some("term-t1-2".into()),
            actor: Some(actor("b")),
            session_id: Some("sess-t1-2".into()),
            reason: None,
        },
    );
    let c2_seq = append_ev(
        &dir_recast,
        "director",
        EventKind::MessageQueued {
            to: "actor-t1-c2".into(),
            file: Some("messages/10-c2.md".into()),
            purpose: None,
            source_seq: None,
        },
    );
    let live_c2 = [LiveTerminal {
        name: "bl-acme-shop-t1".into(),
        address: "term-t1-2".into(),
        labels: BTreeMap::from([("backlot.studio".into(), "acme-shop".into())]),
        ..Default::default()
    }];
    let state_recast = store::load_state(&dir_recast).unwrap();
    stage
        .deliver("acme-shop", &dir_recast, &state_recast, &live_c2, 10_000)
        .await
        .unwrap();
    assert_eq!(drops(&dir_recast), vec![(c1_seq, "actor_closed".into())]);
    let after_recast = store::load_state(&dir_recast).unwrap();
    assert!(
        !after_recast
            .messages
            .iter()
            .find(|m| m.seq == c2_seq)
            .unwrap()
            .dropped
    );
}

#[tokio::test]
async fn keeps_mail_for_paused_resuming_reopened_or_live_actor() {
    let stage = make_stage().await;

    // 1. Paused task: not dropped.
    let dir_paused = tmp("paused");
    seed_base_with_actor(&dir_paused);
    append_ev(
        &dir_paused,
        "director",
        task_state("t1", backlot_model::event::TaskStateChange::Paused, "hold"),
    );
    let state_paused = store::load_state(&dir_paused).unwrap();
    stage
        .deliver("acme-shop", &dir_paused, &state_paused, &[], 10_000)
        .await
        .unwrap();
    assert!(
        drops(&dir_paused).is_empty(),
        "paused actor mail stays queued"
    );

    // 2. Lost actor on running task (being resumed): not dropped.
    let dir_lost = tmp("recovering");
    seed_base_with_actor(&dir_lost);
    append_ev(
        &dir_lost,
        "stage",
        EventKind::RoleLost {
            role: "actor-t1-c1".into(),
            reason: Some("terminal gone".into()),
        },
    );
    let state_lost = store::load_state(&dir_lost).unwrap();
    stage
        .deliver("acme-shop", &dir_lost, &state_lost, &[], 10_000)
        .await
        .unwrap();
    assert!(
        drops(&dir_lost).is_empty(),
        "recovering actor mail stays queued"
    );

    // 3. Closed actor reopened (`role.reopen`, held for resume): not dropped.
    let dir_reopen = tmp("reopen");
    seed_base_with_actor(&dir_reopen);
    append_ev(&dir_reopen, "actor-t1-c1", delivered("t1", 1, "a", 1));
    append_ev(
        &dir_reopen,
        "director",
        reviewed("t1", 1, Verdict::Accepted, vec![]),
    );
    append_ev(
        &dir_reopen,
        "stage",
        EventKind::RoleClosed {
            role: "actor-t1-c1".into(),
            reason: Some("no longer desired".into()),
        },
    );
    append_ev(
        &dir_reopen,
        "user",
        EventKind::RoleReopen {
            role: "actor-t1-c1".into(),
        },
    );
    let state_reopen = store::load_state(&dir_reopen).unwrap();
    stage
        .deliver("acme-shop", &dir_reopen, &state_reopen, &[], 10_000)
        .await
        .unwrap();
    assert!(
        drops(&dir_reopen).is_empty(),
        "reopened actor mail stays queued"
    );

    // 4. Actor still has a live terminal for that actor id: not dropped.
    let dir_live = tmp("live-term");
    seed_base_with_actor(&dir_live);
    append_ev(&dir_live, "actor-t1-c1", delivered("t1", 1, "a", 1));
    append_ev(
        &dir_live,
        "director",
        reviewed("t1", 1, Verdict::Accepted, vec![]),
    );
    let live = [LiveTerminal {
        name: "bl-acme-shop-t1".into(),
        address: "term-t1-1".into(),
        labels: BTreeMap::from([("backlot.studio".into(), "acme-shop".into())]),
        ..Default::default()
    }];
    let state_live = store::load_state(&dir_live).unwrap();
    stage
        .deliver("acme-shop", &dir_live, &state_live, &live, 10_000)
        .await
        .unwrap();
    assert!(
        drops(&dir_live).is_empty(),
        "live actor terminal mail stays queued"
    );

    // 5. Restarting actor (`role.restart`) even after `role.closed { reason: "restarted" }`: not dropped.
    let dir_restart = tmp("restart");
    seed_base_with_actor(&dir_restart);
    append_ev(
        &dir_restart,
        "director",
        EventKind::RoleRestart {
            role: "actor-t1-c1".into(),
            fresh: false,
            reason: Some("wedged".into()),
        },
    );
    append_ev(
        &dir_restart,
        "stage",
        EventKind::RoleClosed {
            role: "actor-t1-c1".into(),
            reason: Some(RESTARTED.into()),
        },
    );
    let state_restart = store::load_state(&dir_restart).unwrap();
    stage
        .deliver("acme-shop", &dir_restart, &state_restart, &[], 10_000)
        .await
        .unwrap();
    assert!(
        drops(&dir_restart).is_empty(),
        "restarting actor mail stays queued"
    );
}

#[tokio::test]
async fn never_drops_fixed_role_mail() {
    let dir = tmp("roles");
    append_ev(&dir, "user", studio_kind());
    append_ev(
        &dir,
        "producer",
        EventKind::ReqVersion {
            version: 1,
            file: "req/v1.md".into(),
            summary: "s".into(),
        },
    );
    for role in ["producer", "director", "writer"] {
        append_ev(
            &dir,
            "stage",
            EventKind::RoleStarted {
                role: role.into(),
                terminal: Some(format!("term-{role}")),
                actor: Some(actor("a")),
                session_id: Some(format!("sess-{role}")),
                reason: None,
            },
        );
        append_ev(
            &dir,
            "user",
            EventKind::MessageQueued {
                to: role.into(),
                file: Some(format!("messages/tell-{role}.md")),
                purpose: None,
                source_seq: None,
            },
        );
        append_ev(
            &dir,
            "stage",
            EventKind::RoleClosed {
                role: role.into(),
                reason: Some("no longer desired".into()),
            },
        );
    }
    append_ev(
        &dir,
        "user",
        EventKind::StudioFinished {
            reason: "done".into(),
        },
    );
    let stage = make_stage().await;
    let state = store::load_state(&dir).unwrap();
    stage
        .deliver("acme-shop", &dir, &state, &[], 10_000)
        .await
        .unwrap();
    assert!(
        drops(&dir).is_empty(),
        "role mail for producer/director/writer is never dropped as actor_closed"
    );
}

#[tokio::test]
async fn old_ledger_with_stale_actor_mail_is_cleaned_without_rewrite() {
    let dir = tmp("old-ledger");
    let events = vec![
        ev(1, "user", studio_kind()),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "s".into(),
            },
        ),
        ev(3, "director", task_created("t1", "feature", 1)),
        ev(4, "director", cast("t1", "actor-a", "a")),
        ev(
            5,
            "stage",
            EventKind::RoleStarted {
                role: "actor-t1-c1".into(),
                terminal: Some("term-1".into()),
                actor: Some(actor("a")),
                session_id: Some("sess-1".into()),
                reason: None,
            },
        ),
        ev(
            6,
            "director",
            EventKind::MessageQueued {
                to: "actor-t1-c1".into(),
                file: Some("messages/6-old.md".into()),
                purpose: None,
                source_seq: None,
            },
        ),
        ev(7, "actor-t1-c1", delivered("t1", 1, "a", 1)),
        ev(8, "director", reviewed("t1", 1, Verdict::Accepted, vec![])),
        ev(
            9,
            "stage",
            EventKind::RoleClosed {
                role: "actor-t1-c1".into(),
                reason: Some("no longer desired".into()),
            },
        ),
    ];
    // Write a pre-existing v1 ledger on disk (without touching store::append).
    let mut raw = String::new();
    for e in &events {
        raw.push_str(&serde_json::to_string(e).unwrap());
        raw.push('\n');
    }
    fs::write(dir.join("events.jsonl"), &raw).unwrap();
    let _replayed: StudioState = replay(&events);

    let stage = make_stage().await;
    let state = store::load_state(&dir).unwrap();
    stage
        .deliver("acme-shop", &dir, &state, &[], 10_000)
        .await
        .unwrap();

    let after_raw = fs::read_to_string(dir.join("events.jsonl")).unwrap();
    assert!(
        after_raw.starts_with(&raw),
        "old ledger lines must stay untouched (no rewrite)"
    );
    assert_eq!(drops(&dir), vec![(6, "actor_closed".into())]);
}

#[tokio::test]
async fn dropped_actor_mail_no_longer_counts_as_pending_and_stays_visible() {
    let dir = tmp("visible");
    append_ev(&dir, "user", studio_kind());
    append_ev(
        &dir,
        "producer",
        EventKind::ReqVersion {
            version: 1,
            file: "req/v1.md".into(),
            summary: "s".into(),
        },
    );
    append_ev(&dir, "director", task_created("t1", "feature", 1));
    append_ev(&dir, "director", cast("t1", "actor-a", "a"));
    append_ev(
        &dir,
        "stage",
        EventKind::RoleStarted {
            role: "actor-t1-c1".into(),
            terminal: Some("term-t1".into()),
            actor: Some(actor("a")),
            session_id: Some("sess-t1".into()),
            reason: None,
        },
    );
    let msg_seq = append_ev(
        &dir,
        "user",
        EventKind::MessageQueued {
            to: "actor-t1-c1".into(),
            file: Some("messages/6-user-tell.md".into()),
            purpose: None,
            source_seq: None,
        },
    );
    let deliv_seq = append_ev(&dir, "actor-t1-c1", delivered("t1", 1, "a", 1));
    let deliv_notice_seq = append_ev(
        &dir,
        "stage",
        EventKind::MessageQueued {
            to: "director".into(),
            file: Some(format!("messages/{deliv_seq}-delivered.md")),
            purpose: None,
            source_seq: Some(deliv_seq),
        },
    );
    append_ev(
        &dir,
        "stage",
        EventKind::MessageDelivered {
            target: deliv_notice_seq,
        },
    );
    append_ev(
        &dir,
        "director",
        reviewed("t1", 1, Verdict::Accepted, vec![]),
    );
    append_ev(
        &dir,
        "stage",
        EventKind::RoleClosed {
            role: "actor-t1-c1".into(),
            reason: Some("no longer desired".into()),
        },
    );

    let before = store::load_state(&dir).unwrap();
    let sum_before = summary_with_exchange(
        &[("acme-shop".into(), before.clone())],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        10_000,
    );
    assert!(sum_before.studios[0].pending_tell);

    let stage = make_stage().await;
    stage
        .deliver("acme-shop", &dir, &before, &[], 10_000)
        .await
        .unwrap();

    let after = store::load_state(&dir).unwrap();
    assert!(outbox(&after, &dir).is_empty(), "not pending in outbox");
    assert!(!queued_for(&after, "actor-t1-c1"), "not pending in nudge");

    let status_out = status(
        "acme-shop",
        &after,
        &Default::default(),
        10_000,
        &Default::default(),
    );
    assert!(
        status_out.contains("messages: 0 queued, 1 delivered, 1 dropped"),
        "{status_out}"
    );
    assert!(
        status_out.contains("actor_closed"),
        "status must surface the drop reason:\n{status_out}"
    );

    let sum_after = summary_with_exchange(
        &[("acme-shop".into(), after.clone())],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        10_000,
    );
    assert!(!sum_after.studios[0].pending_tell, "no longer pending_tell");

    let ex = studio_exchange(&after, &BTreeMap::new());
    let row = ex
        .iter()
        .find(|e| e.seq == msg_seq)
        .unwrap_or_else(|| panic!("dropped user tell stays visible in studio_exchange: {ex:?}"));
    assert_eq!(row.dropped_reason.as_deref(), Some("actor_closed"));
    let msg = after.messages.iter().find(|m| m.seq == msg_seq).unwrap();
    assert!(msg.dropped);
    assert_eq!(msg.dropped_reason.as_deref(), Some("actor_closed"));
}
