//! Hole 3 tests: `BACKLOT_TERMINAL` stamping in launcher envs and CLI
//! ancestor-process-tree gating even when `BACKLOT_ROLE` is unset.

use std::collections::BTreeMap;

use backlot_model::event::ActorIdentity;

use crate::act::role_env;
use crate::args::Common;
use crate::cli::Env;
use crate::hlife::hrole_env;
use crate::proc_guard::{CliVerb, check_cli_gate_with};
use crate::reconcile::{RoleKind, Start};

fn dummy_start(role: &str) -> Start {
    Start {
        role: role.into(),
        kind: RoleKind::Actor,
        task: Some("t1".into()),
        terminal: "bl-acme-shop-t1".into(),
        actor: ActorIdentity {
            runner: "fake".into(),
            runner_version: None,
            model: "m".into(),
            effort: None,
            permission_profile: "standard".into(),
        },
        repo: "/home/user/acme-shop".into(),
        members: Vec::new(),
        no_git: false,
        resume: None,
    }
}

#[test]
fn role_env_and_hrole_env_stamp_backlot_terminal() {
    let r_env = role_env(
        "acme-shop",
        &dummy_start("actor-t1-c1"),
        "/state",
        "/exe",
        "/usr/bin",
        BTreeMap::new(),
        BTreeMap::new(),
    );
    assert_eq!(r_env["BACKLOT_ROLE"], "actor-t1-c1");
    assert_eq!(r_env["BACKLOT_TERMINAL"], "bl-acme-shop-actor-t1-c1");

    let h_env = hrole_env(
        BTreeMap::new(),
        BTreeMap::new(),
        "/state",
        "/exe",
        "/usr/bin",
    );
    assert_eq!(h_env["BACKLOT_ROLE"], "hollywood");
    assert_eq!(h_env["BACKLOT_TERMINAL"], "bl-hollywood");
}

#[test]
fn cli_ancestor_tree_blocks_unsetting_backlot_role_and_bad_approvals() {
    let reader = |pid: u32| -> Option<(u32, Vec<(String, String)>)> {
        match pid {
            300 => Some((200, vec![])), // simulated `env -u BACKLOT_ROLE` child
            200 => Some((
                100,
                vec![
                    ("BACKLOT_STUDIO".into(), "acme-shop".into()),
                    ("BACKLOT_ROLE".into(), "actor-t1-c1".into()),
                    ("BACKLOT_TERMINAL".into(), "bl-acme-shop-actor-t1-c1".into()),
                ],
            )),
            400 => Some((
                100,
                vec![
                    ("BACKLOT_STUDIO".into(), "acme-shop".into()),
                    ("BACKLOT_ROLE".into(), "director".into()),
                    ("BACKLOT_TERMINAL".into(), "bl-acme-shop-director".into()),
                ],
            )),
            500 => Some((
                100,
                vec![("BACKLOT_TERMINAL".into(), "bl-hollywood".into())],
            )),
            100 => Some((1, vec![("HOME".into(), "/home/user".into())])),
            _ => None,
        }
    };

    let clean_env = Env::default(); // BACKLOT_ROLE intentionally unset
    let common = Common::default();

    // Actor ancestor blocks `card apply|cancel`, `perms revoke`, and `approve` even when BACKLOT_ROLE is unset.
    assert!(check_cli_gate_with(CliVerb::CardMutate, &common, &clean_env, &reader, 300).is_err());
    assert!(check_cli_gate_with(CliVerb::PermsRevoke, &common, &clean_env, &reader, 300).is_err());
    assert!(
        check_cli_gate_with(
            CliVerb::Approve {
                person: "actor-t1-c1"
            },
            &common,
            &clean_env,
            &reader,
            300
        )
        .is_err()
    );

    // Spoofing BACKLOT_ROLE=director from an actor ancestor is refused.
    let mut spoof_env = Env::default();
    spoof_env
        .vars
        .insert("BACKLOT_ROLE".into(), "director".into());
    assert!(
        check_cli_gate_with(
            CliVerb::Approve {
                person: "actor-t1-c1"
            },
            &common,
            &spoof_env,
            &reader,
            300
        )
        .is_err()
    );

    // Director ancestor: unset BACKLOT_ROLE fails, `--as user` fails, self-approval fails.
    assert!(
        check_cli_gate_with(
            CliVerb::Approve {
                person: "actor-t1-c1"
            },
            &common,
            &clean_env,
            &reader,
            400
        )
        .is_err()
    );
    let mut dir_env = Env::default();
    dir_env
        .vars
        .insert("BACKLOT_ROLE".into(), "director".into());
    let bad_as = Common {
        role: Some("user".into()),
        ..Common::default()
    };
    assert!(
        check_cli_gate_with(
            CliVerb::Approve {
                person: "actor-t1-c1"
            },
            &bad_as,
            &dir_env,
            &reader,
            400
        )
        .is_err()
    );
    assert!(
        check_cli_gate_with(
            CliVerb::Approve { person: "director" },
            &common,
            &dir_env,
            &reader,
            400
        )
        .is_err()
    );
    // Director approving an actor with matching BACKLOT_ROLE succeeds.
    assert!(
        check_cli_gate_with(
            CliVerb::Approve {
                person: "actor-t1-c1"
            },
            &common,
            &dir_env,
            &reader,
            400
        )
        .is_ok()
    );

    // Hollywood terminal-only ancestor blocks card mutate, perms revoke, and approve.
    assert!(check_cli_gate_with(CliVerb::CardMutate, &common, &clean_env, &reader, 500).is_err());
    assert!(
        check_cli_gate_with(
            CliVerb::Approve {
                person: "hollywood"
            },
            &common,
            &clean_env,
            &reader,
            500
        )
        .is_err()
    );

    // Outside any role ancestor (ppid 100), card apply, perms revoke, and approve succeed.
    assert!(check_cli_gate_with(CliVerb::CardMutate, &common, &clean_env, &reader, 100).is_ok());
    assert!(check_cli_gate_with(CliVerb::PermsRevoke, &common, &clean_env, &reader, 100).is_ok());
    assert!(
        check_cli_gate_with(
            CliVerb::Approve {
                person: "actor-t1-c1"
            },
            &common,
            &clean_env,
            &reader,
            100
        )
        .is_ok()
    );
}

#[test]
fn server_control_is_refused_from_a_role_terminal_and_allowed_for_the_person() {
    let reader = |pid: u32| -> Option<(u32, Vec<(String, String)>)> {
        match pid {
            300 => Some((200, vec![])), // simulated `env -u BACKLOT_ROLE` child
            200 => Some((
                100,
                vec![
                    ("BACKLOT_STUDIO".into(), "acme-shop".into()),
                    ("BACKLOT_ROLE".into(), "producer".into()),
                    ("BACKLOT_TERMINAL".into(), "bl-acme-shop-producer".into()),
                ],
            )),
            100 => Some((1, vec![("HOME".into(), "/home/user".into())])),
            _ => None,
        }
    };
    let clean_env = Env::default();
    let common = Common::default();

    // A role ancestor blocks `backlot server …` even with BACKLOT_ROLE unset.
    let err =
        check_cli_gate_with(CliVerb::ServerControl, &common, &clean_env, &reader, 300).unwrap_err();
    assert!(err.message().contains("role terminal ancestor"), "{err:?}");

    // BACKLOT_ROLE alone is enough.
    let mut role_env = Env::default();
    role_env
        .vars
        .insert("BACKLOT_ROLE".into(), "producer".into());
    let err =
        check_cli_gate_with(CliVerb::ServerControl, &common, &role_env, &reader, 100).unwrap_err();
    assert!(err.message().contains("BACKLOT_ROLE is set"), "{err:?}");

    // The person's own shell (no role env, no role ancestry) is unaffected.
    assert!(check_cli_gate_with(CliVerb::ServerControl, &common, &clean_env, &reader, 100).is_ok());

    // Hollywood control (switch / setup) is blocked under a role ancestor or BACKLOT_ROLE.
    let err = check_cli_gate_with(CliVerb::HollywoodControl, &common, &clean_env, &reader, 300)
        .unwrap_err();
    assert!(err.message().contains("role terminal ancestor"), "{err:?}");
    let err = check_cli_gate_with(CliVerb::HollywoodControl, &common, &role_env, &reader, 100)
        .unwrap_err();
    assert!(err.message().contains("BACKLOT_ROLE is set"), "{err:?}");
    assert!(
        check_cli_gate_with(CliVerb::HollywoodControl, &common, &clean_env, &reader, 100).is_ok()
    );
    assert!(
        crate::proc_guard::check_segment_self_approval(
            &[
                "backlot".into(),
                "hollywood".into(),
                "switch".into(),
                "--runner".into(),
                "fake".into(),
                "--model".into(),
                "m".into(),
            ],
            Some("hollywood"),
        )
        .is_some()
    );
    assert!(
        crate::proc_guard::check_segment_self_approval(
            &["backlot".into(), "hollywood".into(), "setup".into()],
            Some("hollywood"),
        )
        .is_some()
    );
}
