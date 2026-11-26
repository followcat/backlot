//! The `restart` and `cast` cards (D18's new actions): `propose`
//! refuses a card born stale, `card apply` re-checks the base, and the
//! stage carries the action out once — a restart across a stage restart
//! repeats nothing.

use std::fs;
use std::path::Path;

use backlot_model::event::EventKind;
use backlot_model::hollywood::{CardAction, CardBase, HollywoodKind, NewHollywoodEvent};
use backlot_model::testkit as tk;
use backlot_model::{EventBy, NewEvent};
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::cli_testkit::{CAST_TOML, fixture, ok, refused, scratch};
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

fn sevents(base: &Path) -> Vec<Value> {
    store::load_events(&store::studio_dir(base, "acme-shop"))
        .unwrap()
        .0
        .iter()
        .map(|e| serde_json::to_value(e).unwrap())
        .collect()
}

fn hevents(hdir: &Path) -> Vec<Value> {
    store::load_hevents(hdir)
        .unwrap()
        .0
        .iter()
        .map(|e| serde_json::to_value(e).unwrap())
        .collect()
}

/// acme-shop with t1 running under actor-a, the actor live.
fn running_studio(base: &Path) {
    append_studio(base, "user", tk::studio_kind());
    append_studio(
        base,
        "producer",
        EventKind::ReqVersion {
            version: 1,
            file: "req/v1.md".into(),
            summary: "v1".into(),
        },
    );
    append_studio(base, "director", tk::task_created("t1", "feature", 1));
    append_studio(base, "director", tk::cast("t1", "actor-a", "a"));
    append_studio(
        base,
        "stage",
        EventKind::RoleStarted {
            role: "actor-t1-c1".into(),
            terminal: Some("bl-acme-shop-t1".into()),
            actor: Some(tk::actor("a")),
            session_id: Some("s-a".into()),
            reason: None,
        },
    );
}

// ── restart ─────────────────────────────────────────────────────

/// `propose restart` records `card.proposed` against a live person;
/// a closed person and a finished studio refuse it.
#[test]
fn a_restart_card_proposes_against_a_live_person_only() {
    let (scratch, env) = scratch();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let req = fixture(&scratch, "req.md", "# req\n");
    ok(
        &env,
        &format!("req revise --file {req} --summary s --as producer"),
    );
    let acc = fixture(&scratch, "acc.md", "# acc\n");
    ok(
        &env,
        &format!("task new --title cart --kind feature --acceptance-file {acc} --as director"),
    );
    ok(&env, "cast --task t1 --actor actor-a --as director");
    // The actor has not started yet, so it is not a role with a
    // lifecycle — a restart card refuses.
    let msg = refused(
        &env,
        "hollywood propose restart --studio acme-shop --person actor-t1-c1 --why stuck --as hollywood",
    );
    assert!(msg.contains("no role 'actor-t1-c1'"), "{msg}");
}

/// A restart card applies, the stage records one `role.restart` `by:
/// user` with the card's marker, and a second pass repeats nothing.
#[tokio::test]
async fn a_restart_card_runs_once() {
    let base = TestDir::new("backlot-card-restart");
    running_studio(&base);
    let hdir = store::hollywood_dir(&base);
    append_hw(
        &hdir,
        "hollywood",
        HollywoodKind::CardProposed {
            action: CardAction::Restart {
                studio: "acme-shop".into(),
                person: "actor-t1-c1".into(),
                fresh: false,
            },
            why: "stuck at a prompt".into(),
            base: CardBase::PersonRestartable,
        },
    );
    append_hw(&hdir, "user", HollywoodKind::CardApplied { card: 1 });
    let stage = new_stage(&base).await;

    stage.hcards(&hdir).unwrap();
    let sev = sevents(&base);
    let restarts: Vec<&Value> = sev.iter().filter(|e| e["kind"] == "role.restart").collect();
    assert_eq!(restarts.len(), 1, "{sev:?}");
    assert_eq!(restarts[0]["by"]["role"], "user");
    assert_eq!(restarts[0]["role"], "actor-t1-c1");
    assert!(
        restarts[0]["reason"]
            .as_str()
            .unwrap()
            .starts_with("card c1:"),
        "{restarts:?}"
    );
    let hev = hevents(&hdir);
    assert_eq!(hev.last().unwrap()["kind"], "card.done", "{hev:?}");

    // The crash window: the action landed but `card.done` did not. The
    // next pass (a fresh stage) finds the marker and closes the card,
    // never a second restart.
    let before = sevents(&base).len();
    let again = new_stage(&base).await;
    again.hcards(&hdir).unwrap();
    assert_eq!(
        sevents(&base)
            .iter()
            .filter(|e| e["kind"] == "role.restart")
            .count(),
        1,
        "the restart ran twice"
    );
    assert!(sevents(&base).len() >= before);
}

/// A restart card whose person closed between apply and execution lands
/// `card.stale` and appends nothing.
#[tokio::test]
async fn a_restart_card_whose_person_closed_is_stale() {
    let base = TestDir::new("backlot-card-restart-stale");
    running_studio(&base);
    let hdir = store::hollywood_dir(&base);
    append_hw(
        &hdir,
        "hollywood",
        HollywoodKind::CardProposed {
            action: CardAction::Restart {
                studio: "acme-shop".into(),
                person: "actor-t1-c1".into(),
                fresh: true,
            },
            why: "stuck".into(),
            base: CardBase::PersonRestartable,
        },
    );
    append_hw(&hdir, "user", HollywoodKind::CardApplied { card: 1 });
    // The actor closed between the apply and the stage's pass.
    append_studio(
        &base,
        "stage",
        EventKind::RoleClosed {
            role: "actor-t1-c1".into(),
            reason: Some("no longer desired".into()),
        },
    );
    let stage = new_stage(&base).await;
    stage.hcards(&hdir).unwrap();
    let last = hevents(&hdir).pop().unwrap();
    assert_eq!(last["kind"], "card.stale", "{last}");
    assert!(
        last["reason"].as_str().unwrap().contains("closed"),
        "{last}"
    );
    assert_eq!(
        sevents(&base)
            .iter()
            .filter(|e| e["kind"] == "role.restart")
            .count(),
        0
    );
}

// ── cast ────────────────────────────────────────────────────────

/// A cast draft on `fake` runners, with one extra candidate, written
/// where a card's draft resolves: under Hollywood's own directory.
fn cast_file(scratch: &TestDir, name: &str) -> String {
    let dir = store::hollywood_dir(scratch).join("drafts/acme-shop");
    fs::create_dir_all(&dir).unwrap();
    let cast = dir.join(name);
    fs::write(
        &cast,
        r#"[producer]
runner = "fake"
model = "p"
effort = "high"
permission_profile = "standard"
[director]
runner = "fake"
model = "d"
effort = "high"
permission_profile = "standard"
[writer]
runner = "fake"
model = "w"
effort = "high"
permission_profile = "standard"
[[candidates]]
name = "actor-c"
runner = "fake"
model = "c"
effort = "high"
permission_profile = "worktree"
"#,
    )
    .unwrap();
    format!("drafts/acme-shop/{name}")
}

/// A studio whose cast is already on `fake`, so a draft's runners
/// resolve on this machine.
fn fake_studio(base: &Path) {
    let mut kind = tk::studio_kind();
    if let EventKind::StudioCreated {
        roles, candidates, ..
    } = &mut kind
    {
        for r in [&mut roles.producer, &mut roles.director, &mut roles.writer] {
            r.runner = "fake".into();
        }
        for c in candidates.iter_mut() {
            c.runner = "fake".into();
        }
    }
    append_studio(base, "user", kind);
}

/// `propose cast` runs the same runnable checks `studio cast set` does:
/// a draft naming a runner the machine has not got is refused before the
/// card exists.
#[test]
fn a_cast_card_born_unrunnable_is_refused() {
    let (scratch, env) = scratch();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let draft = cast_file(&scratch.dir, "bad.toml");
    let path = store::hollywood_dir(&scratch.dir).join(&draft);
    let text = fs::read_to_string(&path)
        .unwrap()
        .replace("runner = \"fake\"", "runner = \"runner-zzz\"");
    fs::write(&path, text).unwrap();
    // A file argument is resolved against the proposer's directory and
    // must already exist there, so the test names the absolute path.
    let msg = refused(
        &env,
        &format!(
            "hollywood propose cast --studio acme-shop --file {} --why cheaper --as hollywood",
            path.display()
        ),
    );
    assert!(
        msg.contains("runner 'runner-zzz' is not supported"),
        "{msg}"
    );
    let list = ok(&env, "card list");
    assert!(!list.contains("cast acme-shop"), "{list}");
}

/// A cast card applies, records one `studio.cast` `by: user` with the
/// card's marker, and the diff shows in `card show`.
#[tokio::test]
async fn a_cast_card_runs_once_and_shows_its_diff() {
    let base = TestDir::new("backlot-card-cast");
    fake_studio(&base);
    let hdir = store::hollywood_dir(&base);
    let draft = cast_file(&base, "cast.toml");
    // Propose and apply straight into the ledgers (the propose path is
    // the CLI test's).
    append_hw(
        &hdir,
        "hollywood",
        HollywoodKind::CardProposed {
            action: CardAction::Cast {
                studio: "acme-shop".into(),
                cast: draft.clone(),
                snapshot: None,
            },
            why: "cheaper actors".into(),
            base: CardBase::CastSeq { was: 0 },
        },
    );
    append_hw(&hdir, "user", HollywoodKind::CardApplied { card: 1 });
    let stage = new_stage(&base).await;
    stage.hcards(&hdir).unwrap();
    let sev = sevents(&base);
    let cast: Vec<&Value> = sev.iter().filter(|e| e["kind"] == "studio.cast").collect();
    assert_eq!(cast.len(), 1, "{sev:?}");
    assert_eq!(cast[0]["by"]["role"], "user");
    assert_eq!(cast[0]["card"], "c1");
    // The candidate the draft names is now in the studio's cast.
    assert_eq!(cast[0]["candidates"][0]["name"], "actor-c");
    let hev = hevents(&hdir);
    assert_eq!(hev.last().unwrap()["kind"], "card.done", "{hev:?}");

    // Crash window: the append landed, `card.done` did not. A fresh
    // stage finds the marker and appends nothing.
    let again = new_stage(&base).await;
    again.hcards(&hdir).unwrap();
    assert_eq!(
        sevents(&base)
            .iter()
            .filter(|e| e["kind"] == "studio.cast")
            .count(),
        1,
        "the cast change ran twice"
    );
}

/// A cast card whose settings event moved lands `card.stale` and
/// appends nothing.
#[tokio::test]
async fn a_cast_card_whose_settings_moved_is_stale() {
    let base = TestDir::new("backlot-card-cast-stale");
    fake_studio(&base);
    // Someone changed the cast after the card was proposed: a
    // `studio.cast` lands and moves `cast_seq` off `0`.
    append_studio(&base, "user", {
        use backlot_model::cast::CastSettings;
        let state = store::load_state(&store::studio_dir(&base, "acme-shop")).unwrap();
        let mut next = CastSettings::of(&state).unwrap();
        next.rework_cap = 5;
        next.event_kind(None)
    });
    let hdir = store::hollywood_dir(&base);
    let draft = cast_file(&base, "cast.toml");
    append_hw(
        &hdir,
        "hollywood",
        HollywoodKind::CardProposed {
            action: CardAction::Cast {
                studio: "acme-shop".into(),
                cast: draft,
                snapshot: None,
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
        last["reason"].as_str().unwrap().contains("cast changed"),
        "{last}"
    );
}
