//! `backlot recast` through the real CLI on a scratch state dir: the
//! user records one, a role terminal cannot, and the usage names the
//! flags.

use crate::cli::Env;
use crate::cli_testkit::*;

fn studio(scratch: &Scratch, env: &Env) {
    let cast = fixture(
        scratch,
        "cast.toml",
        &format!("design_gate = \"off\"\n{CAST_TOML}"),
    );
    ok(
        env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
}

/// Requirement v1, so a task may be created.
fn requirement(scratch: &Scratch, env: &Env) {
    let req = fixture(scratch, "req.md", "# req\n");
    ok(
        env,
        &format!("req revise --file {req} --summary s --as producer"),
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

/// The effort refusal reads in plain words from the caps: a required
/// runner names its levels and the flag to add.
#[test]
fn the_effort_refusal_reads_in_plain_words() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    let missing = refused(&env, "recast --person director --runner fake --model m");
    assert!(
        missing.contains("fake takes an effort (low, medium, high); add --effort"),
        "{missing}"
    );
}

/// The user's recast of an actor key: a new identity on the task's
/// newest cast, the same key and terminal name, `by: user`.
#[test]
fn an_actor_recast_records_the_new_identity_on_the_cast() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    // t1 needs a cast before its actor has a key to recast.
    requirement(&scratch, &env);
    let acc = fixture(&scratch, "acc.md", "# acc\n");
    ok(
        &env,
        &format!("task new --title cart --kind feature --acceptance-file {acc} --as director"),
    );
    ok(&env, "cast --task t1 --actor actor-a --as director");
    ok(
        &env,
        "recast --person actor-t1-c1 --runner fake --model new-model --effort high \
         --reason quota",
    );
    let json = ok(&env, "status --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let cast = &v["tasks"]["t1"]["cast"];
    assert_eq!(cast["actor"]["model"], "new-model", "{v}");
    assert_eq!(cast["candidate"], "actor-a", "the key and candidate stay");
    // One recast record, for the actor key, by the user.
    let recasts = v["recasts"].as_array().unwrap();
    assert_eq!(recasts.len(), 1, "{v}");
    assert_eq!(recasts[0]["role"], "actor-t1-c1");
    let events = crate::store::load_events(&crate::store::studio_dir(&scratch.dir, "acme-shop"))
        .unwrap()
        .0;
    assert_eq!(events.last().unwrap().by.role, "user");
    assert_eq!(events.last().unwrap().kind.name(), "role.recast");
}

/// A key the task no longer has is refused — recast the newest cast's.
#[test]
fn a_stale_actor_key_refuses() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    requirement(&scratch, &env);
    let acc = fixture(&scratch, "acc.md", "# acc\n");
    ok(
        &env,
        &format!("task new --title cart --kind feature --acceptance-file {acc} --as director"),
    );
    ok(&env, "cast --task t1 --actor actor-a --as director");
    ok(&env, "cast --task t1 --actor actor-b --as director");
    let msg = refused(
        &env,
        "recast --person actor-t1-c1 --runner fake --model new-model --effort high",
    );
    assert!(msg.contains("not the task's current actor"), "{msg}");
}

/// The usage names the actor keys too.
#[test]
fn the_usage_names_actor_keys() {
    let (_scratch, env) = scratch();
    let help = ok(&env, "recast --help");
    assert!(help.contains("actor-<task>-c<n>"), "{help}");
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
        help.contains("backlot recast --person producer|director|writer | actor-<task>-c<n>"),
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
