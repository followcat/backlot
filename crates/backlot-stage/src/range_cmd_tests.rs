//! S9b part 1, item 2 — `deliver --commits A..B` end to end through
//! the CLI on a scratch state dir: the range resolves in `work/<task>`,
//! its tip must sit on `backlot/<studio>/<task>`, and the ledger
//! records the resolved `sha..sha`.

use backlot_model::event::EventKind;

use crate::cli::Env;
use crate::cli_testkit::*;
use crate::store::{self, studio_dir};

fn studio(scratch: &Scratch, env: &Env) {
    let cast = fixture(scratch, "cast.toml", CAST_TOML);
    ok(
        env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let req = fixture(scratch, "req.md", "# req\n");
    ok(
        env,
        &format!("req revise --file {req} --summary s --as producer"),
    );
    let acc = fixture(scratch, "acc.md", "# acc\n");
    ok(
        env,
        &format!("task new --title t1 --kind feature --acceptance-file {acc} --as director"),
    );
    ok(env, "cast --task t1 --actor actor-a --as director");
}

fn commits(env: &Env) -> String {
    let json = ok(env, "status --json");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    v["tasks"]["t1"]["deliveries"][0]["commits"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Short shas in, full `sha..sha` recorded: both ends resolve in the
/// task's work area and the tip is checked against the task branch.
#[test]
fn the_delivered_range_is_resolved_and_recorded() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    let work = work_area(&scratch, "t1");
    let a = commit_in(&work, "one");
    let b = commit_in(&work, "two");
    let rep = fixture(&scratch, "r.md", "report\n");
    ok(
        &env,
        &format!(
            "deliver --commits {}..{} --report {rep} --as actor-t1-c1 --task t1",
            &a[..7],
            &b[..7]
        ),
    );
    let recorded = commits(&env);
    assert_eq!(recorded, format!("{a}..{b}"));
    assert_eq!(recorded.len(), 82, "{recorded}");
    assert!(
        recorded.chars().all(|c| c.is_ascii_hexdigit() || c == '.'),
        "{recorded}"
    );
}

/// An empty range delivers nothing: `B` reachable from `A` refuses,
/// naming the check.
#[test]
fn an_empty_range_is_refused() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    let work = work_area(&scratch, "t1");
    let a = commit_in(&work, "one");
    let rep = fixture(&scratch, "r.md", "report\n");
    for range in [format!("{a}..{a}"), format!("{a}..{a}~0")] {
        let msg = refused(
            &env,
            &format!("deliver --commits {range} --report {rep} --as actor-t1-c1 --task t1"),
        );
        assert!(msg.contains("empty range"), "{range}: {msg}");
    }
}

/// Work committed on a side branch is not on `backlot/<studio>/<task>`
/// — the delivery refuses, naming branch and check.
#[test]
fn a_tip_off_the_task_branch_is_refused() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    let work = work_area(&scratch, "t1");
    let a = commit_in(&work, "one");
    git(&work, &["checkout", "-b", "side"]);
    let b = commit_in(&work, "side work");
    let rep = fixture(&scratch, "r.md", "report\n");
    let msg = refused(
        &env,
        &format!("deliver --commits {a}..{b} --report {rep} --as actor-t1-c1 --task t1"),
    );
    assert!(msg.contains("not on branch"), "{msg}");
    assert!(msg.contains("backlot/acme-shop/t1"), "{msg}");
}

/// A rev that names nothing in the work area says which end failed.
#[test]
fn an_unresolvable_end_is_named() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    let work = work_area(&scratch, "t1");
    let a = commit_in(&work, "one");
    let rep = fixture(&scratch, "r.md", "report\n");
    let msg = refused(
        &env,
        &format!("deliver --commits {a}..nosuch --report {rep} --as actor-t1-c1 --task t1"),
    );
    assert!(msg.contains("cannot resolve 'nosuch'"), "{msg}");
}

/// No `work/<task>` — the actor's terminal never ran — is its own
/// refusal, ahead of any git call.
#[test]
fn a_missing_work_area_is_refused() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    let rep = fixture(&scratch, "r.md", "report\n");
    let msg = refused(
        &env,
        &format!("deliver --commits a..b --report {rep} --as actor-t1-c1 --task t1"),
    );
    assert!(msg.contains("no work area"), "{msg}");
    assert!(msg.contains("work/t1"), "{msg}");
}

/// The appended event itself carries the resolved range — replaying
/// the log gives the same `sha..sha` `status` showed.
#[test]
fn the_event_records_the_resolved_form() {
    let (scratch, env) = scratch();
    studio(&scratch, &env);
    let work = work_area(&scratch, "t1");
    let a = commit_in(&work, "one");
    let b = commit_in(&work, "two");
    let rep = fixture(&scratch, "r.md", "report\n");
    ok(
        &env,
        &format!("deliver --commits {a}..{b} --report {rep} --as actor-t1-c1 --task t1"),
    );
    let (events, _) = store::load_events(&studio_dir(&scratch.dir, "acme-shop")).unwrap();
    let delivered = events
        .iter()
        .find(|e| matches!(e.kind, EventKind::TaskDelivered { .. }))
        .unwrap();
    match &delivered.kind {
        EventKind::TaskDelivered { commits, .. } => {
            assert_eq!(*commits, format!("{a}..{b}"), "{commits}")
        }
        other => panic!("{other:?}"),
    }
}
