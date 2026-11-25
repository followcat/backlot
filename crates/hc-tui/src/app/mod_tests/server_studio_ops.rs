//! B1b: a server-connected TUI creates, finishes, prunes, deletes and
//! restores studios over `plugin.backlot.req`/`res` — probes, plans
//! and commits, with the plan's confirm token riding the commit.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use backlot_model::req_results::{StudioFinishResult, StudioNewResult, StudioPlan};
use backlot_model::req_wire::{REQ_DOC_NAME, ReqAction, ReqDoc, ReqEnvelope, ResAck};
use serde_json::{Value, json};

use super::delete_studio::drain_dialog_fetch;
use super::lifecycle_fixtures::ARCHIVED_JSON;
use super::server_doc_fake::{MemDocs, spawn_doc_core_with};
use super::*;
use crate::app::input::UiEvent;
use crate::new_studio::{Probe, Step};
use crate::studio::StudioKey;
use crate::studio_cmds;
use crate::studio_dialog::{ConfirmFor, StudioDialog};
use hc_vt::{Key, Mods};

pub(super) fn server_app(store: Arc<Mutex<MemDocs>>) -> App {
    let mut app = app_with(&[]);
    app.set_plain(false);
    app.area = (120, 40);
    app.view.link = hc_proto::watch::Link::Online;
    app.view.rpc = hc_proto::watch::Link::Online;
    app.view.core = Some(hc_proto::hello::CoreInfo {
        id: "core-srv".into(),
        name: "backlot".into(),
        hostname: None,
    });
    let (rpc, _) = spawn_doc_core_with(store, None);
    app.set_server_target(Some(backlot_stage::server_cfg::ServerTarget {
        host: "host-a".into(),
        session: "backlot".into(),
    }));
    app.server_ack_timeout = Duration::from_secs(1);
    app.set_operations(rpc);
    app
}

pub(super) fn key(app: &mut App, k: Key) {
    app.on_input(
        UiEvent::Key(term_key(k, Mods::NONE, hc_vt::Action::Press)),
        app.area.0,
    );
}

pub(super) async fn drain_ns(app: &mut App) {
    for _ in 0..40 {
        tokio::task::yield_now().await;
        while let Ok(reply) = app.ns_rx.try_recv() {
            app.on_ns_reply(reply);
        }
    }
}

pub(super) fn seed(store: &Arc<Mutex<MemDocs>>, kind: &str, acks: Vec<ResAck>) {
    store
        .lock()
        .unwrap()
        .by_kind
        .insert(kind.to_owned(), acks.into());
}

pub(super) fn ok_ack(value: Value) -> ResAck {
    ResAck::ok_value("pending", 1, value)
}

pub(super) fn plan_ack(report: Value, token: &str) -> ResAck {
    ok_ack(
        serde_json::to_value(StudioPlan {
            report,
            confirm_token: token.to_owned(),
        })
        .unwrap(),
    )
}

/// Every request the client queued, in order, with its confirm token.
pub(super) fn queued(store: &Mutex<MemDocs>) -> Vec<(ReqAction, Option<String>)> {
    let st = store.lock().unwrap();
    let (_, val) = st.docs.get(REQ_DOC_NAME).expect("a request was written");
    let doc: ReqDoc = serde_json::from_value(val.clone()).unwrap();
    doc.queue
        .iter()
        .map(|entry| {
            let env: ReqEnvelope = serde_json::from_value(entry.clone()).unwrap();
            (
                serde_json::from_value(env.action).unwrap(),
                env.confirm_token,
            )
        })
        .collect()
}

pub(super) fn repos_answer() -> Value {
    json!({
        "cwd": "/home/user",
        "entries": [
            { "name": "acme-shop", "path": "/home/user/acme-shop",
              "kind": "git_ready", "in_use_studio": null, "branch": "main" }
        ]
    })
}

pub(super) fn runners_answer() -> Value {
    json!([{ "runner": "fake", "status": "found", "version": "1",
             "path": "/home/user/bin/fake", "needs_path": false,
             "models": ["model-f"], "efforts": ["medium"] }])
}

pub(super) fn new_studio(seq: Option<u64>) -> Value {
    serde_json::to_value(StudioNewResult {
        studio: "acme-shop".into(),
        title: "Acme shop".into(),
        repo: "/home/user/acme-shop".into(),
        dry_run: seq.is_none(),
        seq,
        ask_seq: seq.map(|_| 2),
        first_message: seq.is_none().then(|| "Build the shop".into()),
    })
    .unwrap()
}

pub(super) fn delete_report(dry: bool, archived: bool) -> Value {
    json!({
        "studio": "acme-shop", "dry_run": dry, "force": false, "finished": false,
        "closed_terminals": [], "removed_worktrees": ["work/t1"],
        "pruned_worktrees": [], "deleted_branches": [],
        "kept": [], "archive": "archive/acme-shop-20260101T120000Z",
        "archived": archived
    })
}

pub(super) fn prune_report(dry: bool) -> Value {
    json!({ "studios": [{ "studio": "acme-shop", "finished": true, "dry_run": dry,
                           "steps": [{ "RemovedWorktree": "work/t1" }] }] })
}

pub(super) fn restore_report(dry: bool) -> Value {
    json!({
        "archived": "old-shop-20260101T120000Z", "name": "old-shop",
        "dest": "/home/user/state/studios/old-shop", "dry_run": dry, "renamed": true
    })
}

#[tokio::test]
async fn the_new_studio_dialog_probes_and_creates_over_the_queue() {
    let store = Arc::new(Mutex::new(MemDocs::default()));
    seed(&store, "version", vec![ok_ack(json!("backlot 0.4.5"))]);
    seed(&store, "repos", vec![ok_ack(repos_answer())]);
    seed(&store, "runners", vec![ok_ack(runners_answer())]);
    seed(
        &store,
        "studio-new",
        vec![ok_ack(new_studio(None)), ok_ack(new_studio(Some(1)))],
    );
    let mut app = server_app(store.clone());

    app.new_studio_open(None);
    drain_ns(&mut app).await;
    assert!(matches!(
        app.new_studio.as_ref().unwrap().machines[0].backlot,
        Probe::Done(ref v) if v == "0.4.5"
    ));

    if app.new_studio.as_ref().unwrap().pane == crate::new_studio::Step1Pane::Machines {
        key(&mut app, Key::Tab); // Machines → Repos
    }
    key(&mut app, Key::Enter); // pick acme-shop
    assert_eq!(app.new_studio.as_ref().unwrap().step, Step::IdTitle);
    app.new_studio
        .as_mut()
        .unwrap()
        .build
        .insert("Build the shop");
    key(&mut app, Key::Enter); // → Cast (runners probe)
    drain_ns(&mut app).await;
    assert_eq!(app.new_studio.as_ref().unwrap().step, Step::Cast);
    key(&mut app, Key::Enter); // → CheckStart (dry run)
    drain_ns(&mut app).await;
    assert_eq!(app.new_studio.as_ref().unwrap().step, Step::CheckStart);
    key(&mut app, Key::Enter); // create
    drain_ns(&mut app).await;
    assert!(app.new_studio.is_none(), "the wizard closes on create");

    let calls = queued(&store);
    assert!(
        calls.iter().any(|(a, _)| matches!(a, ReqAction::Version)),
        "the version probe went through the queue"
    );
    assert!(calls.iter().any(|(a, _)| matches!(a, ReqAction::Runners)));
    assert!(
        calls
            .iter()
            .any(|(a, _)| matches!(a, ReqAction::Repos { .. }))
    );
    let news: Vec<&ReqAction> = calls
        .iter()
        .map(|(a, _)| a)
        .filter(|a| matches!(a, ReqAction::StudioNew { .. }))
        .collect();
    assert_eq!(news.len(), 2, "dry run plus create: {news:?}");
    for action in &news {
        let ReqAction::StudioNew {
            id,
            repo,
            cast,
            ask,
            ..
        } = action
        else {
            unreachable!()
        };
        assert_eq!(id, "acme-shop");
        assert_eq!(repo, "/home/user/acme-shop", "the hub canonicalizes");
        assert!(cast.contains("runner = \"fake\""), "{cast}");
        assert_eq!(
            ask.as_deref(),
            Some("Build the shop\n"),
            "the brief travels as content, not a client --ask-file"
        );
    }
    assert!(matches!(
        news[1],
        ReqAction::StudioNew {
            dry_run: false,
            init: false,
            ..
        }
    ));
}

#[tokio::test]
async fn finish_prune_and_delete_travel_over_the_queue() {
    let store = Arc::new(Mutex::new(MemDocs::default()));
    seed(
        &store,
        "studio-finish",
        vec![ok_ack(
            serde_json::to_value(StudioFinishResult {
                studio: "acme-shop".into(),
                seq: 9,
                reason: "done".into(),
            })
            .unwrap(),
        )],
    );
    seed(
        &store,
        "studio-prune",
        vec![
            plan_ack(prune_report(true), "tok-prune"),
            ok_ack(prune_report(false)),
        ],
    );
    seed(
        &store,
        "studio-delete",
        vec![
            plan_ack(delete_report(true, false), "tok-delete"),
            ok_ack(delete_report(false, true)),
        ],
    );
    let mut app = server_app(store.clone());
    let studio = StudioKey::local("acme-shop");

    app.run_action(studio_cmds::finish(&studio, Some("done")));
    let reply = app.action_rx.recv().await.unwrap();
    app.on_action_reply(reply);
    assert!(
        app.action_toast.as_ref().is_some_and(|t| !t.fail),
        "finish succeeded"
    );
    assert!(matches!(
        queued(&store).last().unwrap().0,
        ReqAction::StudioFinish { .. }
    ));

    app.prune_open(&studio);
    drain_dialog_fetch(&mut app).await;
    let Some(StudioDialog::Confirm(d)) = &app.studio_dialog else {
        panic!("the prune plan");
    };
    assert!(d.body.contains("would remove worktree"), "{}", d.body);
    assert!(matches!(
        &d.for_,
        ConfirmFor::Prune { token: Some(t), .. } if t == "tok-prune"
    ));
    key(&mut app, Key::Enter);
    let reply = app.action_rx.recv().await.unwrap();
    app.on_action_reply(reply);
    assert!(app.action_toast.as_ref().is_some_and(|t| !t.fail));
    let (last, token) = queued(&store).pop().unwrap();
    assert!(matches!(
        last,
        ReqAction::StudioPrune { dry_run: false, .. }
    ));
    assert_eq!(token.as_deref(), Some("tok-prune"));

    app.delete_studio_open(&studio);
    drain_dialog_fetch(&mut app).await;
    let Some(StudioDialog::Confirm(d)) = &app.studio_dialog else {
        panic!("the delete plan");
    };
    assert!(d.body.contains("Archives the ledger"), "{}", d.body);
    assert!(matches!(
        &d.for_,
        ConfirmFor::DeleteStudio { token: Some(t), .. } if t == "tok-delete"
    ));
    key(&mut app, Key::Enter);
    let reply = app.action_rx.recv().await.unwrap();
    app.on_action_reply(reply);
    let toast = app.action_toast.as_ref().expect("delete toast");
    assert!(!toast.fail, "{}", toast.body);
    assert!(
        toast.title.contains("deleted studio acme-shop"),
        "{}",
        toast.title
    );
    let (last, token) = queued(&store).pop().unwrap();
    assert!(matches!(
        last,
        ReqAction::StudioDelete { dry_run: false, .. }
    ));
    assert_eq!(token.as_deref(), Some("tok-delete"));
}

#[tokio::test]
async fn restore_reviews_and_commits_with_the_plan_token() {
    let store = Arc::new(Mutex::new(MemDocs::default()));
    seed(
        &store,
        "studio-restore",
        vec![
            plan_ack(restore_report(true), "tok-restore"),
            ok_ack(restore_report(false)),
        ],
    );
    let mut app = server_app(store.clone());
    app.studio_index
        .note_archived_reply(None, Instant::now(), Ok(ARCHIVED_JSON));
    app.open_archived(None);

    key(&mut app, Key::Enter); // list → name
    key(&mut app, Key::Enter); // name → dry run
    drain_dialog_fetch(&mut app).await;
    let Some(StudioDialog::Restore(d)) = &app.studio_dialog else {
        panic!("the restore dialog");
    };
    assert!(d.body.contains("would restore"), "{}", d.body);
    assert_eq!(d.confirm_token.as_deref(), Some("tok-restore"));

    key(&mut app, Key::Enter); // the real copy
    let reply = app.action_rx.recv().await.unwrap();
    app.on_action_reply(reply);
    assert!(app.action_toast.as_ref().is_some_and(|t| !t.fail));
    let (last, token) = queued(&store).pop().unwrap();
    assert!(matches!(
        last,
        ReqAction::StudioRestore { dry_run: false, .. }
    ));
    assert_eq!(token.as_deref(), Some("tok-restore"));
}
