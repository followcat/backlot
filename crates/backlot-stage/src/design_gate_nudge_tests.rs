//! End-to-end `Stage::nudge` tests across gated opening design phases.

use std::fs;
use std::path::Path;

use backlot_model::design::{DesignGate, DesignSlot};
use backlot_model::event::EventKind;
use backlot_model::state::replay;
use backlot_model::summary::Work;
use backlot_model::testkit::*;
use backlot_model::{EventBy, NewEvent};
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::daemon::Stage;
use crate::reconcile::{self, LiveTerminal};
use crate::store::{self, Pending};
use crate::test_support::TestDir;

fn studio_required() -> EventKind {
    let mut k = studio_kind();
    if let EventKind::StudioCreated { design_gate, .. } = &mut k {
        *design_gate = DesignGate::Required;
    }
    k
}

fn design_entry(version: u32, round: u32, seq: u64, slot: DesignSlot) -> EventKind {
    let slug = match slot {
        DesignSlot::Proposal => "proposal",
        DesignSlot::Buildability => "buildability",
        DesignSlot::Summary => "summary",
        DesignSlot::UserInput => "user_input",
        DesignSlot::Supplement => "supplement",
    };
    EventKind::DesignEntry {
        version,
        round,
        slot,
        file: format!("design/v{version}/r{round}-{seq}-{slug}.md"),
        source: None,
        refs: Vec::new(),
        questions: Vec::new(),
        checks: Vec::new(),
    }
}

async fn fake_stage(base: &Path) -> Stage {
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        let (read, mut write) = tokio::io::split(core_side);
        let mut reader = Reader::new(read);
        let req: Value =
            serde_json::from_slice(&reader.read_line().await.unwrap().unwrap()).unwrap();
        let reply = serde_json::to_vec(&json!({
            "jsonrpc": "2.0",
            "id": req["id"],
            "result": {
                "generation": 1,
                "version": "fake-core 0.0",
                "framed": true,
                "core": {"id": "core-fake-1", "name": "fake-core"},
                "methods": [],
                "features": [],
            }
        }))
        .unwrap();
        write.write_all(&reply).await.unwrap();
        write.write_all(b"\n").await.unwrap();
        while let Ok(Some(_)) = reader.read_frame().await {}
    });
    let (r, w) = tokio::io::split(client_side);
    let mut stage = Stage::for_test(Client::open(Box::pin(r), Box::pin(w), None).await.unwrap());
    stage.base = base.to_path_buf();
    stage
}

#[tokio::test]
async fn stage_nudge_drives_design_slots_without_nudging_waiting_user() {
    let td = TestDir::new("dg-stage-nudge");
    let sdir = store::studio_dir(td.path(), "acme-shop");
    fs::create_dir_all(&sdir).unwrap();
    let put = |by: EventBy, kind: EventKind| {
        store::append(&sdir, move |_, _| {
            Ok::<_, std::convert::Infallible>(Pending {
                event: NewEvent::new(by.clone(), kind.clone()),
                files: Vec::new(),
            })
        })
        .unwrap()
    };
    let put_entry = |by: EventBy, version: u32, round: u32, slot: DesignSlot| {
        let scratch = td.join(format!("entry-{version}-{round}-{slot:?}.md"));
        fs::write(&scratch, format!("{slot:?} body\n")).unwrap();
        store::append(&sdir, move |_, seq| {
            let kind = design_entry(version, round, seq, slot);
            let EventKind::DesignEntry { ref file, .. } = kind else {
                unreachable!()
            };
            Ok::<_, std::convert::Infallible>(Pending {
                event: NewEvent::new(by.clone(), kind.clone()),
                files: vec![crate::store::PendingFile {
                    src: scratch,
                    dest: file.clone(),
                    append: None,
                }],
            })
        })
        .unwrap()
    };
    let s1 = put(EventBy::role("user"), studio_required());
    put(
        EventBy::role("producer"),
        EventKind::ReqVersion {
            version: 1,
            file: "req/v1.md".into(),
            summary: "req v1".into(),
        },
    );
    put(
        EventBy::role("producer"),
        EventKind::DesignOpened {
            version: 1,
            round: 1,
            suggest_skip: false,
            reason: None,
            doc_file: "design/v1.md".into(),
        },
    );
    for r in ["producer", "writer", "director"] {
        put(EventBy::role("stage"), role_started(r));
    }

    let stage = fake_stage(td.path()).await;
    for r in ["producer", "writer", "director"] {
        stage
            .work
            .borrow_mut()
            .insert(("acme-shop".into(), r.into()), Work::Waiting);
    }
    let live: Vec<LiveTerminal> = ["producer", "writer", "director"]
        .into_iter()
        .map(|r| LiveTerminal {
            name: reconcile::terminal_name("acme-shop", r).unwrap(),
            address: format!("t-{r}"),
            ..Default::default()
        })
        .collect();
    let load = || {
        let evs = store::load_events(&sdir).unwrap().0;
        let last_ts = evs.last().unwrap().ts_ms;
        (evs, last_ts)
    };
    let t0 = s1.ts_ms;
    stage
        .nudge("acme-shop", &sdir, &replay(&load().0), &live, t0 + 1_000)
        .unwrap();
    stage
        .nudge("acme-shop", &sdir, &replay(&load().0), &live, t0 + 200_000)
        .unwrap();
    let evs = load().0;
    let nudge_ev = evs
        .iter()
        .find(|e| {
            matches!(
                &e.kind,
                EventKind::MessageQueued {
                    to,
                    purpose: Some(p),
                    source_seq: Some(3),
                    ..
                } if to == "writer" && p == "nudge"
            )
        })
        .expect("writer nudge");
    assert!(sdir.join("messages/3-nudge.md").exists());

    put(
        EventBy::role("stage"),
        EventKind::MessageDelivered {
            target: nudge_ev.seq,
        },
    );
    let t1 = load().1;
    stage
        .nudge(
            "acme-shop",
            &sdir,
            &replay(&load().0),
            &live,
            t1 + 1_200_000,
        )
        .unwrap();
    assert!(load().0.iter().any(|e| {
        matches!(
            &e.kind,
            EventKind::AttentionRaised { role, reason, .. } if role == "writer" && reason == "owed"
        )
    }));

    put_entry(EventBy::role("writer"), 1, 1, DesignSlot::Proposal);
    let t2 = load().1;
    stage
        .nudge("acme-shop", &sdir, &replay(&load().0), &live, t2 + 1_000)
        .unwrap();
    assert!(load().0.iter().any(|e| {
        matches!(
            &e.kind,
            EventKind::AttentionCleared {
                reason: Some(r),
                ..
            } if r == "done"
        )
    }));

    put_entry(EventBy::role("director"), 1, 1, DesignSlot::Buildability);
    put_entry(EventBy::role("producer"), 1, 1, DesignSlot::Summary);
    let (before, t3) = load();
    stage
        .nudge("acme-shop", &sdir, &replay(&before), &live, t3 + 1_000)
        .unwrap();
    stage
        .nudge(
            "acme-shop",
            &sdir,
            &replay(&load().0),
            &live,
            t3 + 20_000_000,
        )
        .unwrap();
    assert_eq!(load().0.len(), before.len(), "WaitingUser must not nudge");
}
