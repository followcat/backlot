//! B1b: a server plan that went stale under the confirm stays in the
//! dialog as its error line, and reviewing again sends a fresh token.

use std::sync::{Arc, Mutex};

use backlot_model::req_results::ERR_STALE_PLAN;
use backlot_model::req_wire::{ReqAction, ResAck};
use serde_json::json;

use super::delete_studio::drain_dialog_fetch;
use super::server_doc_fake::MemDocs;
use super::server_studio_ops::{delete_report, key, ok_ack, plan_ack, queued, seed, server_app};
use crate::studio::StudioKey;
use crate::studio_dialog::{ConfirmFor, StudioDialog};
use hc_vt::Key;

#[tokio::test]
async fn a_stale_delete_plan_reopens_the_dialog_and_re_review_sends_a_fresh_token() {
    let store = Arc::new(Mutex::new(MemDocs::default()));
    seed(
        &store,
        "studio-delete",
        vec![
            plan_ack(delete_report(true, false), "tok-old"),
            ResAck::err("pending", 2, ERR_STALE_PLAN),
            plan_ack(delete_report(true, false), "tok-new"),
            ok_ack(delete_report(false, true)),
        ],
    );
    let mut app = server_app(store.clone());
    let key_ = StudioKey::local("acme-shop");

    app.delete_studio_open(&key_);
    drain_dialog_fetch(&mut app).await;
    key(&mut app, Key::Enter);
    let reply = app.action_rx.recv().await.unwrap();
    app.on_action_reply(reply);

    let Some(StudioDialog::Confirm(d)) = &app.studio_dialog else {
        panic!("the refused dialog stays open");
    };
    assert_eq!(d.error.as_deref(), Some(ERR_STALE_PLAN));
    assert!(
        matches!(&d.for_, ConfirmFor::DeleteStudio { token: None, .. }),
        "the stale token is dropped"
    );

    // `r` reviews the studio again: a fresh plan and token land.
    key(&mut app, Key::Char('r'));
    drain_dialog_fetch(&mut app).await;
    let Some(StudioDialog::Confirm(d)) = &app.studio_dialog else {
        panic!("the reviewed dialog");
    };
    assert!(d.error.is_none(), "{:?}", d.error);
    assert!(matches!(
        &d.for_,
        ConfirmFor::DeleteStudio { token: Some(t), .. } if t == "tok-new"
    ));

    // The second confirm sends the fresh token and lands the delete.
    key(&mut app, Key::Enter);
    let reply = app.action_rx.recv().await.unwrap();
    app.on_action_reply(reply);
    assert!(
        app.action_toast.as_ref().is_some_and(|t| !t.fail),
        "{:?}",
        app.action_toast.as_ref().map(|t| t.body.clone())
    );
    let (last, token) = queued(&store).pop().unwrap();
    assert!(matches!(
        last,
        ReqAction::StudioDelete { dry_run: false, .. }
    ));
    assert_eq!(token.as_deref(), Some("tok-new"));
}

#[tokio::test]
async fn a_stale_prune_plan_reads_as_the_confirm_error() {
    let store = Arc::new(Mutex::new(MemDocs::default()));
    seed(
        &store,
        "studio-prune",
        vec![
            plan_ack(
                json!({ "studios": [{ "studio": "acme-shop", "finished": true,
                                      "dry_run": true,
                                      "steps": [{ "RemovedWorktree": "work/t1" }] }] }),
                "tok-old",
            ),
            ResAck::err("pending", 3, ERR_STALE_PLAN),
        ],
    );
    let mut app = server_app(store.clone());
    let key_ = StudioKey::local("acme-shop");
    app.prune_open(&key_);
    drain_dialog_fetch(&mut app).await;
    key(&mut app, Key::Enter);
    let reply = app.action_rx.recv().await.unwrap();
    app.on_action_reply(reply);
    let Some(StudioDialog::Confirm(d)) = &app.studio_dialog else {
        panic!("the refused prune dialog stays open");
    };
    assert_eq!(d.error.as_deref(), Some(ERR_STALE_PLAN));
    assert!(matches!(&d.for_, ConfirmFor::Prune { token: None, .. }));
}
