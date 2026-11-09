//! Tests for the pure pieces of `act`: the role-terminal
//! environment and the attention a refused launch owes.

use std::collections::BTreeMap;

use backlot_model::event::ActorIdentity;
use backlot_model::state::{Attention, StudioState};

use crate::act::{attention_open, labels, role_env};
use crate::reconcile::{RoleKind, Start};

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
