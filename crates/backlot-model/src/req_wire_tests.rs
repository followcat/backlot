use serde_json::json;

use crate::attention::is_prompt_reason;
use crate::event::{ActorIdentity, EventBy, Priority};
use crate::req_wire::*;

#[test]
fn document_names_match_hc_rule() {
    assert!(is_valid_doc_name(REQ_DOC_NAME));
    assert!(is_valid_doc_name(RES_DOC_NAME));
    assert!(is_valid_doc_name(CAT_DOC_NAME));
    assert!(is_valid_doc_name("_a.0-z"));
    assert!(!is_valid_doc_name(""));
    assert!(!is_valid_doc_name(".leading-dot"));
    assert!(!is_valid_doc_name("-leading-dash"));
    assert!(!is_valid_doc_name("Upper"));
    assert!(!is_valid_doc_name("a/b"));
    assert!(!is_valid_doc_name(&"a".repeat(33)));
    assert!(is_valid_doc_name(&"a".repeat(32)));
}

#[test]
fn additive_fields_ignored_on_req_res_cat_and_event_by() {
    let req: ReqDoc = serde_json::from_value(json!({
        "schema": REQ_SCHEMA,
        "future_field": true,
        "queue": [{
            "req_id": "r1",
            "ts_ms": 1_000,
            "extra_env_field": "kept-compatible",
            "action": {
                "kind": "tell",
                "studio": "acme-shop",
                "role": "producer",
                "text": "check status",
                "unknown_nested": 42
            }
        }]
    }))
    .unwrap();
    assert_eq!(req.queue.len(), 1);
    let env: ReqEnvelope = serde_json::from_value(req.queue[0].clone()).unwrap();
    assert_eq!(
        env.parse_action().unwrap(),
        ReqAction::Tell {
            studio: "acme-shop".into(),
            role: "producer".into(),
            text: "check status".into(),
        }
    );

    let res: ResDoc = serde_json::from_value(json!({
        "schema": RES_SCHEMA,
        "extra": 1,
        "acks": [{
            "req_id": "r1",
            "ts_ms": 1_001,
            "ok": true,
            "result": {"seq": 1},
            "future": "ok"
        }]
    }))
    .unwrap();
    assert_eq!(res.acks.len(), 1);
    assert!(res.acks[0].ok);

    let by = EventBy::doc("user");
    let raw = serde_json::to_value(&by).unwrap();
    assert_eq!(raw, json!({"role": "user", "via": "doc"}));
    let decoded: EventBy = serde_json::from_value(raw).unwrap();
    assert_eq!(decoded, by);
    let legacy: EventBy = serde_json::from_value(json!({"role": "user"})).unwrap();
    assert_eq!(legacy, EventBy::role("user"));
}

#[test]
fn unknown_and_refused_action_tags_fail_cleanly() {
    for tag in [
        "approve",
        "permission",
        "dialog",
        "answer",
        "prompt",
        "prompt-answer",
        "role-approval",
    ] {
        let err = parse_action_value(&json!({
            "kind": tag,
            "studio": "acme-shop",
            "person": "director",
            "decision": "allow"
        }))
        .unwrap_err();
        assert!(
            err.contains("agent terminal"),
            "tag {tag} error should direct to terminal: {err}"
        );
    }

    for tag in [
        "exec",
        "shell",
        "run",
        "terminal",
        "terminal-send",
        "outward",
    ] {
        let err = parse_action_value(&json!({"kind": tag, "cmd": "ls"})).unwrap_err();
        assert!(err.contains("refused"), "{tag}: {err}");
    }

    let unknown = parse_action_value(&json!({"kind": "beam-me-up"})).unwrap_err();
    assert!(unknown.contains("unsupported"), "{unknown}");
}

#[test]
fn timestamp_expiry_and_future_skew_boundaries() {
    let now = 1_000_000;
    assert!(validate_timestamp(now - REQ_TTL_MS, now).is_ok());
    assert_eq!(
        validate_timestamp(now - REQ_TTL_MS - 1, now),
        Err("request expired")
    );
    assert!(validate_timestamp(now + REQ_FUTURE_SKEW_MS, now).is_ok());
    assert_eq!(
        validate_timestamp(now + REQ_FUTURE_SKEW_MS + 1, now),
        Err("request timestamp too far in the future")
    );
}

#[test]
fn res_ack_enforces_max_result_bytes() {
    let ok = ResAck::ok_value("r1", 10, json!({"x": "ok"}));
    assert!(ok.ok);
    assert!(ok.result.is_some());

    let big = ResAck::ok_value("r2", 10, json!({"blob": "a".repeat(MAX_RESULT_BYTES)}));
    assert!(!big.ok);
    assert!(big.result.is_none());
    assert!(big.error.unwrap().contains("65536 byte limit"));
}

#[test]
fn card_ids_accept_c_prefix_or_numeric_and_prompt_reasons_refused() {
    let c: ReqAction = serde_json::from_value(json!({"kind": "card-show", "id": "c42"})).unwrap();
    assert_eq!(c, ReqAction::CardShow { id: 42 });
    let n: ReqAction = serde_json::from_value(json!({"kind": "card-apply", "id": 7})).unwrap();
    assert_eq!(n, ReqAction::CardApply { id: 7 });

    assert!(is_prompt_reason("permission prompt"));
    assert!(is_prompt_reason("question"));
    assert!(is_prompt_reason("folder-trust"));
    assert!(is_prompt_reason("folder trust refused"));
    assert!(!is_prompt_reason("stalled"));
    assert!(!is_prompt_reason("cannot resume"));
    assert!(!is_prompt_reason("owed"));

    let prio: ReqAction = serde_json::from_value(json!({
        "kind": "task-priority",
        "studio": "acme-shop",
        "task": "t1",
        "level": "high"
    }))
    .unwrap();
    assert_eq!(
        prio,
        ReqAction::TaskPriority {
            studio: "acme-shop".into(),
            task: "t1".into(),
            level: Priority::High,
        }
    );

    let cat = CatalogDoc {
        schema: CAT_SCHEMA.into(),
        server_id: "core-1".into(),
        ts_ms: 100,
        runners: Vec::new(),
        hollywood_cfg: HollywoodCfgCatalog::default(),
        default_cast: Some(DefaultCastCatalog {
            mode: None,
            producer: ActorIdentity {
                runner: "fake".into(),
                runner_version: None,
                model: "m".into(),
                effort: None,
                permission_profile: "standard".into(),
            },
            director: ActorIdentity {
                runner: "fake".into(),
                runner_version: None,
                model: "m".into(),
                effort: None,
                permission_profile: "standard".into(),
            },
            writer: ActorIdentity {
                runner: "fake".into(),
                runner_version: None,
                model: "m".into(),
                effort: None,
                permission_profile: "standard".into(),
            },
            candidates: Vec::new(),
            limits: Default::default(),
            max_actors: None,
            rework_cap: None,
            stall_limit_s: None,
            nudge_after_s: None,
            escalate_after_s: None,
            task_budget_s: None,
        }),
        archived: Vec::new(),
        home: "/home/user".into(),
    };
    let same_diff_ts = CatalogDoc {
        ts_ms: 999,
        ..cat.clone()
    };
    assert!(cat.content_eq(&same_diff_ts));

    let sw: ReqAction = serde_json::from_value(json!({
        "kind": "hollywood-switch",
        "runner": "codex",
        "model": "gpt-6-sol",
        "effort": "high",
        "timeout_s": 120
    }))
    .unwrap();
    assert_eq!(
        sw,
        ReqAction::HollywoodSwitch {
            runner: "codex".into(),
            model: "gpt-6-sol".into(),
            effort: Some("high".into()),
            timeout_s: Some(120),
            force: false,
        }
    );
    let cancel: ReqAction =
        serde_json::from_value(json!({"kind": "hollywood-switch-cancel"})).unwrap();
    assert_eq!(cancel, ReqAction::HollywoodSwitchCancel);
    let st: ReqAction = serde_json::from_value(json!({"kind": "hollywood-switch-status"})).unwrap();
    assert_eq!(st, ReqAction::HollywoodSwitchStatus);
}
