//! `backlot recast` through the real CLI on a scratch state dir: the
//! user records one, a role terminal cannot, and the usage names the
//! flags.

use crate::cli::Env;
use crate::cli_testkit::*;

fn studio(scratch: &Scratch, env: &Env) {
    let cast = fixture(scratch, "cast.toml", CAST_TOML);
    ok(
        env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
}

/// The user's recast lands `by: user`, keeps the current permission
/// profile, and carries the new identity.
#[test]
fn a_recast_records_the_new_identity_for_the_user() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    ok(
        &env,
        "recast --person director --runner fake --model new-model --effort high \
         --reason quota",
    );
    let json = ok(&env, "status --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let recast = &v["recasts"][0];
    assert_eq!(recast["role"], "director", "{v}");
    assert_eq!(recast["actor"]["runner"], "fake");
    assert_eq!(recast["actor"]["model"], "new-model");
    assert_eq!(recast["actor"]["effort"], "high");
    assert_eq!(
        recast["actor"]["permission_profile"], "standard",
        "kept from the current identity"
    );
    assert_eq!(recast["reason"], "quota");
    assert_eq!(recast["card"], serde_json::Value::Null);
    assert_eq!(v["last_seq"], recast["seq"], "the last line is the recast");
    // The event's author is the user.
    let events = crate::store::load_events(&crate::store::studio_dir(&scratch.dir, "acme-shop"))
        .unwrap()
        .0;
    assert_eq!(events.last().unwrap().by.role, "user");
    assert_eq!(events.last().unwrap().kind.name(), "role.recast");
}

/// A role terminal is refused before the ledger is read — exactly
/// like `card apply`.
#[test]
fn a_role_terminal_cannot_recast() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    let mut vars = env.vars.clone();
    vars.insert("BACKLOT_ROLE".into(), "producer".into());
    let role_env = Env { vars };
    let msg = refused(
        &role_env,
        "recast --person director --runner fake --model new-model --effort high",
    );
    assert!(msg.contains("BACKLOT_ROLE is set"), "{msg}");
}

/// An unsupported runner refuses with the dry run's line.
#[test]
fn an_unsupported_runner_refuses_before_the_append() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    let msg = refused(
        &env,
        "recast --person director --runner runner-x --model m --effort high",
    );
    assert!(msg.contains("runner 'runner-x' is not supported"), "{msg}");
}

/// A Hollywood recast card reads `recast <role> → <runner>/<model>`
/// and `card show` names both identities.
#[test]
fn a_hollywood_recast_card_shows_both_identities() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    ok(
        &env,
        "hollywood propose recast --studio acme-shop --person director --runner fake \
         --model new-model --effort high --why quota --as hollywood",
    );
    let list = ok(&env, "card list");
    assert!(
        list.contains("recast director → fake/new-model/high"),
        "{list}"
    );
    let show = ok(&env, "card show 1");
    assert!(show.contains("director: runner-d/model-d/high"), "{show}");
    assert!(show.contains("→ fake/new-model/high"), "{show}");
}

/// A card born invalid never exists: the propose refuses the same way
/// the command does.
#[test]
fn a_born_invalid_recast_card_is_refused() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    let msg = refused(
        &env,
        "hollywood propose recast --studio acme-shop --person director --runner runner-x \
         --model m --effort high --why quota --as hollywood",
    );
    assert!(msg.contains("runner 'runner-x' is not supported"), "{msg}");
    // No card was recorded.
    let list = ok(&env, "card list");
    assert!(!list.contains("recast director"), "{list}");
}

/// The usage line names every flag, and a missing one is a usage
/// error.
#[test]
fn recast_help_and_usage() {
    let (_scratch, env) = scratch();
    let help = ok(&env, "recast --help");
    assert!(
        help.contains("backlot recast --person producer|director|writer"),
        "{help}"
    );
    assert!(help.contains("--runner <r>"), "{help}");
    assert!(help.contains("--model <m>"), "{help}");
    assert!(
        help.contains("--permission-profile standard|worktree"),
        "{help}"
    );
    assert!(help.contains("--reason <r>"), "{help}");
    let msg = usage(&env, "recast --person director --model m");
    assert!(msg.contains("missing --runner"), "{msg}");
    let msg = usage(&env, "recast --person director --runner fake");
    assert!(msg.contains("missing --model"), "{msg}");
}
