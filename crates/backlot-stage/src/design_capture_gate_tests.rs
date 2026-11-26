//! PR 5 (`design-capture`) fallback hard-gate & `design_gate = off` tests.

use std::fs;

use backlot_model::NewEvent;
use backlot_model::design::DesignPhase;
use backlot_model::event::{EventBy, EventKind};
use hc_proto::terminal::Status;
use serde_json::json;

use crate::cli_testkit::fixture;
use crate::design_capture::pending_note_path;
use crate::design_capture_tests::{
    emit_signal, fake_stage, role_env, run_hook_json, run_ok, run_refused, runtime, sample_prod,
    setup_studio, term_for,
};
use crate::reasons;
use crate::signals::signals_path;
use crate::store::{self, Pending};

#[test]
fn fallback_hard_gate_refuses_design_next_until_note_and_skips_tainted_turns() {
    let rt = runtime();
    // 1. User config `[design] hook_capture = false` on claude triggers hard gate even after an earlier note.
    let (scratch, env, dir) = setup_studio("required", true, "claude");
    let xdg_cfg = scratch.dir.join("xdg").join("backlot").join("config.toml");
    fs::create_dir_all(xdg_cfg.parent().unwrap()).unwrap();
    fs::write(&xdg_cfg, "[design]\nhook_capture = false\n").unwrap();
    let (st, fake) = rt.block_on(fake_stage(&scratch.dir, env.vars.clone()));
    let mut tm = term_for(Some("waiting"));
    fake.lock().unwrap().screen = "> \n".into();
    let t0 = 1_700_000_000_000u64;

    let prior = fixture(&scratch, "prior.md", "Prior question.\n");
    run_ok(
        &env,
        &[
            "design", "note", "--as", "producer", "--from", "user", "--file", &prior,
        ],
    );

    emit_signal(&dir, t0, "Stop", "s-prod");
    rt.block_on(sample_prod(&st, &dir, &tm, "claude", t0 + 50));
    st.taint_ask("acme-shop", &store::load_state(&dir).unwrap(), "producer");
    emit_signal(&dir, t0 + 500, "UserPromptSubmit", "s-prod");
    tm.status = Some(Status {
        state: "working".into(),
        ..Default::default()
    });
    rt.block_on(sample_prod(&st, &dir, &tm, "claude", t0 + 1_100));
    emit_signal(&dir, t0 + 1_500, "Stop", "s-prod");
    tm.status = Some(Status {
        state: "waiting".into(),
        ..Default::default()
    });
    rt.block_on(sample_prod(&st, &dir, &tm, "claude", t0 + 2_200));
    assert!(!pending_note_path(&dir).exists());

    emit_signal(&dir, t0 + 2_500, "UserPromptSubmit", "s-prod");
    rt.block_on(sample_prod(&st, &dir, &tm, "claude", t0 + 3_300));
    let err = run_refused(&env, &["design", "next", "--as", "producer"]);
    assert!(err.contains("design note"), "{err}");

    let note = fixture(&scratch, "note.md", "Follow-up terminal answer.\n");
    run_ok(
        &env,
        &[
            "design", "note", "--as", "producer", "--from", "user", "--file", &note,
        ],
    );
    run_ok(&env, &["design", "next", "--as", "producer"]);
    assert_eq!(store::load_state(&dir).unwrap().design.unwrap().round, 2);

    // 2. Non-hook runner (`codex`) and `HooksUnavailable` refuse on Waiting -> Working.
    for with_unavailable in [false, true] {
        let runner = if with_unavailable { "claude" } else { "codex" };
        let (scratch, env, dir) = setup_studio("required", true, runner);
        let (st, fake) = rt.block_on(fake_stage(&scratch.dir, env.vars.clone()));
        if with_unavailable {
            store::append(&dir, |_, _| {
                Ok::<_, std::convert::Infallible>(Pending {
                    event: NewEvent::new(
                        EventBy::role("stage"),
                        EventKind::AttentionRaised {
                            role: "producer".into(),
                            reason: reasons::HOOKS_UNAVAILABLE.into(),
                            detail: "missing".into(),
                            needs_user: true,
                        },
                    ),
                    files: Vec::new(),
                })
            })
            .unwrap();
        }
        fake.lock().unwrap().screen = if with_unavailable { "> \n" } else { "? \n" }.into();
        let mut tm = term_for(Some(if with_unavailable { "waiting" } else { "done" }));
        rt.block_on(sample_prod(&st, &dir, &tm, runner, t0));
        fake.lock().unwrap().screen = "• Working (1s • esc to interrupt)\n".into();
        tm.status = Some(Status {
            state: "working".into(),
            ..Default::default()
        });
        rt.block_on(sample_prod(&st, &dir, &tm, runner, t0 + 1_100));
        let err = run_refused(&env, &["design", "next", "--as", "producer"]);
        assert!(err.contains("design note"), "{runner}: {err}");
    }
}

#[test]
fn design_gate_off_leaves_capture_and_design_next_untouched() {
    let rt = runtime();
    let (scratch, env, dir) = setup_studio("off", false, "claude");
    let (st, fake) = rt.block_on(fake_stage(&scratch.dir, env.vars.clone()));
    let prod_env = role_env(&env, "producer");
    let sig = signals_path(&dir, "producer");
    let tm = term_for(Some("waiting"));
    fake.lock().unwrap().screen = "> \n".into();

    run_hook_json(
        None,
        &sig,
        &prod_env,
        &scratch.files,
        json!({"session_id": "s-prod", "hook_event_name": "UserPromptSubmit", "prompt": "off prompt"}),
    );
    rt.block_on(sample_prod(&st, &dir, &tm, "claude", 1_700_000_000_000));

    assert!(!dir.join("design").join(".hook-capture.jsonl").exists());
    assert!(!pending_note_path(&dir).exists());
    let state = store::load_state(&dir).unwrap();
    assert_eq!(state.design_phase(), DesignPhase::Idle);
    assert!(state.design.is_none());
}
