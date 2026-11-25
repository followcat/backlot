//! Fail-first and regression tests verifying that `CardApply` and
//! `CardCancel` arriving via `plugin.backlot.req` are refused before
//! dispatch without modifying ledgers, while other verbs still execute.

use std::path::Path;
use std::time::Duration;

use backlot_model::EventBy;
use backlot_model::event::EventKind;
use backlot_model::hollywood::CardState;
use backlot_model::req_results::TellResult;
use backlot_model::req_wire::{
    REQ_DOC_NAME, REQ_SCHEMA, RES_DOC_NAME, ReqAction, ReqEnvelope, ResDoc,
};
use serde_json::json;

use crate::daemon::Stage;
use crate::doc_client;
use crate::doc_req::Consumer;
use crate::doc_req_fake_core::FakeCore;
use crate::doc_req_verbs_seed::{env_for, make_fake_runner, seed_full_world};
use crate::store;
use crate::test_support::TestDir;

const EXPECTED_REFUSAL: &str =
    "card approvals are not taken over the server queue; apply it on the server host";

#[tokio::test]
async fn queue_refuses_card_apply_and_cancel_before_dispatch_while_tell_still_works() {
    let dir = TestDir::new("blcard-refuse");
    let fake_bin = make_fake_runner(&dir);
    let env = env_for(&dir, &fake_bin);
    let base = Path::new(env.get("BACKLOT_STATE_DIR").unwrap());
    let (_stalled_seq, _tell_seq, recast_card_seq, _new_card_seq) = seed_full_world(base, &env);
    let setup_card_seq = recast_card_seq + 1;

    let core = FakeCore::spawn(dir.join("hc.sock")).await;
    let mut stage = Stage::for_test(core.client().await);
    stage.base = base.to_path_buf();
    stage.env_vars = env.vars.clone();

    let mut consumer = Consumer::load(&stage.base, crate::live::now_ms());
    let (h_before, _) = store::load_hevents(&store::hollywood_dir(base)).unwrap();
    let (s_before, _) = store::load_events(&store::studio_dir(base, "acme-shop")).unwrap();

    let now_ms = crate::live::now_ms();
    let apply_req = ReqEnvelope::new(
        "req-card-apply-1",
        now_ms,
        &ReqAction::CardApply {
            id: recast_card_seq,
        },
        None,
    )
    .unwrap();
    let cancel_req = ReqEnvelope::new(
        "req-card-cancel-1",
        now_ms,
        &ReqAction::CardCancel {
            id: setup_card_seq,
            reason: "hold off".into(),
        },
        None,
    )
    .unwrap();

    core.set_raw_doc(
        REQ_DOC_NAME,
        json!({ "schema": REQ_SCHEMA, "queue": [apply_req, cancel_req] }),
    );
    assert!(consumer.process(&stage, now_ms).await.unwrap());

    let (_, res_val) = core.doc(RES_DOC_NAME).unwrap();
    let res_doc: ResDoc = serde_json::from_value(res_val).unwrap();
    let apply_ack = res_doc
        .acks
        .iter()
        .find(|a| a.req_id == "req-card-apply-1")
        .unwrap();
    assert!(!apply_ack.ok);
    assert_eq!(apply_ack.error.as_deref(), Some(EXPECTED_REFUSAL));

    let cancel_ack = res_doc
        .acks
        .iter()
        .find(|a| a.req_id == "req-card-cancel-1")
        .unwrap();
    assert!(!cancel_ack.ok);
    assert_eq!(cancel_ack.error.as_deref(), Some(EXPECTED_REFUSAL));

    let (h_after, _) = store::load_hevents(&store::hollywood_dir(base)).unwrap();
    let (s_after, _) = store::load_events(&store::studio_dir(base, "acme-shop")).unwrap();
    assert_eq!(h_after.len(), h_before.len());
    assert_eq!(s_after.len(), s_before.len());

    let hstate = store::load_hstate(&store::hollywood_dir(base)).unwrap();
    assert_eq!(hstate.cards[&recast_card_seq].state, CardState::Open);
    assert_eq!(hstate.cards[&setup_card_seq].state, CardState::Open);

    // Non-approval queue verbs such as `tell` continue executing as before.
    let client = core.client().await;
    let ep = core.endpoint();
    let tell_fut = tokio::spawn(async move {
        doc_client::send_typed::<TellResult>(
            &client,
            Some(&ep),
            &ReqAction::Tell {
                studio: "acme-shop".into(),
                role: "producer".into(),
                text: "still queued".into(),
            },
            None,
            Duration::from_secs(2),
        )
        .await
    });
    loop {
        if tell_fut.is_finished() {
            let res = tell_fut.await.unwrap().unwrap();
            assert_eq!(res.role, "producer");
            break;
        }
        let _ = consumer
            .process(&stage, crate::live::now_ms())
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    let (s_tell, _) = store::load_events(&store::studio_dir(base, "acme-shop")).unwrap();
    assert_eq!(s_tell.len(), s_before.len() + 1);
    assert_eq!(s_tell.last().unwrap().by, EventBy::doc("user"));
    assert!(matches!(
        s_tell.last().unwrap().kind,
        EventKind::MessageQueued { ref to, .. } if to == "producer"
    ));
}
