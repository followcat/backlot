//! What a check leaves behind (S9b part 2, fix round): an exiting
//! shell's group is KILL'd so a `server &` dies with it, dropping
//! a `Running` kills the group, and a leftover `checks/run-*` tree
//! loses every process parked inside before it is reused or
//! removed — including one whose cwd the delete already took.
//! The first pass prunes the trees no owed check needs.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use backlot_model::event::Verdict;
use backlot_model::state::StudioState;
use backlot_model::testkit as tk;
use backlot_model::{Event, EventBy, EventKind as K, NewEvent};

use crate::checkrun::{self, CheckChild, CheckSpawn, PollOut};
use crate::checks;
use crate::checkwork::{self, Running};
use crate::cli_testkit::*;
use crate::store;
use crate::workareas::GitOut;

/// A studio whose cast declares `unit`; t1 is cast and delivered
/// (delivery event seq 5, commits `a..b`).
fn checked_state() -> (Vec<Event>, StudioState) {
    let mut kind = tk::studio_kind();
    let K::StudioCreated { checks, .. } = &mut kind else {
        unreachable!()
    };
    *checks = vec![tk::check_decl("unit", "sleep 300 & exit 0", 600)];
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

/// The events of `checked_state` appended to a real studio dir.
fn seed_ledger(dir: &Path) {
    let (events, _) = checked_state();
    for e in events {
        let role = e.by.role.clone();
        let kind = e.kind;
        store::append(dir, move |_, _| {
            Ok::<_, std::convert::Infallible>(store::Pending {
                event: NewEvent::new(EventBy::role(&role), kind),
                files: Vec::new(),
            })
        })
        .unwrap();
    }
}

/// Recorded git calls, scripted per-subcommand exit codes.
fn fake_git(
    calls: Arc<Mutex<Vec<Vec<String>>>>,
    codes: &'static [(&'static str, i32)],
) -> impl FnMut(&[&str], &str) -> GitOut {
    move |argv, _cwd| {
        calls
            .lock()
            .unwrap()
            .push(argv.iter().map(|s| s.to_string()).collect());
        let code = codes
            .iter()
            .find(|(sub, _)| argv.contains(sub))
            .map(|(_, c)| *c)
            .unwrap_or(0);
        GitOut {
            code,
            stdout: String::new(),
            stderr: "git declined".into(),
        }
    }
}

/// `sleep N` parked in `dir` with no inherited pipes.
fn sleeper(dir: &Path, secs: u64) -> std::process::Child {
    std::process::Command::new("sleep")
        .arg(secs.to_string())
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn sleep")
}

/// Until `f` holds or two seconds pass — SIGKILL delivery is
/// prompt but not synchronous with the caller's next line.
fn until(mut f: impl FnMut() -> bool) -> bool {
    for _ in 0..200 {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

/// The signal a killed child reports (unix), so the assertions
/// name KILL rather than any nonzero exit.
#[cfg(unix)]
fn sig(child: &mut std::process::Child) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    until(|| child.try_wait().unwrap().is_some())
        .then(|| child.wait().unwrap().signal())
        .flatten()
}

fn env_path() -> Vec<(String, String)> {
    [("PATH".to_string(), "/usr/bin:/bin".to_string())]
        .into_iter()
        .collect()
}

fn stage_env() -> BTreeMap<String, String> {
    env_path().into_iter().collect()
}

/// A scripted child that exits 0 at once.
struct Done;
impl CheckChild for Done {
    fn poll(&mut self) -> Option<PollOut> {
        Some(PollOut {
            code: Some(0),
            output: Vec::new(),
        })
    }
    fn term(&mut self) {}
    fn kill(&mut self) {}
}

/// `sleep 300 & exit 0`: the shell exits at once and the run
/// records 0 — but the backgrounded sleep dies with the group.
#[test]
fn a_backgrounded_check_process_dies_with_the_shell() {
    let (scratch, _) = scratch();
    let work = scratch.dir.join("checks").join("run-5");
    fs::create_dir_all(&work).unwrap();
    let child = checkrun::real_spawn(&CheckSpawn {
        run: "sleep 300 & exit 0".into(),
        dir: work.clone(),
        env: env_path(),
    })
    .expect("spawn");
    let mut run = Running::for_test(child, 60_000, work.clone());
    let mut out = None;
    for _ in 0..200 {
        if let Some(o) = run.poll(1_000) {
            out = Some(o);
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(out.expect("the run finished").exit_code, Some(0));
    assert!(
        until(|| checkwork::cwd_pids(&work).is_empty()),
        "a leftover process still lives in {work:?}"
    );
}

/// The stage stops mid-check: dropping the `Running` KILLs the
/// whole group, so nothing keeps running in its tree.
#[test]
fn a_dropped_run_kills_its_group() {
    let (scratch, _) = scratch();
    let work = scratch.dir.join("checks").join("run-5");
    fs::create_dir_all(&work).unwrap();
    let child = checkrun::real_spawn(&CheckSpawn {
        run: "sleep 300".into(),
        dir: work.clone(),
        env: env_path(),
    })
    .expect("spawn");
    let run = Running::for_test(child, 60_000, work.clone());
    // Let the shell start its sleep, then the stage dies.
    std::thread::sleep(Duration::from_millis(100));
    drop(run);
    assert!(
        until(|| checkwork::cwd_pids(&work).is_empty()),
        "a dropped run left a process in {work:?}"
    );
}

/// A dead stage left both its tree and a live process in it:
/// `start` kills the process before the tree comes down for the
/// fresh run.
#[test]
fn a_leftover_tree_loses_its_processes_before_the_rerun() {
    let (scratch, _) = scratch();
    let dir = store::studio_dir(&scratch.dir, "acme-shop");
    fs::create_dir_all(&dir).unwrap();
    seed_ledger(&dir);
    let state = store::load_state(&dir).unwrap();
    let work = checks::run_dir(&dir, 5);
    fs::create_dir_all(&work).unwrap();
    let mut sleeper = sleeper(&work, 300);
    assert!(
        until(|| !checkwork::cwd_pids(&work).is_empty()),
        "the sleeper never parked inside"
    );
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut git = fake_git(Arc::clone(&calls), &[]);
    let spawn = |_req: &CheckSpawn| -> Result<Box<dyn CheckChild>, String> { Ok(Box::new(Done)) };
    let owed = checks::owed(&state).unwrap();
    checkwork::start(&mut git, &spawn, &state, &dir, &stage_env(), &owed, 10_000).expect("start");
    assert_eq!(sig(&mut sleeper), Some(libc::SIGKILL));
}

/// A process parked in a tree the delete already took shows its
/// cwd as `<path> (deleted)` — the sweep kills it too.
#[cfg(target_os = "linux")]
#[test]
fn a_process_in_a_deleted_tree_still_dies() {
    let (scratch, _) = scratch();
    let work = scratch.dir.join("checks").join("run-9");
    fs::create_dir_all(&work).unwrap();
    let mut sleeper = sleeper(&work, 300);
    assert!(
        until(|| !checkwork::cwd_pids(&work).is_empty()),
        "the sleeper never parked inside"
    );
    fs::remove_dir_all(&work).unwrap();
    assert!(
        until(|| !checkwork::cwd_pids(&work).is_empty()),
        "the deleted cwd should still match the tree's path"
    );
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut git = fake_git(Arc::clone(&calls), &[("remove", 1)]);
    checkwork::remove_tree(&mut git, "", &work);
    assert_eq!(sig(&mut sleeper), Some(libc::SIGKILL));
}

/// The first pass prunes every `run-*` tree no owed check needs —
/// the stale one goes, the owed one stays for `start`.
#[test]
fn the_first_pass_prunes_only_unneeded_run_trees() {
    let (scratch, _) = scratch();
    let dir = store::studio_dir(&scratch.dir, "acme-shop");
    fs::create_dir_all(&dir).unwrap();
    seed_ledger(&dir);
    let state = store::load_state(&dir).unwrap();
    let owed_run = checks::run_dir(&dir, 5);
    let stale = checks::run_dir(&dir, 99);
    fs::create_dir_all(&owed_run).unwrap();
    fs::create_dir_all(&stale).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut git = fake_git(Arc::clone(&calls), &[("remove", 1)]);
    checkwork::prune_run_trees(&mut git, &dir, &state);
    assert!(owed_run.exists(), "the owed run tree was pruned");
    assert!(!stale.exists(), "the stale run tree survived");

    // A delivery whose task moved on owes nothing: its tree goes.
    let (mut events, _) = checked_state();
    events.push(tk::ev(
        6,
        "director",
        tk::reviewed("t1", 1, Verdict::ChangesRequested, vec![]),
    ));
    let state = backlot_model::state::replay(&events);
    fs::create_dir_all(&stale).unwrap();
    let mut git = fake_git(Arc::clone(&calls), &[("remove", 1)]);
    checkwork::prune_run_trees(&mut git, &dir, &state);
    assert!(!owed_run.exists(), "a run tree for a moved-on task stayed");
}
