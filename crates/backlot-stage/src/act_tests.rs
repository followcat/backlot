//! Tests for the pure pieces of `act`: the role-terminal
//! environment and the attention a refused launch owes.

use std::collections::BTreeMap;

use backlot_model::event::ActorIdentity;
use backlot_model::state::{Attention, StudioState};

use crate::act::{attention_open, labels, role_env, start_event, work_area_cleared_events};
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
            effort: Some("high".into()),
            permission_profile: "standard".into(),
        },
        repo: "/home/user/acme-shop".into(),
        members: Vec::new(),
        no_git: false,
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
    let labels = serde_json::to_string(&labels("acme", &StudioState::default(), &start)).unwrap();
    assert!(!labels.contains("proxy"), "{labels}");
}

/// S9g: `BACKLOT_ROLE` is the ledger key — `actor-<task>-c<n>`, the
/// name the role's own appends record under — never the bare kind
/// name. A fresh cast gets `c1`, a recast `c2`, a resumed or
/// reopened `c1` keeps its key; `BACKLOT_TASK` stays the task.
#[test]
fn an_actor_terminal_gets_its_ledger_key() {
    let env_of = |start: &Start| {
        role_env(
            "acme-shop",
            start,
            "/state",
            "/exe",
            "/bin",
            BTreeMap::new(),
            BTreeMap::new(),
        )
    };
    let actor = |role: &str| Start {
        role: role.into(),
        kind: RoleKind::Actor,
        task: Some("t1".into()),
        terminal: "bl-acme-shop-t1".into(),
        ..start()
    };
    let env = env_of(&actor("actor-t1-c1"));
    assert_eq!(env["BACKLOT_ROLE"], "actor-t1-c1");
    assert_eq!(env["BACKLOT_TASK"], "t1");
    // A recast starts c2 under the same terminal name.
    assert_eq!(env_of(&actor("actor-t1-c2"))["BACKLOT_ROLE"], "actor-t1-c2");
    // A resume — recovered or reopened — keeps the recorded cast's key.
    for reason in ["recovered", "reopened"] {
        let mut held = actor("actor-t1-c1");
        held.resume = Some(crate::reconcile::Resume {
            session: "sess-1".into(),
            reason,
        });
        assert_eq!(
            env_of(&held)["BACKLOT_ROLE"],
            "actor-t1-c1",
            "a {reason} launch must keep the key"
        );
    }
    // A fixed role keeps its name.
    assert_eq!(env_of(&start())["BACKLOT_ROLE"], "producer");
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

/// A PATH the user config sets — where `hollywood setup` writes one —
/// reaches the launch env after the exe's directory, exactly as a
/// per-core `[runner_env]` PATH does.
#[test]
fn a_user_configs_runner_path_reaches_the_launch() {
    let dir = crate::test_support::TestDir::new("backlot-act-user-path");
    let user = "[runner_env.cmd]\nPATH = \"/opt/nvm/bin\"\n";
    let merged =
        crate::config::runner_env_layered_home(dir.path(), "", user, "cmd", dir.path()).unwrap();
    let env = role_env(
        "acme",
        &start(),
        "/state",
        "/exe",
        "/exe:/usr/bin",
        BTreeMap::new(),
        merged,
    );
    assert_eq!(env["PATH"], "/exe:/opt/nvm/bin");
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
        ts_ms: 0,
        needs_user: false,
        cleared_reason: None,
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
    held.resume = Some(crate::reconcile::Resume {
        session: "sess-1".into(),
        reason: "reopened",
    });
    let launch = Launch {
        argv: vec!["runner".into()],
        env: BTreeMap::new(),
        // Codex and devin stay PostLaunch even on a resume.
        session: Session::PostLaunch,
        hooks: None,
        prep: None,
    };
    match start_event(&held, &launch, "t7", None) {
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
        hooks: None,
        prep: None,
    };
    match start_event(&fresh, &launch, "t7", None) {
        backlot_model::EventKind::RoleStarted {
            session_id, reason, ..
        } => {
            assert_eq!(session_id.as_deref(), Some("chosen-1"));
            assert_eq!(reason, None);
        }
        other => panic!("a fresh start must record role.started, got {other:?}"),
    }

    // A fresh restart's start carries its own word.
    match start_event(&fresh, &launch, "t7", Some("restarted")) {
        backlot_model::EventKind::RoleStarted { reason, .. } => {
            assert_eq!(reason.as_deref(), Some("restarted"));
        }
        other => panic!("a fresh restart must record role.started, got {other:?}"),
    }
}

/// When `workareas::ensure` succeeds on a later tick, any open
/// `work area failed` or `not a git repository` attention for that role
/// clears while unrelated roles or reasons stay open.
#[test]
fn work_area_cleared_events_clears_matching_open_attention() {
    let state = StudioState {
        attention: vec![
            Attention {
                seq: 3,
                role: "actor-t1-c1".into(),
                reason: crate::reasons::WORK_AREA_FAILED.into(),
                detail: "already exists".into(),
                open: true,
                ts_ms: 0,
                needs_user: false,
                cleared_reason: None,
            },
            Attention {
                seq: 4,
                role: "actor-t1-c1".into(),
                reason: "launch refused".into(),
                detail: "taken".into(),
                open: true,
                ts_ms: 0,
                needs_user: false,
                cleared_reason: None,
            },
            Attention {
                seq: 5,
                role: "actor-t2-c1".into(),
                reason: crate::reasons::WORK_AREA_FAILED.into(),
                detail: "other role".into(),
                open: true,
                ts_ms: 0,
                needs_user: false,
                cleared_reason: None,
            },
        ],
        ..StudioState::default()
    };
    let events = work_area_cleared_events(&state, "actor-t1-c1");
    assert_eq!(events.len(), 1);
    match &events[0] {
        backlot_model::EventKind::AttentionCleared { target, reason } => {
            assert_eq!(*target, 3);
            assert_eq!(reason.as_deref(), Some("work area ready"));
        }
        other => panic!("unexpected event {other:?}"),
    }
}
