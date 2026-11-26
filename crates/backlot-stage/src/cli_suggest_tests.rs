//! `cast --suggest` and `records --kind` tests (L3): the suggestion
//! view is read-only and factual across studios — declaration order,
//! honest thin/no-history notices, never a score or a pick (D8).

use std::collections::BTreeMap;
use std::path::Path;

use backlot_model::event::*;

use crate::cli::{Fail, run};
use crate::cli_testkit::*;
use crate::store::Pending;

/// The actor identity `CAST_TOML`'s `actor-a` candidate declares —
/// seeded history must match it for a `cast --suggest` join.
fn actor_a() -> ActorIdentity {
    ActorIdentity {
        runner: "runner-a".into(),
        runner_version: None,
        model: "model-a".into(),
        effort: Some("high".into()),
        permission_profile: "worktree".into(),
    }
}

/// A bare `studio.created` for the seeded studio — the one
/// candidate suggest's cross-studio join needs is `actor-a`.
fn studio_created(repo: &str) -> EventKind {
    EventKind::StudioCreated {
        title: repo.into(),
        repo: format!("/home/user/{repo}"),
        members: Vec::new(),
        no_git: false,
        mode: StudioMode::Standard,
        roles: Box::new(RoleConfigs {
            producer: actor_a(),
            director: actor_a(),
            writer: actor_a(),
        }),
        candidates: vec![Candidate {
            name: "actor-a".into(),
            runner: "runner-a".into(),
            runner_version: None,
            model: "model-a".into(),
            effort: Some("high".into()),
            permission_profile: "worktree".into(),
        }],
        rework_cap: 2,
        stall_limit_s: 600,
        nudge_after_s: 180,
        escalate_after_s: 600,
        check_env: Vec::new(),
        checks: Vec::new(),
        task_budget_s: None,
        limits: BTreeMap::new(),
        max_actors: backlot_model::event::default_max_actors(),
        push: Default::default(),
        design_gate: DesignGate::Off,
    }
}

/// One real event into `dir`'s log through `store::append` — the
/// model's own validation runs, like every ledger write. `user`
/// may record every kind, which keeps seeding a history to what
/// the fixture means to say.
fn seed(dir: &Path, kind: EventKind) {
    crate::store::append(dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: backlot_model::NewEvent::new(EventBy::role("user"), kind),
            files: Vec::new(),
        })
    })
    .expect("seed event refused");
}

/// `beta-tools` exists with req v1 — call once before `seed_tasks`.
fn seed_studio(scratch: &Scratch) {
    let dir = crate::store::studio_dir(&scratch.dir, "beta-tools");
    seed(&dir, studio_created("beta-tools"));
    seed(
        &dir,
        EventKind::ReqVersion {
            version: 1,
            file: "req/v1.md".into(),
            summary: "first".into(),
        },
    );
}

/// `n` accepted `kind` tasks for `actor-a` on `beta-tools` — cast →
/// delivered → reviewed accepted, like the real flow records them.
/// `start` keeps ids unique across calls.
fn seed_tasks(scratch: &Scratch, kind: &str, n: usize, start: usize) {
    let dir = crate::store::studio_dir(&scratch.dir, "beta-tools");
    for i in start..start + n {
        let t = format!("t{i}");
        let usage = match i {
            1 => Some(Usage {
                input_tokens: Some(1200),
                output_tokens: Some(300),
                ..Usage::default()
            }),
            _ => None,
        };
        for kind_ev in [
            EventKind::TaskCreated {
                task: t.clone(),
                title: format!("task {t}"),
                task_kind: kind.to_string(),
                requirement_version: 1,
                acceptance: "plan/acceptance.md".into(),
                dependencies: Vec::new(),
            },
            EventKind::TaskCast {
                task: t.clone(),
                candidate: "actor-a".into(),
                actor: actor_a(),
                reason: "picked".into(),
                priority: None,
            },
            EventKind::TaskDelivered {
                task: t.clone(),
                delivery: 1,
                actor: actor_a(),
                commits: "a..b".into(),
                report: format!("reports/{t}-d1.md"),
                requirement_version: 1,
                checks: Vec::new(),
                usage,
            },
            EventKind::TaskReviewed {
                task: t,
                delivery: 1,
                reviewer: "director".into(),
                verdict: Verdict::Accepted,
                conclusion: "ok".into(),
                findings: Vec::new(),
                upstream_changes: Vec::new(),
            },
        ] {
            seed(&dir, kind_ev);
        }
    }
}

/// `beta-tools` plus `n` accepted `kind` tasks for `actor-a`.
fn seed_history(scratch: &Scratch, kind: &str, n: usize) {
    seed_studio(scratch);
    seed_tasks(scratch, kind, n, 1);
}

/// `acme-shop` exists with one `kind` task `t1` — the ordinary
/// standard-mode flow: studio, req v1, task.
fn acme_with_task(scratch: &Scratch, env: &crate::cli::Env, kind: &str) {
    let cast = fixture(scratch, "cast.toml", CAST_TOML);
    ok(
        env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let req = fixture(scratch, "req.md", "# requirement\n");
    ok(
        env,
        &format!("req revise --file {req} --summary first --as producer"),
    );
    let acc = fixture(scratch, "acc.md", "# acceptance\n");
    ok(
        env,
        &format!("task new --title one --kind {kind} --acceptance-file {acc} --as director"),
    );
}

/// The suggestion view joins the candidates' cross-studio history:
/// actor-a's six accepted `feature` tasks on `beta-tools` show as a
/// full record; actor-b has none and says so — nothing ranks.
#[test]
fn cast_suggest_text_shows_factual_history() {
    let (scratch, env) = scratch();
    acme_with_task(&scratch, &env, "feature");
    seed_history(&scratch, "feature", 6);

    let out = ok(&env, "cast --suggest --task t1 --as director");
    assert!(out.contains("kind 'feature'"), "{out}");
    assert!(out.contains("no ranking"), "{out}");
    assert!(out.contains("actor-a"), "{out}");
    assert!(out.contains("runner-a/model-a/high (worktree)"), "{out}");
    assert!(out.contains("feature: 6 tasks, 6 deliveries"), "{out}");
    assert!(out.contains("usage 1.2k in / 300 out"), "{out}");
    assert!(out.contains("first delivery: 6 accepted"), "{out}");
    assert!(out.contains("wall time:"), "{out}");
    // actor-b has never run anything — honest notice, not a zero row.
    assert!(out.contains("actor-b"), "{out}");
    assert!(out.contains("(no history for 'feature')"), "{out}");
    // The view keeps the cast's declaration order — never a ranking.
    assert!(out.find("actor-a").unwrap() < out.find("actor-b").unwrap());
}

/// The `--json` view is the structured `CandidateHistory` list —
/// factual fields only, no score anywhere.
#[test]
fn cast_suggest_json_is_the_structured_view() {
    let (scratch, env) = scratch();
    acme_with_task(&scratch, &env, "feature");
    seed_history(&scratch, "feature", 6);

    let out = ok(&env, "cast --suggest --task t1 --json --as director");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let rows = v.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["candidate"]["name"], "actor-a");
    assert_eq!(rows[0]["version_matched"], "exact");
    assert_eq!(rows[0]["exact_kind"]["tasks"], 6);
    assert_eq!(rows[0]["exact_kind"]["task_kind"], "feature");
    assert_eq!(rows[0]["enough_data"], true);
    assert_eq!(rows[1]["candidate"]["name"], "actor-b");
    assert!(rows[1]["exact_kind"].is_null());
    assert_eq!(rows[1]["enough_data"], false);
    for bad in ["score", "rank", "grade", "recommend"] {
        assert!(!out.to_lowercase().contains(bad), "json contains '{bad}'");
    }
}

/// Below `MIN_TASKS` the record prints but is honestly marked —
/// and thin exact-kind history surfaces the actor's other kinds.
#[test]
fn cast_suggest_marks_thin_history() {
    let (scratch, env) = scratch();
    acme_with_task(&scratch, &env, "feature");
    seed_studio(&scratch);
    seed_tasks(&scratch, "feature", 3, 1);
    seed_tasks(&scratch, "fix", 7, 10);

    let out = ok(&env, "cast --suggest --task t1 --as director");
    assert!(out.contains("(not enough data: 3/5)"), "{out}");
    assert!(out.contains("other kinds: fix (7 tasks)"), "{out}");
    let v: serde_json::Value =
        serde_json::from_str(&ok(&env, "cast --suggest --task t1 --json --as director")).unwrap();
    assert_eq!(v[0]["enough_data"], false);
    assert_eq!(v[0]["other_kinds"][0]["task_kind"], "fix");
}

/// A runner-version-agnostic match — history recorded under another
/// `runner_version` still joins, clearly marked.
#[test]
fn cast_suggest_matches_across_runner_versions() {
    let (scratch, env) = scratch();
    acme_with_task(&scratch, &env, "feature");
    let dir = crate::store::studio_dir(&scratch.dir, "beta-tools");
    // History under `runner_version` "9.9" — the candidate pins none.
    let mut ident = actor_a();
    ident.runner_version = Some("9.9".into());
    seed(&dir, studio_created("beta-tools"));
    seed(
        &dir,
        EventKind::ReqVersion {
            version: 1,
            file: "req/v1.md".into(),
            summary: "first".into(),
        },
    );
    for kind_ev in [
        EventKind::TaskCreated {
            task: "t1".into(),
            title: "task".into(),
            task_kind: "feature".into(),
            requirement_version: 1,
            acceptance: "a.md".into(),
            dependencies: Vec::new(),
        },
        EventKind::TaskCast {
            task: "t1".into(),
            candidate: "actor-a".into(),
            actor: actor_a(), // the cast's identity, version unpinned
            reason: "picked".into(),
            priority: None,
        },
        EventKind::TaskDelivered {
            task: "t1".into(),
            delivery: 1,
            actor: ident.clone(),
            commits: "a..b".into(),
            report: "r.md".into(),
            requirement_version: 1,
            checks: Vec::new(),
            usage: None,
        },
        EventKind::TaskReviewed {
            task: "t1".into(),
            delivery: 1,
            reviewer: "director".into(),
            verdict: Verdict::Accepted,
            conclusion: "ok".into(),
            findings: Vec::new(),
            upstream_changes: Vec::new(),
        },
    ] {
        seed(&dir, kind_ev);
    }
    let out = ok(&env, "cast --suggest --task t1 --as director");
    assert!(out.contains("[matched across runner versions]"), "{out}");
    let v: serde_json::Value =
        serde_json::from_str(&ok(&env, "cast --suggest --task t1 --json --as director")).unwrap();
    assert_eq!(v[0]["version_matched"], "across_versions");
    assert_eq!(v[0]["exact_kind"]["tasks"], 1);
}

/// `--suggest` is task-level: `--actor` with it is a usage error
/// (exit 2), as is `--reason` or `--priority` beside it.
#[test]
fn cast_suggest_with_actor_is_a_usage_error() {
    let (_scratch, env) = scratch();
    for s in [
        "cast --task t1 --suggest --actor actor-a",
        "cast --task t1 --suggest --reason r",
        "cast --task t1 --suggest --priority high",
        "cast --task t1 --json",
    ] {
        match run(&s.split(' ').map(String::from).collect::<Vec<_>>(), &env) {
            Err(Fail::Usage(_)) => {}
            other => panic!("expected usage error on '{s}', got {other:?}"),
        }
    }
}

/// A task id that does not exist is a clean refusal, never a panic.
#[test]
fn cast_suggest_unknown_task_is_refused() {
    let (scratch, env) = scratch();
    acme_with_task(&scratch, &env, "feature");
    let err = refused(&env, "cast --suggest --task t9 --as director");
    assert!(err.contains("no task 't9'"), "{err}");
}

/// `records --kind <k>` keeps only that kind's rows — the read
/// itself is unchanged without the flag.
#[test]
fn records_kind_filters_the_rows() {
    let (scratch, env) = scratch();
    seed_studio(&scratch);
    seed_tasks(&scratch, "feature", 6, 1);
    seed_tasks(&scratch, "fix", 2, 10);

    let out = ok(&env, "records --kind feature");
    assert!(out.contains("feature: 6 tasks"), "{out}");
    assert!(!out.contains("fix:"), "{out}");
    let out = ok(&env, "records --kind fix");
    assert!(out.contains("fix: 2 tasks"), "{out}");
    assert!(!out.contains("feature:"), "{out}");
    let out = ok(&env, "records --kind docs");
    assert!(
        out.contains("no deliveries recorded for kind 'docs'"),
        "{out}"
    );
    let v: serde_json::Value =
        serde_json::from_str(&ok(&env, "records --kind fix --json")).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);
    assert_eq!(v[0]["task_kind"], "fix");
}
