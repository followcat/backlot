//! Card execution on the stage side (field notes §1.2, §2.2): an edited
//! source file makes a cast card `stale` at apply time, and an applied
//! `reopen-studio` card records `studio.reopened` `by: user` with the
//! card's marker — once, even across a restart.

use std::fs;
use std::path::Path;

use backlot_model::event::EventKind;
use backlot_model::hollywood::{
    CardAction, CardBase, CardSnapshot, HollywoodKind, NewHollywoodEvent, SnapshotFile, sha256_hex,
};
use backlot_model::testkit as tk;
use backlot_model::{EventBy, NewEvent};
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

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

fn hevents(hdir: &Path) -> Vec<Value> {
    store::load_hevents(hdir)
        .unwrap()
        .0
        .iter()
        .map(|e| serde_json::to_value(e).unwrap())
        .collect()
}

/// A studio on `fake`, so apply's runner check passes.
fn fake_studio(base: &Path) {
    let mut kind = tk::studio_kind();
    if let EventKind::StudioCreated { roles, .. } = &mut kind {
        for r in [&mut roles.producer, &mut roles.director, &mut roles.writer] {
            r.runner = "fake".into();
        }
    }
    append_studio(base, "user", kind);
}

/// The card's own cast copy and its source file, the source edited after
/// the proposal — the card is `stale` and adds no `studio.cast`.
#[tokio::test]
async fn an_edited_source_makes_a_cast_card_stale_at_apply() {
    let base = TestDir::new("backlot-card-snapshot-stale");
    fake_studio(&base);
    let hdir = store::hollywood_dir(&base);
    // The card's own copy under its seq.
    let cards = hdir.join("cards/c1");
    fs::create_dir_all(&cards).unwrap();
    let cast_text = format!("{}\nrework_cap = 4\n", cast_roles_text());
    fs::write(cards.join("cast.toml"), &cast_text).unwrap();
    // The source the proposer named, edited after the card existed.
    let source = base.join("proposer-cast.toml");
    fs::write(
        &source,
        cast_text.replace("rework_cap = 4", "rework_cap = 9"),
    )
    .unwrap();

    append_hw(
        &hdir,
        "hollywood",
        HollywoodKind::CardProposed {
            action: CardAction::Cast {
                studio: "acme-shop".into(),
                cast: "cards/c1/cast.toml".into(),
                snapshot: Some(CardSnapshot {
                    cast: Some(SnapshotFile::new(
                        source.to_string_lossy().into_owned(),
                        sha256_hex(cast_text.as_bytes()),
                    )),
                    brief: None,
                }),
            },
            why: "cheaper".into(),
            base: CardBase::CastSeq { was: 0 },
        },
    );
    append_hw(&hdir, "user", HollywoodKind::CardApplied { card: 1 });
    let stage = new_stage(&base).await;
    stage.hcards(&hdir).unwrap();

    let last = hevents(&hdir).pop().unwrap();
    assert_eq!(last["kind"], "card.stale", "{last}");
    assert!(
        last["reason"]
            .as_str()
            .unwrap()
            .contains("the cast file changed after this card was proposed"),
        "{last}"
    );
    // No `studio.cast` landed.
    let (sev, _) = store::load_events(&store::studio_dir(&base, "acme-shop")).unwrap();
    assert!(
        !sev.iter()
            .any(|e| matches!(e.kind, EventKind::StudioCast { .. })),
        "a stale card still changed the cast"
    );
}

/// An applied `reopen-studio` card reopens a finished studio `by: user`,
/// carries the card's marker, and a second pass appends nothing.
#[tokio::test]
async fn a_reopen_studio_card_runs_once() {
    let base = TestDir::new("backlot-card-reopen");
    fake_studio(&base);
    append_studio(
        &base,
        "user",
        EventKind::StudioFinished {
            reason: "done".into(),
        },
    );
    let hdir = store::hollywood_dir(&base);
    append_hw(
        &hdir,
        "hollywood",
        HollywoodKind::CardProposed {
            action: CardAction::ReopenStudio {
                studio: "acme-shop".into(),
            },
            why: "more work".into(),
            base: CardBase::StudioFinished,
        },
    );
    append_hw(&hdir, "user", HollywoodKind::CardApplied { card: 1 });
    let stage = new_stage(&base).await;
    stage.hcards(&hdir).unwrap();

    let state = store::load_state(&store::studio_dir(&base, "acme-shop")).unwrap();
    assert!(state.finished.is_none(), "the studio runs again");
    let reopened = state.reopened.expect("a studio.reopened record");
    assert_eq!(reopened.card.as_deref(), Some("c1"));
    assert_eq!(hevents(&hdir).last().unwrap()["kind"], "card.done");

    // The crash window: the record landed, `card.done` did not. A fresh
    // pass closes the card without a second reopen.
    let before = store::load_events(&store::studio_dir(&base, "acme-shop"))
        .unwrap()
        .0
        .len();
    let again = new_stage(&base).await;
    again.hcards(&hdir).unwrap();
    let after = store::load_events(&store::studio_dir(&base, "acme-shop"))
        .unwrap()
        .0
        .len();
    assert_eq!(before, after, "the reopen ran twice");
}

/// The three fixed roles on `fake` — a cast a card can carry.
fn cast_roles_text() -> String {
    let role = |name: &str, model: &str| {
        format!(
            "[{name}]\nrunner = \"fake\"\nmodel = \"{model}\"\neffort = \"high\"\npermission_profile = \"standard\"\n"
        )
    };
    format!(
        "{}{}{}",
        role("producer", "model-p"),
        role("director", "model-d"),
        role("writer", "model-w"),
    )
}
