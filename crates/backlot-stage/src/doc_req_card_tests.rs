//! Queued card approvals travel over the server document queue: a
//! `CardApply` / `CardCancel` request from a client applies and cancels the
//! card on the server, with the same ledgers the CLI writes.

use std::path::Path;

use backlot_model::EventBy;
use backlot_model::hollywood::{CardState, HollywoodKind};
use backlot_model::req_results::{CardApplyResult, CardCancelResult};
use backlot_model::req_wire::{
    REQ_DOC_NAME, REQ_SCHEMA, RES_DOC_NAME, ReqAction, ReqEnvelope, ResDoc,
};
use serde_json::json;

use crate::daemon::Stage;
use crate::doc_req::Consumer;
use crate::doc_req_fake_core::FakeCore;
use crate::doc_req_verbs_seed::{env_for, make_fake_runner, seed_full_world};
use crate::store;
use crate::test_support::TestDir;

#[tokio::test]
async fn queued_card_apply_and_cancel_reach_the_server_ledgers() {
    let dir = TestDir::new("blcard-queue");
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
    assert!(apply_ack.ok, "{apply_ack:?}");
    let apply_res: CardApplyResult =
        serde_json::from_value(apply_ack.result.clone().unwrap()).unwrap();
    assert_eq!(apply_res.id, format!("c{recast_card_seq}"));
    assert!(apply_res.stale.is_none());

    let cancel_ack = res_doc
        .acks
        .iter()
        .find(|a| a.req_id == "req-card-cancel-1")
        .unwrap();
    assert!(cancel_ack.ok, "{cancel_ack:?}");
    let cancel_res: CardCancelResult =
        serde_json::from_value(cancel_ack.result.clone().unwrap()).unwrap();
    assert_eq!(cancel_res.reason, "hold off");

    let hstate = store::load_hstate(&store::hollywood_dir(base)).unwrap();
    assert_eq!(hstate.cards[&recast_card_seq].state, CardState::Applied);
    assert_eq!(hstate.cards[&setup_card_seq].state, CardState::Cancelled);

    let (h_after, _) = store::load_hevents(&store::hollywood_dir(base)).unwrap();
    assert_eq!(h_after.len(), h_before.len() + 2);
    let [apply_ev, cancel_ev] = &h_after[h_before.len()..] else {
        panic!("expected two card events, got {h_after:?}");
    };
    assert_eq!(apply_ev.by, EventBy::doc("user"));
    assert!(matches!(
        apply_ev.kind,
        HollywoodKind::CardApplied { card } if card == recast_card_seq
    ));
    assert_eq!(cancel_ev.by, EventBy::doc("user"));
    assert!(matches!(
        cancel_ev.kind,
        HollywoodKind::CardCancelled { card, .. } if card == setup_card_seq
    ));
}
