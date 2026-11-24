//! Feedback where a recast happens: `backlot recast` CLI and
//! Hollywood card outcomes state when an unstarted writer starts.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use backlot_model::event::{ActorIdentity, EventKind};
use backlot_model::hollywood::{CardAction, CardBase, HollywoodKind, NewHollywoodEvent, replay};
use backlot_model::testkit as tk;
use backlot_model::{EventBy, NewEvent};
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::cli_testkit::{CAST_TOML, fixture, ok, scratch};
use crate::daemon::Stage;
use crate::store::{self, HPending, Pending};
use crate::test_support::TestDir;

async fn new_stage(base: &Path) -> Stage {
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        let (read, mut write) = tokio::io::split(core_side);
        let mut reader = Reader::new(read);
        let hello = reader.read_line().await.unwrap().unwrap();
        let request: Value = serde_json::from_slice(&hello).unwrap();
        let reply = serde_json::to_vec(&json!({
            "jsonrpc": "2.0", "id": request["id"],
            "result": {
                "generation": 1, "version": "fake-core 0.0",
                "framed": true,
                "core": {"id": "core-fake-1", "name": "fake-core"},
                "methods": [], "features": [],
            }
        }))
        .unwrap();
        write.write_all(&reply).await.unwrap();
        write.write_all(b"\n").await.unwrap();
        while let Ok(Some(_)) = reader.read_frame().await {}
    });
    let (read, write) = tokio::io::split(client_side);
    let client = Client::open(Box::pin(read), Box::pin(write), None)
        .await
        .unwrap();
    let mut stage = Stage::for_test(client);
    stage.base = base.to_path_buf();
    stage
}

fn append_studio(base: &Path, role: &str, kind: EventKind) {
    let dir = store::studio_dir(base, "acme-shop");
    fs::create_dir_all(&dir).unwrap();
    store::append(&dir, move |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(EventBy::role(role), kind),
            files: Vec::new(),
        })
    })
    .unwrap();
}

fn append_hw(hdir: &Path, role: &str, kind: HollywoodKind) {
    fs::create_dir_all(hdir).unwrap();
    store::append_hollywood(hdir, move |_, _| {
        Ok::<_, std::convert::Infallible>(HPending {
            event: NewHollywoodEvent::new(EventBy::role(role), kind),
            files: Vec::new(),
        })
    })
    .unwrap();
}

#[test]
fn recast_cli_explains_when_writer_has_not_started_and_is_quiet_once_started() {
    let (s, env) = scratch();
    let cast = fixture(&s, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );

    let out = ok(
        &env,
        "--as user recast --person writer --runner fake --model new-writer --effort high --reason quota",
    );
    assert!(
        out.contains("writer starts when the director asks for a plan"),
        "expected unstarted writer feedback, got: {out}"
    );

    let prod_out = ok(
        &env,
        "--as user recast --person producer --runner fake --model new-prod --effort high --reason quota",
    );
    assert!(
        prod_out.contains("producer starts with the studio"),
        "expected unstarted producer feedback, got: {prod_out}"
    );

    // Once director starts, recasting director no longer carries the unstarted note.
    append_studio(
        &s.dir,
        "stage",
        EventKind::RoleStarted {
            role: "director".into(),
            terminal: Some("bl-acme-shop-director".into()),
            actor: None,
            session_id: Some("sess-d".into()),
            reason: None,
        },
    );
    let dir_out = ok(
        &env,
        "--as user recast --person director --runner fake --model new-dir --effort high --reason quota",
    );
    assert!(
        !dir_out.contains("starts with the studio")
            && !dir_out.contains("starts when the director asks for a plan"),
        "started role should not carry unstarted note: {dir_out}"
    );
}

#[tokio::test]
async fn hcard_recast_outcome_explains_when_writer_has_not_started() {
    let base = TestDir::new("backlot-recast-card-test");
    append_studio(&base, "user", tk::studio_kind());
    let hdir = store::hollywood_dir(&base);
    let new_actor = ActorIdentity {
        runner: "fake".into(),
        runner_version: None,
        model: "new-writer".into(),
        effort: Some("high".into()),
        permission_profile: "standard".into(),
    };
    append_hw(
        &hdir,
        "hollywood",
        HollywoodKind::CardProposed {
            action: CardAction::Recast {
                studio: "acme-shop".into(),
                person: "writer".into(),
                actor: new_actor,
            },
            why: "quota".into(),
            base: CardBase::RoleIdentity {
                actor: tk::actor("wri"),
            },
        },
    );
    append_hw(&hdir, "user", HollywoodKind::CardApplied { card: 1 });

    let stage = new_stage(&base).await;
    stage.hcards(&hdir).unwrap();

    let (hevents, _) = store::load_hevents(&hdir).unwrap();
    let hstate = replay(&hevents);
    let card = hstate.cards.get(&1).expect("card c1");
    assert_eq!(
        card.detail,
        "writer starts when the director asks for a plan"
    );

    // A clean `card.done` is passive context (S2): the note rides along
    // with the next delivery instead of waking Hollywood on its own.
    let passive = crate::hnotice::passive_items(
        &BTreeMap::new(),
        &hstate,
        &std::collections::BTreeSet::new(),
    );
    let outcome = passive
        .iter()
        .find(|p| p.text.starts_with("c1 done"))
        .expect("card c1 passive outcome");
    assert_eq!(
        outcome.text,
        "c1 done — writer starts when the director asks for a plan"
    );
}
