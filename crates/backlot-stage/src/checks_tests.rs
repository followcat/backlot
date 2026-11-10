//! Declaring stage checks in the cast file (S9b part 2, item 1):
//! `check_env` and `[[checks]]` reach `studio.created`; bad names,
//! duplicates, empty commands, zero timeouts and dangerous
//! environment names are usage errors that create nothing.

use crate::cli::{Env, Fail, run};
use crate::cli_testkit::*;
use crate::store;
use backlot_model::{Event, EventKind};

/// Cast TOML with the caller's `check_env` line and check stanzas.
/// `check_env` is a bare key: it belongs beside `mode`/`rework_cap`,
/// ahead of the first `[table]` header, or TOML files it under the
/// last `[[candidates]]` entry.
fn cast_with_checks(check_env: &str, checks: &str) -> String {
    let base = CAST_TOML.replacen("rework_cap = 2", &format!("rework_cap = 2\n{check_env}"), 1);
    format!("{base}\n{checks}\n")
}

/// `studio new` with a cast built from `cast_with_checks`; answers
/// the studio's events, or the failure message when refused.
fn new_studio(env: &Env, scratch: &Scratch, cast: &str) -> Result<Vec<Event>, String> {
    let cast_path = fixture(scratch, "cast-checks.toml", cast);
    let args: Vec<String> = [
        "studio",
        "new",
        "--title",
        "Shop",
        "--repo",
        "/home/user/acme-shop",
        "--cast",
        &cast_path,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    match run(&args, env) {
        Ok(_) => {
            let (events, _) = store::load_events(&store::studio_dir(&scratch.dir, "acme-shop"))
                .expect("load events");
            Ok(events)
        }
        Err(Fail::Usage(m) | Fail::Refused(m) | Fail::Core(m)) => Err(m),
    }
}

#[test]
fn declared_checks_reach_studio_created() {
    let (scratch, env) = scratch();
    let events = new_studio(
        &env,
        &scratch,
        &cast_with_checks(
            "check_env = [\"CARGO_HOME\", \"RUSTUP_HOME\"]",
            "[[checks]]\nname = \"unit\"\nrun = \"python3 -m unittest\"\n\n\
             [[checks]]\nname = \"smoke-test\"\nrun = \"sh smoke.sh\"\ntimeout_s = 30",
        ),
    )
    .expect("studio new");
    let [only] = events.as_slice() else {
        panic!("one event")
    };
    let EventKind::StudioCreated {
        check_env, checks, ..
    } = &only.kind
    else {
        panic!("studio.created")
    };
    assert_eq!(
        check_env,
        &["CARGO_HOME".to_string(), "RUSTUP_HOME".to_string()]
    );
    assert_eq!(checks.len(), 2);
    assert_eq!(checks[0].name, "unit");
    assert_eq!(checks[0].run, "python3 -m unittest");
    assert_eq!(checks[0].timeout_s, 600, "the default timeout is 600s");
    assert_eq!(checks[1].name, "smoke-test");
    assert_eq!(checks[1].timeout_s, 30);
}

#[test]
fn a_bad_check_name_is_a_usage_error() {
    for name in ["Unit", "has space", "dot.name", ""] {
        let (scratch, env) = scratch();
        let out = new_studio(
            &env,
            &scratch,
            &cast_with_checks(
                "",
                &format!("[[checks]]\nname = \"{name}\"\nrun = \"true\""),
            ),
        )
        .unwrap_err();
        assert!(out.contains("check"), "{out}");
        assert!(
            !store::studio_dir(&scratch.dir, "acme-shop").exists(),
            "the refused studio was still created"
        );
    }
}

#[test]
fn a_duplicate_check_name_is_a_usage_error() {
    let (scratch, env) = scratch();
    let out = new_studio(
        &env,
        &scratch,
        &cast_with_checks(
            "",
            "[[checks]]\nname = \"unit\"\nrun = \"a\"\n\n[[checks]]\nname = \"unit\"\nrun = \"b\"",
        ),
    )
    .unwrap_err();
    assert!(out.contains("unit"), "{out}");
}

#[test]
fn an_empty_run_is_a_usage_error() {
    let (scratch, env) = scratch();
    let out = new_studio(
        &env,
        &scratch,
        &cast_with_checks("", "[[checks]]\nname = \"unit\"\nrun = \"   \""),
    )
    .unwrap_err();
    assert!(out.contains("unit"), "{out}");
}

#[test]
fn a_zero_timeout_is_a_usage_error() {
    let (scratch, env) = scratch();
    let out = new_studio(
        &env,
        &scratch,
        &cast_with_checks(
            "",
            "[[checks]]\nname = \"unit\"\nrun = \"true\"\ntimeout_s = 0",
        ),
    )
    .unwrap_err();
    assert!(out.contains("timeout_s"), "{out}");
}

/// `check_env` names that would hand the stage's own identity or a
/// secret to the check are refused; the reason names the variable.
#[test]
fn a_dangerous_check_env_name_is_a_usage_error() {
    for var in [
        "BACKLOT_ROLE",
        "HC_SESSION",
        "HTTPS_PROXY",
        "api_token",
        "AWS_SECRET_KEY",
        "DB_PASSWORD",
        "client_credential",
        "PRIVATE_KEY",
    ] {
        let (scratch, env) = scratch();
        let out = new_studio(
            &env,
            &scratch,
            &cast_with_checks(
                &format!("check_env = [\"{var}\"]"),
                "[[checks]]\nname = \"unit\"\nrun = \"true\"",
            ),
        )
        .unwrap_err();
        assert!(out.contains(var), "{out}");
    }
}

/// Ordinary build variables pass through.
#[test]
fn ordinary_check_env_names_are_accepted() {
    let (scratch, env) = scratch();
    new_studio(
        &env,
        &scratch,
        &cast_with_checks(
            "check_env = [\"CARGO_HOME\", \"LANG_EXTRA\", \"CC\"]",
            "[[checks]]\nname = \"unit\"\nrun = \"true\"",
        ),
    )
    .expect("ordinary names accepted");
}

// ── Running the checks ───────────────────────────────────────────
//
// `owed` derives the next unrecorded check from the ledger;
// `start` builds the detached worktree and strips the environment;
// `finish` writes the log, appends `task.checked` and drops the
// tree when the delivery is done with it.

use std::sync::{Arc, Mutex};

use backlot_model::event::Verdict;
use backlot_model::state::{StudioState, TaskStatus};
use backlot_model::testkit as tk;
use backlot_model::{EventBy, EventKind as K, NewEvent};

use crate::checkrun::{CheckChild, PollOut};
use crate::checks;
use crate::checkwork;
use crate::workareas::GitOut;

/// A studio whose cast declares `unit` then `lint`, with t1 cast
/// and delivered (commits `sha-a..sha-b`).
fn checked_state() -> (Vec<backlot_model::Event>, StudioState) {
    let mut kind = tk::studio_kind();
    let K::StudioCreated { checks, .. } = &mut kind else {
        unreachable!()
    };
    *checks = vec![
        tk::check_decl("unit", "cargo test --lib", 600),
        tk::check_decl("lint", "cargo clippy", 60),
    ];
    let events = vec![
        tk::ev(1, "user", kind),
        tk::ev(
            2,
            "producer",
            K::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "first".into(),
            },
        ),
        tk::ev(3, "director", tk::task_created("t1", "feature", 1)),
        tk::ev(4, "director", tk::cast("t1", "actor-a", "a")),
        tk::ev(5, "actor-t1-c1", tk::delivered("t1", 1, "a", 1)),
    ];
    let state = backlot_model::state::replay(&events);
    (events, state)
}

/// A `task.checked` event for t1 delivery 1.
fn checked_ev(seq: u64, name: &str) -> backlot_model::Event {
    tk::ev(
        seq,
        "stage",
        K::TaskChecked {
            task: "t1".into(),
            delivery: 1,
            name: name.into(),
            exit_code: Some(0),
            timed_out: false,
            output: format!("checks/5-{name}.log"),
            duration_ms: 1,
        },
    )
}

/// Checks run in declared order, once each, and only while the task
/// stays `delivered` on that delivery.
#[test]
fn owed_runs_each_check_once_in_declared_order() {
    let (mut events, _) = checked_state();
    let state = backlot_model::state::replay(&events);
    let owed = checks::owed(&state).expect("unit owed");
    assert_eq!(
        (owed.task.as_str(), owed.delivery, owed.check.name.as_str()),
        ("t1", 1, "unit")
    );

    events.push(checked_ev(6, "unit"));
    let state = backlot_model::state::replay(&events);
    assert_eq!(checks::owed(&state).unwrap().check.name, "lint");

    events.push(checked_ev(7, "lint"));
    let state = backlot_model::state::replay(&events);
    assert!(checks::owed(&state).is_none(), "nothing left to run");

    // A task that left `delivered` owes nothing new, even with
    // declared checks unrecorded.
    let (mut events, _) = checked_state();
    events.push(tk::ev(
        6,
        "director",
        tk::reviewed("t1", 1, Verdict::ChangesRequested, vec![]),
    ));
    let state = backlot_model::state::replay(&events);
    assert_eq!(state.tasks["t1"].status, TaskStatus::Running);
    assert!(checks::owed(&state).is_none());
}

/// Seed a real studio dir through `store::append` so `finish`'s
/// append validates: the studio with both checks, t1 delivered as
/// `sha-a..sha-b`. Answers the delivery's event seq.
fn seed_ledger(dir: &std::path::Path) -> u64 {
    let (events, _) = checked_state();
    let mut dseq = 0;
    for e in events {
        let role = e.by.role.clone();
        let kind = e.kind;
        let ev = store::append(dir, move |_, _| {
            Ok::<_, std::convert::Infallible>(store::Pending {
                event: NewEvent::new(EventBy::role(&role), kind),
                files: Vec::new(),
            })
        })
        .unwrap();
        dseq = ev.seq;
    }
    dseq
}

/// A child that exits at once with code 0.
struct Done;
impl CheckChild for Done {
    fn poll(&mut self) -> Option<PollOut> {
        Some(PollOut {
            code: Some(0),
            output: b"clean".to_vec(),
        })
    }
    fn term(&mut self) {}
    fn kill(&mut self) {}
}

/// `start` removes a stale tree, `worktree add --detach`es the
/// delivery's end SHA, and the spawn sees the stripped env.
#[test]
fn start_builds_the_detached_tree_with_the_stripped_env() {
    let (scratch, _) = scratch();
    let dir = store::studio_dir(&scratch.dir, "acme-shop");
    std::fs::create_dir_all(&dir).unwrap();
    let dseq = seed_ledger(&dir);
    let state = store::load_state(&dir).unwrap();
    // A dead stage left its tree behind.
    let stale = checks::run_dir(&dir, dseq);
    std::fs::create_dir_all(&stale).unwrap();
    let calls = Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
    let seen = Arc::clone(&calls);
    let mut git = move |argv: &[&str], _cwd: &str| {
        seen.lock()
            .unwrap()
            .push(argv.iter().map(|s| s.to_string()).collect());
        GitOut {
            code: 0,
            stdout: String::new(),
            stderr: String::new(),
        }
    };
    let spawns = Arc::new(Mutex::new(Vec::<crate::checkrun::CheckSpawn>::new()));
    let record = Arc::clone(&spawns);
    let spawn = move |req: &crate::checkrun::CheckSpawn| -> Result<Box<dyn CheckChild>, String> {
        record.lock().unwrap().push(crate::checkrun::CheckSpawn {
            run: req.run.clone(),
            dir: req.dir.clone(),
            env: req.env.clone(),
        });
        Ok(Box::new(Done))
    };
    let env = [
        ("PATH", "/usr/bin"),
        ("BACKLOT_ROLE", "x"),
        ("HTTPS_PROXY", "http://proxy.invalid:1"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let owed = checks::owed(&state).unwrap();
    checkwork::start(&mut git, &spawn, &state, &dir, &env, &owed, 10_000).expect("start");
    let calls = calls.lock().unwrap();
    // The stale tree came down before the fresh one went up.
    assert!(
        calls
            .iter()
            .take_while(|a| !a.contains(&"add".to_string()))
            .any(|a| a.contains(&"remove".to_string())),
        "{calls:?}"
    );
    let add = calls
        .iter()
        .find(|a| a.contains(&"add".to_string()))
        .unwrap();
    assert!(add.contains(&"--detach".to_string()), "{add:?}");
    assert!(add.iter().any(|a| a == "b"), "{add:?}");
    let spawns = spawns.lock().unwrap();
    assert_eq!(spawns[0].run, "cargo test --lib");
    assert_eq!(spawns[0].dir, checks::run_dir(&dir, dseq));
    assert_eq!(
        spawns[0].env,
        [("PATH".to_string(), "/usr/bin".to_string())]
    );
}

/// `finish`: the log lands first, `task.checked` appends by
/// `stage`, and the tree stays while checks remain — then goes.
#[test]
fn finish_logs_appends_and_drops_the_tree_when_done() {
    let (scratch, _) = scratch();
    let dir = store::studio_dir(&scratch.dir, "acme-shop");
    std::fs::create_dir_all(&dir).unwrap();
    let dseq = seed_ledger(&dir);
    let state = store::load_state(&dir).unwrap();
    let calls = Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
    let seen = Arc::clone(&calls);
    let mut git = move |argv: &[&str], _cwd: &str| {
        seen.lock()
            .unwrap()
            .push(argv.iter().map(|s| s.to_string()).collect());
        GitOut {
            code: 0,
            stdout: String::new(),
            stderr: String::new(),
        }
    };
    let worktree = checks::run_dir(&dir, dseq);
    let run = checkwork::Running::for_test(Box::new(Done), 60_000, worktree.clone());
    checkwork::finish(
        &mut git,
        &dir,
        &state,
        run,
        checkwork::Outcome {
            exit_code: Some(0),
            timed_out: false,
            duration_ms: 12,
            output: b"the log\n".to_vec(),
        },
    )
    .expect("finish");
    // The log file landed ahead of the event.
    assert_eq!(
        std::fs::read(dir.join(format!("checks/{dseq}-unit.log"))).unwrap(),
        b"the log\n"
    );
    // `task.checked` is in the ledger, by stage, pointing at it.
    let (events, _) = store::load_events(&dir).unwrap();
    let last = events.last().unwrap();
    assert_eq!(last.by.role, "stage");
    let K::TaskChecked {
        task,
        delivery,
        name,
        exit_code,
        timed_out,
        output,
        duration_ms,
    } = &last.kind
    else {
        panic!("task.checked")
    };
    assert_eq!(
        (
            task.as_str(),
            *delivery,
            name.as_str(),
            *exit_code,
            *timed_out,
            output.as_str(),
            *duration_ms
        ),
        (
            "t1",
            1,
            "unit",
            Some(0),
            false,
            format!("checks/{dseq}-unit.log").as_str(),
            12
        )
    );
    // `lint` is still owed: the tree stays.
    assert!(
        !calls
            .lock()
            .unwrap()
            .iter()
            .any(|a| a.contains(&"remove".to_string())),
        "{:?}",
        calls.lock().unwrap()
    );
    // The last check finishes: the tree comes down.
    let state = store::load_state(&dir).unwrap();
    let run = checkwork::Running::for_test(Box::new(Done), 60_000, worktree.clone());
    let mut run = run;
    run.name = "lint".into();
    checkwork::finish(
        &mut git,
        &dir,
        &state,
        run,
        checkwork::Outcome {
            exit_code: Some(1),
            timed_out: false,
            duration_ms: 9,
            output: b"failed\n".to_vec(),
        },
    )
    .expect("finish");
    assert!(
        calls
            .lock()
            .unwrap()
            .iter()
            .any(|a| a.contains(&"remove".to_string())),
        "{:?}",
        calls.lock().unwrap()
    );
    let state = store::load_state(&dir).unwrap();
    assert!(checks::owed(&state).is_none());
}

// ── Results the studio shows ─────────────────────────────────────
//
// `backlot status` carries the stage's results on the task line;
// the reviewer's report copies gain the "Accepted with failing
// stage checks" section only while such a task exists.

/// `backlot status` marks each declared check on the latest
/// delivery: `running` while the record is owed, `pass` or `fail`
/// when it lands — and `-` when it never will.
#[test]
fn status_marks_each_check_on_the_task_line() {
    let (mut events, state) = checked_state();
    let line = |state: &StudioState| {
        crate::render::status("acme-shop", state, 0)
            .lines()
            .find(|l| l.trim_start().starts_with("t1"))
            .unwrap()
            .to_string()
    };
    assert!(
        line(&state).contains("checks unit:running lint:running"),
        "{}",
        line(&state)
    );
    events.push(checked_ev(6, "unit"));
    let state = backlot_model::state::replay(&events);
    assert!(
        line(&state).contains("checks unit:pass lint:running"),
        "{}",
        line(&state)
    );
    let mut lint_fail = checked_ev(7, "lint");
    let K::TaskChecked { exit_code, .. } = &mut lint_fail.kind else {
        unreachable!()
    };
    *exit_code = Some(1);
    events.push(lint_fail);
    let state = backlot_model::state::replay(&events);
    assert!(
        line(&state).contains("checks unit:pass lint:fail"),
        "{}",
        line(&state)
    );
}

/// `failing_section` lists each accepted task's failing checks and
/// the review's conclusion — nothing while no such task exists.
#[test]
fn the_failing_checks_section_lists_what_the_review_accepted_over() {
    let (mut events, _) = checked_state();
    // unit failed, lint passed; the review accepted naming unit.
    let mut unit_fail = checked_ev(6, "unit");
    let K::TaskChecked { exit_code, .. } = &mut unit_fail.kind else {
        unreachable!()
    };
    *exit_code = Some(1);
    events.push(unit_fail);
    events.push(checked_ev(7, "lint"));
    let mut accepted = tk::reviewed("t1", 1, Verdict::Accepted, vec![]);
    let K::TaskReviewed { conclusion, .. } = &mut accepted else {
        unreachable!()
    };
    *conclusion = "unit failed, accepted anyway".into();
    events.push(tk::ev(8, "director", accepted));
    let state = backlot_model::state::replay(&events);
    let section = checks::failing_section(&state).expect("a section");
    assert!(
        section.contains("## Accepted with failing stage checks"),
        "{section}"
    );
    assert!(
        section.contains("t1 delivery 1: unit — unit failed, accepted anyway"),
        "{section}"
    );
    // No accepted-with-failures task: no section.
    let (events, _) = checked_state();
    let state = backlot_model::state::replay(&events);
    assert!(checks::failing_section(&state).is_none());
}

/// `store::append` writes a `PendingFile.append` after the copy —
/// the stage's annotation lands inside the ledger's file.
#[test]
fn a_pending_append_lands_after_the_copied_file() {
    let (scratch, _) = scratch();
    let dir = store::studio_dir(&scratch.dir, "acme-shop");
    std::fs::create_dir_all(&dir).unwrap();
    seed_ledger(&dir);
    let src = scratch.files.join("r.md");
    std::fs::write(&src, "the report body\n").unwrap();
    store::append(&dir, |_, seq| {
        Ok::<_, std::convert::Infallible>(store::Pending {
            event: NewEvent::new(
                EventBy::role("director"),
                K::ReportSent {
                    to: "producer".into(),
                    node: "integration".into(),
                    file: format!("reports/{seq}-r.md"),
                },
            ),
            files: vec![store::PendingFile {
                src: src.clone(),
                dest: format!("reports/{seq}-r.md"),
                append: Some("\n\n## Accepted with failing stage checks\n\n- t1 delivery 1: unit — it failed\n".into()),
            }],
        })
    })
    .unwrap();
    let copy = std::fs::read_to_string(dir.join("reports/6-r.md")).unwrap();
    assert!(copy.starts_with("the report body\n"), "{copy}");
    assert!(
        copy.ends_with("- t1 delivery 1: unit — it failed\n"),
        "{copy}"
    );
}

/// End to end: the delivery's own report lands before the verdict —
/// no section yet — while the node report that follows an
/// accepted-with-failures task carries it.
#[test]
fn a_report_written_after_an_accept_over_failures_carries_the_section() {
    let (scratch, env) = scratch();
    new_studio(
        &env,
        &scratch,
        &cast_with_checks("", "[[checks]]\nname = \"unit\"\nrun = \"true\""),
    )
    .unwrap();
    let req = fixture(&scratch, "req.md", "the ask\n");
    let acc = fixture(&scratch, "acc.md", "acceptance\n");
    ok(
        &env,
        &format!("req revise --file {req} --summary first --as producer"),
    );
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
    let rep = fixture(&scratch, "d1.md", "delivery report\n");
    ok(
        &env,
        &format!(
            "deliver --commits {a}..{b} --report {rep} --check unit=pass --as actor-t1-c1 --task t1"
        ),
    );
    // The stage records unit as a failure; the director accepts
    // naming it.
    let dir = store::studio_dir(&scratch.dir, "acme-shop");
    store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(store::Pending {
            event: NewEvent::new(
                EventBy::role("stage"),
                K::TaskChecked {
                    task: "t1".into(),
                    delivery: 1,
                    name: "unit".into(),
                    exit_code: Some(1),
                    timed_out: false,
                    output: "checks/5-unit.log".into(),
                    duration_ms: 10,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    ok(
        &env,
        "review --task t1 --delivery 1 --verdict accepted --conclusion unit_failed_accepted --as director",
    );
    let report = fixture(&scratch, "node.md", "node report\n");
    ok(
        &env,
        &format!("report --node integration --file {report} --as director"),
    );
    // The delivery's own copy predates the verdict: no section.
    let delivery_copy = std::fs::read_to_string(
        dir.join("reports")
            .read_dir()
            .unwrap()
            .filter_map(|e| e.ok())
            .find(|e| e.file_name().to_string_lossy().contains("t1-d1"))
            .unwrap()
            .path(),
    )
    .unwrap();
    assert!(
        !delivery_copy.contains("failing stage checks"),
        "{delivery_copy}"
    );
    // The node report's copy carries it: task, delivery, names,
    // conclusion.
    let node_copy = std::fs::read_to_string(
        dir.join("reports")
            .read_dir()
            .unwrap()
            .filter_map(|e| e.ok())
            .find(|e| e.file_name().to_string_lossy().contains("integration"))
            .unwrap()
            .path(),
    )
    .unwrap();
    assert!(
        node_copy.contains("## Accepted with failing stage checks"),
        "{node_copy}"
    );
    assert!(
        node_copy.contains("t1 delivery 1: unit — unit_failed_accepted"),
        "{node_copy}"
    );
}
