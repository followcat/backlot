//! Tests for the pure pieces of `act`: the role-terminal
//! environment and the attention a refused launch owes.

use std::collections::BTreeMap;

use backlot_model::event::ActorIdentity;
use backlot_model::state::{Attention, StudioState};

use crate::act::{attention_open, labels, role_env, start_event};
use crate::reconcile::{RoleKind, Start};
use crate::runners::{Launch, Session};

fn start() -> Start {
    Start {
        role: "producer".into(),
        kind: RoleKind::Producer,
        task: None,
        terminal: "bl-acme-producer".into(),
        actor: ActorIdentity {
            runner: "devin".into(),
            runner_version: None,
            model: "m".into(),
            effort: "high".into(),
            permission_profile: "standard".into(),
        },
        repo: "/home/user/acme-shop".into(),
        resume: None,
    }
}

/// `runner_env` values reach the terminal env — and nothing
/// else: not the labels, which are all the ledger and summary
/// could ever see.
#[test]
fn runner_env_reaches_env_and_nothing_else() {
    let start = start();
    let runner_env = BTreeMap::from([("HTTPS_PROXY".into(), "http://proxy:8080".into())]);
    let env = role_env(
        "acme",
        &start,
        "/state",
        "/exe",
        "/bin",
        BTreeMap::from([("ADAPTER".into(), "1".into())]),
        runner_env,
    );
    assert_eq!(env["HTTPS_PROXY"], "http://proxy:8080");
    assert_eq!(env["BACKLOT_ROLE"], "producer");
    // Structural variables still win over the runner's map.
    let runner_env = BTreeMap::from([("BACKLOT_ROLE".into(), "evil".into())]);
    let env = role_env(
        "acme",
        &start,
        "/state",
        "/exe",
        "/bin",
        BTreeMap::new(),
        runner_env,
    );
    assert_eq!(env["BACKLOT_ROLE"], "producer");
    let labels = serde_json::to_string(&labels("acme", &start)).unwrap();
    assert!(!labels.contains("proxy"), "{labels}");
}

/// `PATH` is the one structural variable a config may bend: a
/// configured value leads, the exe's own directory stays first;
/// no configured PATH keeps the stage's usual one.
#[test]
fn runner_env_path_leads_but_keeps_the_exe_dir() {
    let start = start();
    let runner_env = BTreeMap::from([("PATH".into(), "/opt/nvm/bin".into())]);
    let env = role_env(
        "acme",
        &start,
        "/state",
        "/exe",
        "/exe:/usr/bin",
        BTreeMap::new(),
        runner_env,
    );
    assert_eq!(env["PATH"], "/exe:/opt/nvm/bin");
    let env = role_env(
        "acme",
        &start,
        "/state",
        "/exe",
        "/exe:/usr/bin",
        BTreeMap::new(),
        BTreeMap::new(),
    );
    assert_eq!(env["PATH"], "/exe:/usr/bin");
}

/// `launch refused` and every other raiser share the one open
/// attention per role and reason: a second identical raise is
/// suppressed, a different role or reason is not, and a cleared
/// attention never blocks the next raise.
#[test]
fn attention_is_deduped_per_role_and_reason() {
    let mut state = StudioState::default();
    state.attention.push(Attention {
        seq: 1,
        role: "producer".into(),
        reason: "launch refused".into(),
        detail: "terminal.run: taken: bl-acme-producer".into(),
        open: true,
    });
    assert!(attention_open(&state, "producer", "launch refused"));
    assert!(!attention_open(&state, "director", "launch refused"));
    assert!(!attention_open(&state, "producer", "unknown runner"));
    state.attention[0].open = false;
    assert!(!attention_open(&state, "producer", "launch refused"));
}

/// A resume launch appends `role.resumed` with the requested session
/// — post-launch runners report no id on a resume, so `launch` has
/// none to give. A fresh start records `role.started` with whatever
/// the adapter chose.
#[test]
fn a_resume_launch_records_role_resumed_with_the_asked_session() {
    let mut held = start();
    held.resume = Some("sess-1".into());
    let launch = Launch {
        argv: vec!["runner".into()],
        env: BTreeMap::new(),
        // Codex and devin stay PostLaunch even on a resume.
        session: Session::PostLaunch,
    };
    match start_event(&held, &launch, "t7") {
        backlot_model::EventKind::RoleResumed {
            role,
            terminal,
            actor,
            session_id,
            reason,
        } => {
            assert_eq!(role, "producer");
            assert_eq!(terminal.as_deref(), Some("t7"));
            assert!(actor.is_some());
            assert_eq!(session_id.as_deref(), Some("sess-1"));
            assert_eq!(reason.as_deref(), Some("reopened"));
        }
        other => panic!("a resume must record role.resumed, got {other:?}"),
    }

    let fresh = start();
    let launch = Launch {
        argv: vec!["runner".into()],
        env: BTreeMap::new(),
        session: Session::Chosen("chosen-1".into()),
    };
    match start_event(&fresh, &launch, "t7") {
        backlot_model::EventKind::RoleStarted {
            session_id, reason, ..
        } => {
            assert_eq!(session_id.as_deref(), Some("chosen-1"));
            assert_eq!(reason, None);
        }
        other => panic!("a fresh start must record role.started, got {other:?}"),
    }
}
