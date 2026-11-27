//! CLI & status/metrics integration tests for `backlot halt` and `backlot resume`.

use std::fs;

use crate::cli_testkit::*;

fn run_args(env: &crate::cli::Env, args: &[&str]) -> String {
    let owned: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    crate::cli::run(&owned, env).unwrap_or_else(|e| panic!("failed {args:?}: {e:?}"))
}

fn setup_running_studio() -> (Scratch, crate::cli::Env) {
    let (scratch, env) = scratch();
    let cast = fixture(
        &scratch,
        "cast.toml",
        &format!("design_gate = \"off\"\n{CAST_TOML}"),
    );
    ok(
        &env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let req = fixture(&scratch, "req.md", "# v1\nbuild it\n");
    ok(
        &env,
        &format!("req revise --file {req} --summary s1 --as producer"),
    );
    let acc = fixture(&scratch, "acc.md", "- works\n");
    ok(
        &env,
        &format!("task new --title T1 --kind bugfix --acceptance-file {acc} --as director"),
    );
    ok(
        &env,
        &format!("task new --title T2 --kind feature --acceptance-file {acc} --as director"),
    );
    ok(
        &env,
        "cast --task t1 --actor actor-a --reason first --as director",
    );
    (scratch, env)
}

#[test]
fn cli_halt_resume_status_and_metrics_end_to_end() {
    let (scratch, mut env) = setup_running_studio();
    let work = work_area(&scratch, "t1");
    let c1 = commit_in(&work, "base-fix");
    let c2 = commit_in(&work, "done-fix");

    // Bare "actor" and "hollywood" cannot halt.
    assert!(refused(&env, "halt --reason bad --as actor").contains("actor-<task>-c<n>"));
    assert!(refused(&env, "halt --reason bad --as hollywood").contains("hollywood"));

    // Actor key `actor-t1-c1` can halt; studio still keeps running tasks alive.
    let conf = run_args(
        &env,
        &[
            "halt",
            "--reason",
            "wrong base branch",
            "--as",
            "actor-t1-c1",
        ],
    );
    assert!(conf.contains("studio.halted"), "{conf}");

    // New TaskCast is refused while halted.
    let cast_err = refused(
        &env,
        "cast --task t2 --actor actor-b --reason second --as director",
    );
    assert!(cast_err.contains("halted"), "{cast_err}");

    // Running task t1 still delivers, reports progress, tells, and asks.
    let mut actor_env = crate::cli::Env {
        vars: env.vars.clone(),
    };
    actor_env
        .vars
        .insert("BACKLOT_ROLE".into(), "actor-t1-c1".into());
    actor_env.vars.insert("BACKLOT_TASK".into(), "t1".into());
    run_args(&actor_env, &["progress", "--text", "still moving"]);
    let ask_f = fixture(&scratch, "ask.md", "Need clarification on fixture?\n");
    ok(&env, &format!("ask --file {ask_f} --as producer"));
    let rpt = fixture(&scratch, "rpt.md", "# Report\n\nAll good.\n");
    ok(
        &env,
        &format!("deliver --task t1 --commits {c1}..{c2} --report {rpt} --as actor-t1-c1"),
    );
    ok(
        &env,
        "review --task t1 --delivery 1 --verdict accepted --conclusion ok --as director",
    );
    run_args(
        &env,
        &[
            "tell",
            "user",
            "--text",
            "stopped because base is old",
            "--note",
            "--as",
            "producer",
        ],
    );

    // Status shows halted line.
    let st = ok(&env, "status");
    assert!(
        st.contains("halted: by actor-t1-c1") && st.contains("wrong base branch"),
        "{st}"
    );

    // Writer / actor / hollywood cannot resume; director/producer/user can.
    assert!(refused(&env, "resume --note go --as writer").contains("cannot do this"));
    assert!(refused(&env, "resume --note go --as actor-t1-c1").contains("cannot do this"));
    assert!(refused(&env, "resume --note go --as hollywood").contains("hollywood"));

    let res = run_args(
        &env,
        &[
            "resume",
            "--note",
            "rebased onto main",
            "--false-positive",
            "--as",
            "director",
        ],
    );
    assert!(res.contains("studio.resumed"), "{res}");

    // Now casting t2 works again and status drops halted line.
    ok(
        &env,
        "cast --task t2 --actor actor-b --reason second --as director",
    );
    let st_after = ok(&env, "status");
    assert!(!st_after.contains("halted:"), "{st_after}");

    // Default user caller (no BACKLOT_ROLE and no --as) works for halt/resume.
    env.vars.remove("BACKLOT_ROLE");
    run_args(&env, &["halt", "--reason", "user sanity check"]);
    run_args(&env, &["resume", "--note", "looks good"]);

    // Metrics JSON & table expose halts, resumes, false-positive share, and halted duration.
    let m_json = ok(&env, "metrics --studio acme-shop --json");
    let v: serde_json::Value = serde_json::from_str(&m_json).unwrap();
    let h = &v["studios"][0]["halts"];
    assert_eq!(h["halts"], 2);
    assert_eq!(h["resumes"], 2);
    assert_eq!(h["false_positives"], 1);
    assert!((h["false_positive_share"].as_f64().unwrap() - 0.5).abs() < 1e-9);
    assert!(h["halted_ms"].is_u64());

    let table = ok(&env, "metrics --studio acme-shop");
    assert!(table.contains("halts"), "{table}");
    assert!(table.contains("fp-share"), "{table}");
    for line in table.lines() {
        assert!(line.chars().count() <= 100, "line > 100 cols: {line}");
    }

    // Verify prompt SHA-256 helper hashes rendered file contents cleanly.
    let pfile = scratch.dir.join("p.md");
    fs::write(&pfile, "hello prompt").unwrap();
    assert_eq!(
        crate::prompts::file_sha256(&pfile).as_deref(),
        Some(backlot_model::hollywood::sha256_hex(b"hello prompt").as_str())
    );
}
