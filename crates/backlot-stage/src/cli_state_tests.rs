//! CLI tests for the rework commands: studio-name rules, `finding`,
//! `task pause|resume|abandon|takeover`, and reviews at the cap.

use std::fs;

use crate::cli::Env;
use crate::cli_testkit::*;

/// Studio names are `[a-z0-9_-]{1,16}`: bad names are usage errors
/// (exit 2) checked before any path is built, and commands on a
/// studio with no log are refused without touching the disk.
#[test]
fn studio_names_are_checked_and_missing_studios_refused() {
    // An unpinned shell: with `BACKLOT_STUDIO` set a differing
    // `--studio` is refused before the name is ever checked.
    let (scratch, mut env) = scratch();
    env.vars.remove("BACKLOT_STUDIO");
    let studios_dir = scratch.dir.join("studios");
    let nothing_on_disk =
        || !studios_dir.exists() || fs::read_dir(&studios_dir).unwrap().next().is_none();

    // Traversal, uppercase and over-length names are all usage errors.
    for bad in ["../x", "../../escape", "Upper", "seventeen_charname"] {
        let m = usage(&env, &format!("status --studio {bad}"));
        assert!(m.contains("bad studio name"), "{bad}: {m}");
        let m = usage(&env, &format!("plan request --studio {bad} --as producer"));
        assert!(m.contains("bad studio name"), "{bad}: {m}");
    }
    assert!(nothing_on_disk());

    // A well-formed but absent studio is refused (exit 1) on reads
    // and appends alike; the directory is never created.
    assert!(refused(&env, "status --studio nosuch").contains("no studio 'nosuch'"));
    assert!(
        refused(&env, "plan request --studio nosuch --as producer").contains("no studio 'nosuch'")
    );
    assert!(!scratch.dir.join("studios/nosuch").exists());

    // The 16-char maximum works: `studio new` creates the directory.
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!(
            "studio new --title Shop --repo /home/user/acme-shop --cast {cast} --studio abcdefghijklmnop"
        ),
    );
    assert!(
        scratch
            .dir
            .join("studios/abcdefghijklmnop/events.jsonl")
            .is_file()
    );
}

/// `backlot finding` records defects on open and closed tasks and
/// never moves the status; a delivery the task does not have refuses.
#[test]
fn finding_records_on_open_and_closed_tasks() {
    let (scratch, env) = scratch();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
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
    ok(&env, "cast --task t1 --actor actor-a --as director");
    let rep = fixture(&scratch, "r.md", "report\n");
    ok(
        &env,
        &format!("deliver --commits a..b --report {rep} --as actor --task t1"),
    );

    // --task is required; a delivery the task never had is refused.
    assert!(
        usage(&env, "finding --delivery 1 --findings x.json --as producer")
            .contains("missing task")
    );
    let f = fixture(
        &scratch,
        "f.json",
        r#"[{"id":"D9","summary":"found in integration","severity":"high"}]"#,
    );
    let out = ok(
        &env,
        &format!("finding --task t1 --delivery 1 --findings {f} --as producer"),
    );
    assert!(out.contains("task.finding t1 d1"), "{out}");
    assert!(
        refused(
            &env,
            &format!("finding --task t1 --delivery 9 --findings {f} --as producer")
        )
        .contains("no delivery 9")
    );

    // After acceptance the task still takes findings — status unchanged.
    ok(
        &env,
        "review --task t1 --delivery 1 --verdict accepted --conclusion done --as director",
    );
    ok(
        &env,
        &format!("finding --task t1 --delivery 1 --findings {f} --as producer"),
    );
    let json = ok(&env, "status --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["tasks"]["t1"]["status"], "accepted");
    assert_eq!(
        v["tasks"]["t1"]["finding_reports"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

/// `backlot task pause|resume|abandon|takeover` record task.state
/// events; each has a refused case.
#[test]
fn task_state_commands() {
    let (scratch, env) = scratch();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let req = fixture(&scratch, "req.md", "# req\n");
    ok(
        &env,
        &format!("req revise --file {req} --summary s --as producer"),
    );
    let acc = fixture(&scratch, "acc.md", "# acc\n");
    let new_task = |env: &Env, title: &str| {
        ok(
            env,
            &format!(
                "task new --title {title} --kind feature --acceptance-file {acc} --as director"
            ),
        );
    };
    new_task(&env, "one");
    new_task(&env, "two");
    new_task(&env, "three");
    new_task(&env, "four");
    ok(&env, "cast --task t1 --actor actor-a --as director");
    ok(&env, "cast --task t2 --actor actor-a --as director");
    ok(&env, "cast --task t3 --actor actor-a --as director");
    ok(&env, "cast --task t4 --actor actor-a --as director");

    // pause works on a running task, not twice.
    let out = ok(&env, "task pause --task t1 --reason waiting --as director");
    assert!(out.contains("task.state t1 Paused"), "{out}");
    assert!(
        refused(&env, "task pause --task t1 --reason again --as director")
            .contains("cannot record 'paused'")
    );
    // resume only works from paused, and restores the old status.
    assert!(
        refused(&env, "task resume --task t2 --reason go --as director")
            .contains("cannot record 'resumed'")
    );
    ok(&env, "task resume --task t1 --reason go --as director");
    let json = ok(&env, "status --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["tasks"]["t1"]["status"], "running");

    // abandon works once; the second is a terminal refusal.
    let out = ok(
        &env,
        "task abandon --task t2 --reason rescoped --as producer",
    );
    assert!(out.contains("task.state t2 Abandoned"), "{out}");
    assert!(
        refused(&env, "task abandon --task t2 --reason again --as producer")
            .contains("finished tasks take no events")
    );

    // takeover works on open tasks, not on terminal ones.
    let out = ok(
        &env,
        "task takeover --task t3 --reason director-finishes --as director",
    );
    assert!(out.contains("task.state t3 TakenOver"), "{out}");
    let rep = fixture(&scratch, "r.md", "report\n");
    ok(
        &env,
        &format!("deliver --commits a..b --report {rep} --as actor --task t4"),
    );
    ok(
        &env,
        "review --task t4 --delivery 1 --verdict accepted --conclusion done --as director",
    );
    assert!(
        refused(&env, "task takeover --task t4 --reason late --as director")
            .contains("finished tasks take no events")
    );

    // --reason is required, and --task comes from the caller.
    assert!(usage(&env, "task pause --task t1 --as director").contains("missing --reason"));
    assert!(usage(&env, "task pause --reason r --as director").contains("missing task"));
}

/// At the rework cap the producer accepts (possibly partially) or
/// recasts; another changes-requested is refused.
#[test]
fn review_at_cap_accepts_or_recasts() {
    let (scratch, env) = scratch();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
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
    ok(&env, "cast --task t1 --actor actor-a --as director");
    // cap = 2: three changes-requested land at_cap.
    let rep = fixture(&scratch, "r.md", "report\n");
    for d in 1..=3 {
        ok(
            &env,
            &format!("deliver --commits a..b --report {rep} --as actor --task t1"),
        );
        ok(
            &env,
            &format!(
                "review --task t1 --delivery {d} --verdict changes-requested --conclusion x --as director"
            ),
        );
    }
    let json = ok(&env, "status --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["tasks"]["t1"]["status"], "at_cap");

    // changes-requested at the cap is refused; accept closes it.
    let m = refused(
        &env,
        "review --task t1 --delivery 3 --verdict changes-requested --conclusion x --as director",
    );
    assert!(m.contains("rework cap"), "{m}");
    let out = ok(
        &env,
        "review --task t1 --delivery 3 --verdict partially-accepted --conclusion known-gaps --as producer",
    );
    assert!(out.contains("task.reviewed t1 d3"), "{out}");
    let json = ok(&env, "status --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["tasks"]["t1"]["status"], "partially_accepted");
}
