//! Usage flags on `deliver` and the `status`/`records` render
//! (P4a): valid figures land on the `task.delivered` event and
//! aggregate into the factual views; absent flags write no `usage`
//! key at all; malformed values are usage errors.

use backlot_model::event::EventKind;

use crate::cli_testkit::*;

/// Studio + cast + one commit, ready for `deliver` on `t1`.
fn ready() -> (Scratch, crate::cli::Env, String, String) {
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
    let rep = fixture(&scratch, "r1.md", "report 1\n");
    (scratch, env, format!("{a}..{b}"), rep)
}

/// The ledger's `task.delivered` events for `acme-shop`.
fn delivered_events(scratch: &Scratch) -> Vec<backlot_model::Event> {
    let (events, _) =
        crate::store::load_events(&crate::store::studio_dir(&scratch.dir, "acme-shop")).unwrap();
    events
        .into_iter()
        .filter(|e| matches!(e.kind, EventKind::TaskDelivered { .. }))
        .collect()
}

/// `--tokens-*`/`--cost-usd` figures land on the event as
/// `usage` — `0.08` parses to exactly 80_000 micro-dollars — and
/// `status`/`records` show the compact factual line.
#[test]
fn deliver_usage_flags_record_and_display() {
    let (scratch, env, commits, rep) = ready();
    let out = ok(
        &env,
        &format!(
            "deliver --commits {commits} --report {rep} --tokens-in 12400 --tokens-out 3100 --cache-read-tokens 2000 --cost-usd 0.08 --as actor-t1-c1 --task t1"
        ),
    );
    assert!(out.contains("task.delivered t1 d1"), "{out}");

    let deliveries = delivered_events(&scratch);
    assert_eq!(deliveries.len(), 1);
    let EventKind::TaskDelivered { usage, .. } = &deliveries[0].kind else {
        unreachable!()
    };
    let usage = usage.as_ref().expect("usage recorded");
    assert_eq!(usage.input_tokens, Some(12_400));
    assert_eq!(usage.output_tokens, Some(3_100));
    assert_eq!(usage.cache_read_tokens, Some(2_000));
    assert_eq!(usage.cost_micros_usd, Some(80_000));

    let status = ok(&env, "status");
    assert!(
        status.contains("usage: 12.4k in / 3.1k out / 2.0k cache · $0.08"),
        "{status}"
    );
    let recs = ok(&env, "records");
    assert!(
        recs.contains("usage 12.4k in / 3.1k out / 2.0k cache · $0.08"),
        "{recs}"
    );
    // `--all` spells the same line.
    let status_all = ok(&env, "status --all");
    assert!(status_all.contains("usage: 12.4k in"), "{status_all}");
}

/// No flags: the event writes no `usage` key, `status`/`records`
/// show no usage segment — byte-identical to before.
#[test]
fn deliver_without_usage_leaves_no_trace() {
    let (scratch, env, commits, rep) = ready();
    ok(
        &env,
        &format!("deliver --commits {commits} --report {rep} --as actor-t1-c1 --task t1"),
    );
    let deliveries = delivered_events(&scratch);
    assert_eq!(deliveries.len(), 1);
    let EventKind::TaskDelivered { usage, .. } = &deliveries[0].kind else {
        unreachable!()
    };
    assert_eq!(*usage, None);
    let line = serde_json::to_value(&deliveries[0]).unwrap();
    assert!(line.get("usage").is_none(), "{line}");
    assert!(!ok(&env, "status").contains("usage:"));
    assert!(!ok(&env, "records").contains("usage"));
}

/// A `--cost-usd` fraction beyond micro precision, a negative and a
/// non-numeric all fail as usage errors — as do bad token counts.
#[test]
fn bad_usage_figures_are_usage_errors() {
    let (_scratch, env, commits, rep) = ready();
    for flag in [
        "--cost-usd 0.1234567", // beyond micro precision
        "--cost-usd -1",
        "--cost-usd abc",
        "--cost-usd .5",
        "--cost-usd 1.2.3",
        "--tokens-in -5",
        "--tokens-in x",
        "--tokens-out 1.5",
    ] {
        let err = usage(
            &env,
            &format!(
                "deliver --commits {commits} --report {rep} {flag} --as actor-t1-c1 --task t1"
            ),
        );
        assert!(err.contains("bad --"), "{flag}: {err}");
    }
    // And a bare `--cost-usd` with no value fails the same way.
    let err = usage(
        &env,
        &format!(
            "deliver --commits {commits} --report {rep} --cost-usd --as actor-t1-c1 --task t1"
        ),
    );
    assert!(err.contains("cost-usd"), "{err}");
    // Nothing was delivered.
    assert!(delivered_events(&_scratch).is_empty());
}

/// Whole-dollar, whole-micro and equal spellings land exactly.
#[test]
fn cost_usd_parses_exactly() {
    let (_scratch, env, commits, rep) = ready();
    ok(
        &env,
        &format!(
            "deliver --commits {commits} --report {rep} --cost-usd 5 --as actor-t1-c1 --task t1"
        ),
    );
    let deliveries = delivered_events(&_scratch);
    let EventKind::TaskDelivered { usage, .. } = &deliveries[0].kind else {
        unreachable!()
    };
    assert_eq!(usage.as_ref().unwrap().cost_micros_usd, Some(5_000_000));
    // `=`-joined flags parse the same.
    let (scratch2, env2, commits2, rep2) = ready();
    let _ = scratch2;
    ok(
        &env2,
        &format!(
            "deliver --commits {commits2} --report {rep2} --tokens-in=12400 --cost-usd=1.2345 --as actor-t1-c1 --task t1"
        ),
    );
    let deliveries = delivered_events(&scratch2);
    let EventKind::TaskDelivered { usage, .. } = &deliveries[0].kind else {
        unreachable!()
    };
    let usage = usage.as_ref().unwrap();
    assert_eq!(usage.input_tokens, Some(12_400));
    assert_eq!(usage.cost_micros_usd, Some(1_234_500));
}
