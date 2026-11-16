//! Carrying an applied card out (D17-D19): `hcards` re-checks the
//! base against the studio's ledger, records the action `by: user`
//! through the same validation a command crosses, and lands the
//! outcome — `card.done`, `card.stale`, `card.failed` — in
//! Hollywood's own ledger. The never-twice proof: an effect already
//! in the studio records `card.done` without re-appending.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use backlot_model::event::EventKind;
use backlot_model::hollywood::{HollywoodKind, NewHollywoodEvent};
use backlot_model::testkit as tk;
use backlot_model::{EventBy, NewEvent};
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::daemon::Stage;
use crate::store::{self, HPending, Pending};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn tmp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "backlot-hcard-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// `hcards` never calls the core — a hello then silence serves.
async fn new_stage() -> Stage {
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        let (read, mut write) = tokio::io::split(core_side);
        let mut reader = Reader::new(read);
        let hello = reader.read_line().await.unwrap().unwrap();
        let request: Value = serde_json::from_slice(&hello).unwrap();
        assert_eq!(request["method"], "hello");
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
    Stage::for_test(client)
}

/// One studio event into `studios/acme-shop`'s ledger.
fn append(base: &Path, role: &str, kind: EventKind) {
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

/// One event into Hollywood's own ledger.
fn happend(hdir: &Path, role: &str, kind: HollywoodKind) {
    fs::create_dir_all(hdir).unwrap();
    store::append_hollywood(hdir, move |_, _| {
        Ok::<_, std::convert::Infallible>(HPending {
            event: NewHollywoodEvent::new(EventBy::role(role), kind),
            files: Vec::new(),
        })
    })
    .unwrap();
}

/// acme-shop with `t1` running under actor-a — a `pause t1` card's
/// base.
fn running_studio(base: &Path) {
    append(base, "user", tk::studio_kind());
    append(
        base,
        "producer",
        EventKind::ReqVersion {
            version: 1,
            file: "req/v1.md".into(),
            summary: "v1".into(),
        },
    );
    append(base, "director", tk::task_created("t1", "feature", 1));
    append(base, "director", tk::cast("t1", "actor-a", "a"));
}

fn hevents(hdir: &Path) -> Vec<Value> {
    store::load_hevents(hdir)
        .unwrap()
        .0
        .iter()
        .map(|e| serde_json::to_value(e).unwrap())
        .collect()
}

fn sevents(base: &Path) -> Vec<Value> {
    store::load_events(&store::studio_dir(base, "acme-shop"))
        .unwrap()
        .0
        .iter()
        .map(|e| serde_json::to_value(e).unwrap())
        .collect()
}

/// An applied pause card the stage carries out: `task.state` lands
/// `by: user` with the card's marker reason, `card.done` closes it.
#[tokio::test]
async fn an_applied_card_runs_and_records_done() {
    let base = tmp();
    running_studio(&base);
    let hdir = store::hollywood_dir(&base);
    happend(&hdir, "hollywood", tk::hpropose("t1"));
    happend(&hdir, "user", HollywoodKind::CardApplied { card: 1 });
    let mut stage = new_stage().await;
    stage.base = base.clone();

    stage.hcards(&hdir).unwrap();
    let sev = sevents(&base);
    let task_state = sev.iter().find(|e| e["kind"] == "task.state").unwrap();
    assert_eq!(task_state["by"]["role"], "user", "{task_state}");
    assert_eq!(task_state["task"], "t1");
    assert_eq!(task_state["state"], "paused");
    assert!(
        task_state["reason"]
            .as_str()
            .unwrap()
            .starts_with("card c1"),
        "{task_state}"
    );
    let hev = hevents(&hdir);
    assert_eq!(hev.last().unwrap()["kind"].as_str().unwrap(), "card.done");
    assert_eq!(hev.last().unwrap()["by"]["role"], "stage");
    // A second pass: the card is closed — nothing new lands.
    stage.hcards(&hdir).unwrap();
    assert_eq!(hevents(&hdir).len(), hev.len());
    assert_eq!(
        sevents(&base)
            .iter()
            .filter(|e| e["kind"] == "task.state")
            .count(),
        1
    );
}

/// A base that moved after the apply records `card.stale` — and
/// the studio gains nothing.
#[tokio::test]
async fn a_moved_base_is_stale_not_done() {
    let base = tmp();
    running_studio(&base);
    let hdir = store::hollywood_dir(&base);
    happend(&hdir, "hollywood", tk::hpropose("t1"));
    happend(&hdir, "user", HollywoodKind::CardApplied { card: 1 });
    // The task paused between apply and execution.
    append(
        &base,
        "user",
        tk::task_state("t1", backlot_model::event::TaskStateChange::Paused, "hold"),
    );
    let mut stage = new_stage().await;
    stage.base = base.clone();

    stage.hcards(&hdir).unwrap();
    let hev = hevents(&hdir);
    let last = hev.last().unwrap();
    assert_eq!(last["kind"], "card.stale", "{last}");
    assert_eq!(last["by"]["role"], "stage");
    assert!(last["reason"].as_str().unwrap().contains("paused"));
    assert!(
        !sevents(&base)[..sevents(&base).len() - 1]
            .iter()
            .any(|e| e["reason"].as_str().unwrap_or("").starts_with("card c1")),
        "a stale card still acted"
    );
}

/// A card whose studio does not exist fails with the reason — the
/// gate's refusal, never silent.
#[tokio::test]
async fn a_card_on_a_missing_studio_fails() {
    let base = tmp();
    let hdir = store::hollywood_dir(&base);
    happend(&hdir, "hollywood", tk::hpropose("t1"));
    happend(&hdir, "user", HollywoodKind::CardApplied { card: 1 });
    let mut stage = new_stage().await;
    stage.base = base.clone();

    stage.hcards(&hdir).unwrap();
    let last = hevents(&hdir).pop().unwrap();
    assert_eq!(last["kind"], "card.failed", "{last}");
    assert!(
        last["reason"].as_str().unwrap().contains("no studio"),
        "{last}"
    );
}

/// The crash window: the action landed but `card.done` did not —
/// the next pass finds the card marker already in the studio and
/// closes the card without a second `task.state`.
#[tokio::test]
async fn an_effect_already_landed_never_repeats() {
    let base = tmp();
    running_studio(&base);
    let hdir = store::hollywood_dir(&base);
    happend(&hdir, "hollywood", tk::hpropose("t1"));
    happend(&hdir, "user", HollywoodKind::CardApplied { card: 1 });
    // As if the first pass acted then died before `card.done`: the
    // studio carries this card's marker already.
    append(
        &base,
        "user",
        tk::task_state(
            "t1",
            backlot_model::event::TaskStateChange::Paused,
            "card c1: actor is looping",
        ),
    );
    let mut stage = new_stage().await;
    stage.base = base.clone();

    stage.hcards(&hdir).unwrap();
    let last = hevents(&hdir).pop().unwrap();
    assert_eq!(last["kind"], "card.done", "{last}");
    assert_eq!(
        sevents(&base)
            .iter()
            .filter(|e| e["kind"] == "task.state")
            .count(),
        1,
        "the card's action ran twice"
    );
}

/// The smallest legal cast — the three fixed roles on `fake`.
const CAST: &str = "[producer]\nrunner = \"fake\"\nmodel = \"m\"\neffort = \"high\"\n\
permission_profile = \"standard\"\n\
[director]\nrunner = \"fake\"\nmodel = \"m\"\neffort = \"high\"\n\
permission_profile = \"standard\"\n\
[writer]\nrunner = \"fake\"\nmodel = \"m\"\neffort = \"high\"\n\
permission_profile = \"standard\"\n";

/// `git init` the fixture repo — the dry-run asks git itself.
fn git_repo(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    let out = std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .arg(dir)
        .output()
        .expect("git init");
    assert!(out.status.success());
}

/// `studio.created` under the card's own title — what this card's
/// earlier pass left before the crash, never someone else's studio.
fn studio_kind_titled(title: &str) -> EventKind {
    let mut k = tk::studio_kind();
    if let EventKind::StudioCreated { title: t, .. } = &mut k {
        *t = title.into();
    }
    k
}

/// A studio-new card's two ledger events — proposal then apply;
/// returns the card's seq (its proposal's).
fn propose_studio_new(hdir: &Path, studio: &str, repo: &Path, why: &str) -> u64 {
    let draft = format!("drafts/{studio}");
    fs::create_dir_all(hdir.join(&draft)).unwrap();
    fs::write(hdir.join(format!("{draft}/cast.toml")), CAST).unwrap();
    fs::write(hdir.join(format!("{draft}/brief.md")), "the brief\n").unwrap();
    happend(
        hdir,
        "hollywood",
        HollywoodKind::CardProposed {
            action: backlot_model::hollywood::CardAction::StudioNew {
                studio: studio.into(),
                title: studio.into(),
                repo: repo.to_string_lossy().into_owned(),
                cast: format!("{draft}/cast.toml"),
                brief: format!("{draft}/brief.md"),
            },
            why: why.into(),
            base: backlot_model::hollywood::CardBase::StudioAbsent,
        },
    );
    let card = hevents(hdir).last().unwrap()["seq"].as_u64().unwrap();
    happend(hdir, "user", HollywoodKind::CardApplied { card });
    card
}

/// A studio-new card: the draft re-validates, `studio.created`
/// lands with the cast copied in, and the brief queues as the
/// producer's first message — `purpose: "brief"` marks it.
#[tokio::test]
async fn a_studio_new_card_makes_the_studio_and_brief() {
    let base = tmp();
    let hdir = store::hollywood_dir(&base);
    fs::create_dir_all(hdir.join("drafts/acme-beta")).unwrap();
    let cast = hdir.join("drafts/acme-beta/cast.toml");
    fs::write(&cast, CAST).unwrap();
    let brief = hdir.join("drafts/acme-beta/brief.md");
    fs::write(&brief, "the beta brief\n").unwrap();
    let repo = tmp().join("repo-beta");
    git_repo(&repo);
    happend(
        &hdir,
        "hollywood",
        HollywoodKind::CardProposed {
            action: backlot_model::hollywood::CardAction::StudioNew {
                studio: "acme-beta".into(),
                title: "acme-beta".into(),
                repo: repo.to_string_lossy().into_owned(),
                cast: "drafts/acme-beta/cast.toml".into(),
                brief: "drafts/acme-beta/brief.md".into(),
            },
            why: "a second shop".into(),
            base: backlot_model::hollywood::CardBase::StudioAbsent,
        },
    );
    happend(&hdir, "user", HollywoodKind::CardApplied { card: 1 });
    let mut stage = new_stage().await;
    stage.base = base.clone();

    stage.hcards(&hdir).unwrap();
    let sdir = store::studio_dir(&base, "acme-beta");
    let (sev, _) = store::load_events(&sdir).unwrap();
    let kinds: Vec<&str> = sev.iter().map(|e| e.kind.name()).collect();
    assert_eq!(
        kinds,
        vec!["studio.created", "message.queued"],
        "{kinds:?} hev: {:?}",
        hevents(&hdir)
    );
    match &sev[0].kind {
        EventKind::StudioCreated { title, .. } => assert_eq!(title, "acme-beta"),
        k => panic!("expected studio.created, got {k:?}"),
    }
    match &sev[1].kind {
        EventKind::MessageQueued { to, purpose, .. } => {
            assert_eq!(to, "producer");
            assert_eq!(purpose.as_deref(), Some("brief"));
        }
        k => panic!("expected message.queued, got {k:?}"),
    }
    assert!(sdir.join("cast.toml").is_file());
    let last = hevents(&hdir).pop().unwrap();
    assert_eq!(last["kind"], "card.done", "{last}");

    // The crash window again: the brief alone missing — only it
    // lands on the next pass, never a second studio.created.
    let base2 = tmp();
    let hdir2 = store::hollywood_dir(&base2);
    fs::create_dir_all(hdir2.join("drafts/acme-gamma")).unwrap();
    let cast2 = hdir2.join("drafts/acme-gamma/cast.toml");
    fs::write(&cast2, CAST).unwrap();
    let brief2 = hdir2.join("drafts/acme-gamma/brief.md");
    fs::write(&brief2, "the gamma brief\n").unwrap();
    let repo2 = tmp().join("repo-gamma");
    git_repo(&repo2);
    let sdir2 = store::studio_dir(&base2, "acme-gamma");
    fs::create_dir_all(&sdir2).unwrap();
    // studio.created landed already (after this card's apply) —
    // the brief's append never made it.
    happend(
        &hdir2,
        "hollywood",
        HollywoodKind::CardProposed {
            action: backlot_model::hollywood::CardAction::StudioNew {
                studio: "acme-gamma".into(),
                title: "acme-gamma".into(),
                repo: repo2.to_string_lossy().into_owned(),
                cast: "drafts/acme-gamma/cast.toml".into(),
                brief: "drafts/acme-gamma/brief.md".into(),
            },
            why: "the gamma shop".into(),
            base: backlot_model::hollywood::CardBase::StudioAbsent,
        },
    );
    happend(&hdir2, "user", HollywoodKind::CardApplied { card: 1 });
    store::append(&sdir2, move |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(EventBy::role("user"), studio_kind_titled("acme-gamma")),
            files: Vec::new(),
        })
    })
    .unwrap();
    let mut stage2 = new_stage().await;
    stage2.base = base2.clone();
    stage2.hcards(&hdir2).unwrap();
    let (sev2, _) = store::load_events(&sdir2).unwrap();
    let kinds2: Vec<&str> = sev2.iter().map(|e| e.kind.name()).collect();
    assert_eq!(
        kinds2,
        vec!["studio.created", "message.queued"],
        "{kinds2:?}"
    );
    let last = hevents(&hdir2).pop().unwrap();
    assert_eq!(last["kind"], "card.done", "{last}");
}

/// R1-2: the marker match ends at the colon — card c1 does not
/// claim card c12's `task.state`. The base it did move lands the
/// card `stale`, never a borrowed `card.done`.
#[tokio::test]
async fn another_cards_marker_is_not_this_cards() {
    let base = tmp();
    running_studio(&base);
    let hdir = store::hollywood_dir(&base);
    happend(&hdir, "hollywood", tk::hpropose("t1"));
    happend(&hdir, "user", HollywoodKind::CardApplied { card: 1 });
    // Card c12 paused the task already — not this card's doing.
    append(
        &base,
        "user",
        tk::task_state(
            "t1",
            backlot_model::event::TaskStateChange::Paused,
            "card c12: on hold",
        ),
    );
    let mut stage = new_stage().await;
    stage.base = base.clone();

    stage.hcards(&hdir).unwrap();
    let last = hevents(&hdir).pop().unwrap();
    assert_eq!(last["kind"], "card.stale", "{last}");
    assert_eq!(
        sevents(&base)
            .iter()
            .filter(|e| e["kind"] == "task.state")
            .count(),
        1,
        "card c1 claimed c12's action"
    );
}

/// R1-3: a second studio-new card for the same name is not "done" —
/// the first card's brief is not its own, so the base moved and the
/// card ends `stale` with nothing appended.
#[tokio::test]
async fn a_second_studio_new_card_is_stale_not_done() {
    let base = tmp();
    let hdir = store::hollywood_dir(&base);
    let repo = tmp().join("repo-beta");
    git_repo(&repo);
    let c1 = propose_studio_new(&hdir, "acme-beta", &repo, "a second shop");
    let mut stage = new_stage().await;
    stage.base = base.clone();
    stage.hcards(&hdir).unwrap();
    assert_eq!(hevents(&hdir).last().unwrap()["kind"], "card.done");

    let c2 = propose_studio_new(&hdir, "acme-beta", &repo, "again");
    assert_ne!(c1, c2);
    stage.hcards(&hdir).unwrap();
    let last = hevents(&hdir).pop().unwrap();
    assert_eq!(last["kind"], "card.stale", "{last}");
    // Nothing appended — no second created, no second brief.
    let sdir = store::studio_dir(&base, "acme-beta");
    let (sev, _) = store::load_events(&sdir).unwrap();
    let kinds: Vec<&str> = sev.iter().map(|e| e.kind.name()).collect();
    assert_eq!(kinds, vec!["studio.created", "message.queued"], "{kinds:?}");
}

/// An applied card runs whether or not `[hollywood]` is configured
/// — the config gates the agent, never the gate.
#[tokio::test]
async fn cards_run_without_the_layer_configured() {
    let base = tmp();
    running_studio(&base);
    let hdir = store::hollywood_dir(&base);
    happend(&hdir, "hollywood", tk::hpropose("t1"));
    happend(&hdir, "user", HollywoodKind::CardApplied { card: 1 });
    let mut stage = new_stage().await;
    stage.base = base.clone();
    // No config file anywhere — `env_vars` empty is the absent case.
    stage.env_vars = BTreeMap::new();

    stage.hcards(&hdir).unwrap();
    let last = hevents(&hdir).pop().unwrap();
    assert_eq!(last["kind"], "card.done", "{last}");
}
