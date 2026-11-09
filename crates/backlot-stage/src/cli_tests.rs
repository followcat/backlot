//! CLI tests driving `cli::run` in-process with a scratch state dir.

use std::fs;

use crate::cli::{Fail, run};
use crate::cli_testkit::*;

#[test]
fn studio_new_records_everything() {
    let (scratch, env) = scratch();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    let out = run(
        &[
            "studio".into(),
            "new".into(),
            "--title".into(),
            "The Shop".into(),
            "--repo".into(),
            "/home/user/acme-shop".into(),
            "--cast".into(),
            cast,
        ],
        &env,
    )
    .unwrap();
    assert!(out.contains("studio.created"));
    assert!(out.contains("'The Shop'"));
    // Role defaults to "user" for studio new.
    let (events, _) =
        crate::store::load_events(&crate::store::studio_dir(&scratch.dir, "acme-shop")).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].by.role, "user");
    match &events[0].kind {
        backlot_model::EventKind::StudioCreated {
            candidates,
            rework_cap,
            ..
        } => {
            assert_eq!(candidates.len(), 2);
            assert_eq!(*rework_cap, 2);
        }
        other => panic!("wrong event: {other:?}"),
    }
    // A second studio.created is refused.
    let again = run(
        &[
            "studio".into(),
            "new".into(),
            "--title".into(),
            "x".into(),
            "--repo".into(),
            "y".into(),
            "--cast".into(),
            fixture(&scratch, "cast.toml", CAST_TOML),
        ],
        &env,
    );
    assert!(matches!(again, Err(Fail::Refused(_))));
}

#[test]
fn full_loop_status_and_records() {
    let (scratch, env) = scratch();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let req = fixture(&scratch, "req.md", "# requirement\nbuild the shop\n");
    ok(
        &env,
        &format!("req revise --file {req} --summary first-cut --as producer"),
    );
    ok(&env, "plan request --as producer");
    let plan = fixture(&scratch, "plan.md", "# plan\n");
    ok(
        &env,
        &format!("plan publish --kind plan --file {plan} --as writer"),
    );
    let acc = fixture(&scratch, "acc.md", "# acceptance\n");
    ok(
        &env,
        &format!("plan publish --kind acceptance --file {acc} --as writer"),
    );
    let disp = fixture(&scratch, "dispatch.md", "director: go\n");
    ok(&env, &format!("dispatch --file {disp} --as producer"));
    ok(
        &env,
        &format!("task new --title cart --kind feature --acceptance-file {acc} --as director"),
    );
    ok(
        &env,
        "cast --task t1 --actor actor-a --reason strongest --as director",
    );
    let rep1 = fixture(&scratch, "r1.md", "report 1\n");
    let out = ok(
        &env,
        &format!("deliver --commits a..b --report {rep1} --check tests=pass --as actor --task t1"),
    );
    assert!(out.contains("task.delivered t1 d1"));
    // While delivered, a new delivery is refused until a review lands.
    assert!(
        refused(
            &env,
            &format!("deliver --commits b..c --report {rep1} --as actor --task t1")
        )
        .contains("not running")
    );
    let findings = fixture(
        &scratch,
        "f.json",
        r#"[{"id":"D1","summary":"cart misses tax","severity":"medium","cause":"delivery_defect"}]"#,
    );
    ok(
        &env,
        &format!(
            "review --task t1 --delivery 1 --verdict changes-requested --conclusion close --findings {findings} --as director"
        ),
    );
    // Reviewing while the task is back running is refused.
    assert!(
        refused(
            &env,
            "review --task t1 --delivery 9 --verdict accepted --conclusion x --as director"
        )
        .contains("only a delivered")
    );
    let rep2 = fixture(&scratch, "r2.md", "report 2\n");
    ok(
        &env,
        &format!("deliver --commits b..c --report {rep2} --as actor --task t1"),
    );
    ok(
        &env,
        "review --task t1 --delivery 2 --verdict accepted --conclusion done --as director",
    );

    // Status shows the accepted task.
    let status = ok(&env, "status");
    assert!(status.contains("acme-shop"), "{status}");
    assert!(status.contains("t1"), "{status}");
    assert!(status.contains("accepted"), "{status}");
    assert!(status.contains("rework 1/2"), "{status}");
    // JSON view parses.
    let json = ok(&env, "status --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["tasks"]["t1"]["status"], "accepted");

    // Records: actor-a has one task — not enough data.
    let recs = ok(&env, "records");
    assert!(recs.contains("not enough data"), "{recs}");
    let recs_json = ok(&env, "records --json");
    let r: serde_json::Value = serde_json::from_str(&recs_json).unwrap();
    assert_eq!(r[0]["tasks"], 1);
    assert_eq!(r[0]["first_delivery"]["changes_requested"], 1);
    assert_eq!(r[0]["final_outcome"]["accepted"], 1);
    assert_eq!(r[0]["defects"], 1);
    assert_eq!(r[0]["actor_fault_defects"], 1);

    // The ledger files sit inside the studio dir, not the worktree.
    let sdir = crate::store::studio_dir(&scratch.dir, "acme-shop");
    assert!(sdir.join("req/v1.md").is_file());
    assert!(
        fs::read_to_string(sdir.join("req/v1.md"))
            .unwrap()
            .contains("build the shop")
    );
    assert!(sdir.join("reports").read_dir().unwrap().count() >= 2);
    assert!(sdir.join("plan").read_dir().unwrap().count() >= 3);
    assert!(sdir.join("messages").read_dir().unwrap().count() >= 1);
    let report_path = v["tasks"]["t1"]["deliveries"][0]["report"]
        .as_str()
        .unwrap();
    assert!(!report_path.starts_with('/'), "{report_path}");
}

#[test]
fn exit_kinds_usage_vs_refused() {
    let (scratch, env) = scratch();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    // Usage: missing flags, unknown flags, bad values.
    assert!(usage(&env, "req revise --file x").contains("missing --summary"));
    assert!(usage(&env, "deliver --commits a..b --report x").contains("missing role"));
    assert!(usage(&env, "studio new --title t").contains("missing --repo"));
    assert!(usage(&env, "status --bogus").contains("unexpected argument"));
    assert!(
        usage(
            &env,
            "review --task t1 --delivery 1 --verdict meh --conclusion c --as director"
        )
        .contains("bad --verdict")
    );
    assert!(
        usage(
            &env,
            "req revise --file /definitely/missing.md --summary s --as producer"
        )
        .contains("file not found")
    );
    // Refused: state-level failures. A task pins a requirement version,
    // so one must exist first.
    let req = fixture(&scratch, "req.md", "# req\n");
    ok(
        &env,
        &format!("req revise --file {req} --summary s --as producer"),
    );
    let acc = fixture(&scratch, "acc.md", "# acc\n");
    ok(
        &env,
        &format!("task new --title cart --kind feature --acceptance-file {acc} --as director"),
    );
    assert!(refused(&env, "cast --task t9 --actor actor-a --as director").contains("no task 't9'"));
    assert!(
        refused(&env, "cast --task t1 --actor nobody --as director")
            .contains("not a studio candidate")
    );
    let rep = fixture(&scratch, "r.md", "report\n");
    assert!(
        refused(
            &env,
            &format!("deliver --commits a..b --report {rep} --as actor --task t1")
        )
        .contains("has no cast")
    );
    assert!(refused(&env, "status --studio missing").contains("no studio 'missing'"));
}

#[test]
fn missing_caller_values_are_usage_errors() {
    let (_scratch, mut env) = scratch();
    env.vars.remove("BACKLOT_STUDIO");
    assert!(usage(&env, "status").contains("BACKLOT_STUDIO"));
    env.vars.insert("BACKLOT_STUDIO".into(), "acme-shop".into());
    assert!(usage(&env, "block --reason x").contains("BACKLOT_ROLE"));
    env.vars.insert("BACKLOT_ROLE".into(), "actor".into());
    assert!(usage(&env, "block --reason x").contains("BACKLOT_TASK"));
}
/// Without `BACKLOT_STATE_DIR` a command needs the core for its id;
/// an unreachable one refuses with a clear message. Every pointer is
/// aimed at absent paths so a real session can never answer.
#[test]
fn no_state_dir_and_no_core_is_a_clear_refusal() {
    let mut vars = std::collections::BTreeMap::new();
    vars.insert("HOME".into(), "/definitely/absent".into());
    vars.insert("XDG_RUNTIME_DIR".into(), "/definitely/absent/run".into());
    vars.insert("HC_SOCKET".into(), "/definitely/absent/hc.sock".into());
    let env = crate::cli::Env { vars };
    let msg = refused(&env, "status --studio acme-shop");
    assert!(msg.contains("cannot reach the hc core"), "{msg}");
    assert!(msg.contains("BACKLOT_STATE_DIR"), "{msg}");
}

/// `--session`/`--remote` parse as common flags; with the override
/// set they cost nothing.
#[test]
fn session_and_remote_are_common_flags() {
    let (scratch, _env) = scratch();
    let mut vars = std::collections::BTreeMap::new();
    vars.insert(
        "BACKLOT_STATE_DIR".into(),
        scratch.dir.to_string_lossy().into_owned(),
    );
    let env = crate::cli::Env { vars };
    // Reaches the ledger lookup, so the flag was consumed, not rejected.
    assert!(
        refused(&env, "status --studio acme-shop --session scratch-x")
            .contains("no studio 'acme-shop'")
    );
}

#[test]
fn cast_toml_needs_all_roles() {
    let (scratch, env) = scratch();
    let bad = fixture(&scratch, "bad.toml", "[producer]\nrunner = \"r\"\n");
    let err = run(
        &[
            "studio".into(),
            "new".into(),
            "--title".into(),
            "t".into(),
            "--repo".into(),
            "r".into(),
            "--cast".into(),
            bad,
        ],
        &env,
    );
    assert!(matches!(err, Err(Fail::Usage(_))), "{err:?}");
}
