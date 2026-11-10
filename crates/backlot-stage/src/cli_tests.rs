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
            "/home/user/other".into(),
            "--cast".into(),
            fixture(&scratch, "cast.toml", CAST_TOML),
        ],
        &env,
    );
    assert!(matches!(again, Err(Fail::Refused(_))));
}

/// A relative `--repo` is a usage error (exit 2) — the studio would
/// otherwise bind to whatever cwd the stage happened to run from.
/// An existing path is canonicalized (`..` resolved); a missing
/// path keeps its spelling, since fixtures use neutral paths.
#[test]
fn studio_new_repo_must_be_absolute_and_canonical() {
    let (scratch, env) = scratch();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    let err = run(
        &[
            "studio".into(),
            "new".into(),
            "--title".into(),
            "t".into(),
            "--repo".into(),
            "relative/shop".into(),
            "--cast".into(),
            cast.clone(),
        ],
        &env,
    );
    assert!(matches!(err, Err(Fail::Usage(_))));

    // An existing path canonicalizes: `<dir>/./` records `<dir>`.
    let repo_dir = scratch.dir.join("repo");
    fs::create_dir(&repo_dir).unwrap();
    let spelled = format!("{}/.", repo_dir.display());
    let out = run(
        &[
            "studio".into(),
            "new".into(),
            "--title".into(),
            "t".into(),
            "--repo".into(),
            spelled,
            "--cast".into(),
            cast,
        ],
        &env,
    )
    .unwrap();
    assert!(out.contains("studio.created"), "{out}");
    let (events, _) =
        crate::store::load_events(&crate::store::studio_dir(&scratch.dir, "acme-shop")).unwrap();
    match &events[0].kind {
        backlot_model::EventKind::StudioCreated { repo, .. } => {
            // Same canonicalization the parser ran — /var →
            // /private/var on macOS included.
            assert_eq!(*repo, repo_dir.canonicalize().unwrap().to_string_lossy());
        }
        other => panic!("wrong event: {other:?}"),
    }
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
    let work = work_area(&scratch, "t1");
    let a = commit_in(&work, "one");
    let b = commit_in(&work, "two");
    let c = commit_in(&work, "three");
    let rep1 = fixture(&scratch, "r1.md", "report 1\n");
    let out = ok(
        &env,
        &format!(
            "deliver --commits {a}..{b} --report {rep1} --check tests=pass --as actor-t1-c1 --task t1"
        ),
    );
    assert!(out.contains("task.delivered t1 d1"));
    // While delivered, a new delivery is refused until a review lands.
    assert!(
        refused(
            &env,
            &format!("deliver --commits {b}..{c} --report {rep1} --as actor-t1-c1 --task t1")
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
        &format!("deliver --commits {b}..{c} --report {rep2} --as actor-t1-c1 --task t1"),
    );
    ok(
        &env,
        "review --task t1 --delivery 2 --verdict accepted --conclusion done --as director",
    );

    // Status shows the accepted task — and the step the ledger now
    // owes: every task terminal, no node report yet, so the director
    // holds it (S9a).
    let status = ok(&env, "status");
    assert!(status.contains("acme-shop"), "{status}");
    assert!(status.contains("t1"), "{status}");
    assert!(status.contains("accepted"), "{status}");
    assert!(status.contains("rework 1/2"), "{status}");
    assert!(
        status.contains("next: node report — director, owed"),
        "{status}"
    );
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
    // A cast file whose stall limit is zero is a usage error like
    // any other bad value — and it creates nothing.
    let zero = fixture(
        &scratch,
        "cast-zero.toml",
        &CAST_TOML.replacen("rework_cap = 2", "stall_limit_s = 0", 1),
    );
    let mut vars = env.vars.clone();
    vars.insert("BACKLOT_STUDIO".into(), "shop-zero".into());
    let env_zero = crate::cli::Env { vars };
    assert!(
        usage(
            &env_zero,
            &format!("studio new --title Zero --repo /home/user/acme-shop --cast {zero}")
        )
        .contains("stall_limit_s")
    );
    assert!(
        !crate::store::studio_dir(&scratch.dir, "shop-zero").exists(),
        "the refused studio was still created"
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
            &format!("deliver --commits a..b --report {rep} --as user --task t1")
        )
        .contains("has no cast")
    );
    // A `--studio` away from the pin is refused by the identity
    // check before the studio is even looked up — the missing-studio
    // refusal needs an unpinned env.
    let mut vars = env.vars.clone();
    vars.remove("BACKLOT_STUDIO");
    assert!(
        refused(&crate::cli::Env { vars }, "status --studio missing")
            .contains("no studio 'missing'")
    );
}

/// Zero nudge and escalate delays are usage errors like
/// `stall_limit_s = 0` — and they create nothing.
#[test]
fn zero_nudge_delays_are_usage_errors() {
    let (scratch, env) = scratch();
    for (key, name) in [("nudge_after_s", "z-nudge"), ("escalate_after_s", "z-esc")] {
        let cast = fixture(
            &scratch,
            &format!("cast-{name}.toml"),
            &CAST_TOML.replacen("rework_cap = 2", &format!("{key} = 0"), 1),
        );
        let mut vars = env.vars.clone();
        vars.insert("BACKLOT_STUDIO".into(), name.into());
        let env_zero = crate::cli::Env { vars };
        assert!(
            usage(
                &env_zero,
                &format!("studio new --title Zero --repo /home/user/acme-shop --cast {cast}")
            )
            .contains(key)
        );
        assert!(
            !crate::store::studio_dir(&scratch.dir, name).exists(),
            "the refused studio was still created"
        );
    }
    // Set numbers reach the ledger unchanged.
    let cast = fixture(
        &scratch,
        "cast-timing.toml",
        &CAST_TOML.replacen(
            "rework_cap = 2",
            "nudge_after_s = 5\nescalate_after_s = 9",
            1,
        ),
    );
    ok(
        &env,
        &format!("studio new --title Timed --repo /home/user/acme-shop --cast {cast}"),
    );
    let (events, _) =
        crate::store::load_events(&crate::store::studio_dir(&scratch.dir, "acme-shop")).unwrap();
    match &events[0].kind {
        backlot_model::EventKind::StudioCreated {
            nudge_after_s,
            escalate_after_s,
            ..
        } => {
            assert_eq!(*nudge_after_s, 5);
            assert_eq!(*escalate_after_s, 9);
        }
        other => panic!("wrong event: {other:?}"),
    }
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

/// A pinned role terminal cannot claim another identity: `--as`,
/// `--studio` and `--task` must equal their `BACKLOT_*` variable or
/// be absent — a disagreeing flag refuses before anything is
/// appended. Equal is fine, and unset the flag works as it always
/// has (the user's shell and the panel pass `--as` unpinned).
#[test]
fn a_pinned_terminal_cannot_claim_another_identity() {
    let (scratch, mut env) = scratch();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let events = || {
        crate::store::load_events(&crate::store::studio_dir(&scratch.dir, "acme-shop"))
            .unwrap()
            .0
            .len()
    };
    env.vars.insert("BACKLOT_ROLE".into(), "actor-t1-c1".into());
    // Another `--as` names both values and appends nothing — the
    // approval it tried to answer never lands.
    for (args, claimed) in [
        (
            "approve --person actor-t1-c2 --decision allow --as director",
            "director",
        ),
        (
            "approve --person actor-t1-c2 --decision allow --as user",
            "user",
        ),
    ] {
        let m = refused(&env, args);
        assert!(m.contains("actor-t1-c1"), "{args}: {m}");
        assert!(m.contains(claimed), "{args}: {m}");
    }
    // `--studio` and `--task` against their pins refuse the same way.
    let m = refused(&env, "status --studio other --as actor-t1-c1");
    assert!(m.contains("acme-shop") && m.contains("other"), "{m}");
    env.vars.insert("BACKLOT_TASK".into(), "t1".into());
    let m = refused(&env, "task pause --task t2 --reason r --as actor-t1-c1");
    assert!(m.contains("t1") && m.contains("t2"), "{m}");
    assert_eq!(events(), 1, "a refused flag appends nothing");
    // A flag equal to its variable is fine — the command still runs.
    ok(&env, "status --studio acme-shop --as actor-t1-c1 --task t1");
    // Unset, the same flags work exactly as before.
    env.vars.remove("BACKLOT_ROLE");
    env.vars.remove("BACKLOT_TASK");
    ok(&env, "status --as director");
    env.vars.remove("BACKLOT_STUDIO");
    ok(&env, "status --studio acme-shop --as user");
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

/// `stage` subcommand parsing plus `run`'s outside-the-terminal
/// refusal (which happens before any core call).
#[test]
fn stage_parses_and_run_refuses_outside() {
    let (_scratch, env) = scratch();
    assert!(usage(&env, "stage").contains("missing stage subcommand"));
    assert!(usage(&env, "stage nope").contains("unknown stage subcommand"));
    let msg = refused(&env, "stage run");
    assert!(msg.contains("stage start"), "{msg}");
}

/// `stage start` always connects (it creates a terminal); absent
/// endpoint pointers make that a clear refusal, never a real session.
#[test]
fn stage_start_with_no_core_is_a_clear_refusal() {
    let mut vars = std::collections::BTreeMap::new();
    vars.insert("HOME".into(), "/definitely/absent".into());
    vars.insert("XDG_RUNTIME_DIR".into(), "/definitely/absent/run".into());
    vars.insert("HC_SOCKET".into(), "/definitely/absent/hc.sock".into());
    let env = crate::cli::Env { vars };
    let msg = refused(&env, "stage start");
    assert!(msg.contains("cannot reach the hc core"), "{msg}");
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

/// Every first word the parser accepts is registered in `COMMANDS` —
/// a missing one falls through `maybe_run` into the TUI's own
/// parser, where it dies as an unexpected argument. `maybe_run` on
/// the bare word is a usage refusal, never a `None`.
#[test]
fn every_command_word_is_registered() {
    for word in [
        "studio", "req", "plan", "dispatch", "report", "ask", "tell", "task", "cast", "priority",
        "reopen", "close", "approve", "deliver", "block", "review", "finding", "correct", "status",
        "records", "stage",
    ] {
        assert!(
            crate::cli::maybe_run(&[word.to_string()]).is_some(),
            "{word} is missing from COMMANDS"
        );
    }
}
