//! The git-shaped calls: a work-area `ensure`, a check run, a
//! `deliver` range resolve and a prune — each driven through the
//! driver's git runner, so a remote `machine.exec` fits behind them.

use super::*;
use crate::driver::{LocalDriver, MachineDriver};

#[test]
fn ensure_issues_its_git_through_the_driver() {
    let root = tmp("ensure");
    let dir = root.join("state");
    std::fs::create_dir_all(&dir).unwrap();
    let git = crate::driver::shared_git();
    let recorded: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&recorded);
    *git.borrow_mut() = Box::new(move |argv: &[&str], _cwd: &str| {
        seen.lock().unwrap().push(argv.join(" "));
        crate::workareas::GitOut {
            code: 0,
            stdout: "true\n".into(),
            stderr: String::new(),
        }
    });
    let driver = LocalDriver::with_git(git);
    let repo = "/home/user/repo";
    let area = crate::workareas::Area::Integration;
    {
        let mut git_ref = driver.git();
        crate::workareas::ensure(&mut **git_ref, "acme", repo, &[], false, &dir, &area).unwrap();
    }
    let seen = recorded.lock().unwrap();
    assert!(
        seen.iter()
            .any(|a| a == "-C /home/user/repo rev-parse --is-inside-work-tree"),
        "the work-tree probe went through the driver's git: {seen:?}"
    );
    assert!(
        seen.iter().any(|a| a.contains("worktree list --porcelain")),
        "the worktree listing went through the driver's git: {seen:?}"
    );
}

/// The check pass's `start` builds its worktree through the driver's
/// git, then spawns the process through the same injected spawner.
#[tokio::test]
async fn a_check_run_issues_its_git_through_the_driver() {
    let root = tmp("check");
    let dir = studio_dir(&root, "acme");
    seed(&dir, true);
    let state = state_of(&dir);
    let (mut stage, _log) = recorded_stage(&root).await;

    // The delivery's end sha is `b`; let `worktree add` and the rest
    // succeed, and record every argv the driver's git ran.
    let recorded: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&recorded);
    *stage.git.borrow_mut() = Box::new(move |argv: &[&str], _cwd: &str| {
        seen.lock().unwrap().push(argv.join(" "));
        crate::workareas::GitOut {
            code: 0,
            stdout: String::new(),
            stderr: String::new(),
        }
    });
    struct Done;
    impl crate::checkrun::CheckChild for Done {
        fn poll(&mut self) -> Option<crate::checkrun::PollOut> {
            Some(crate::checkrun::PollOut {
                code: Some(0),
                output: Vec::new(),
            })
        }
        fn term(&mut self) {}
        fn kill(&mut self) {}
    }
    stage.spawn_check = Box::new(|_| Ok(Box::new(Done)));

    stage.check("acme", &dir, &state, 10_000).unwrap();
    let seen = recorded.lock().unwrap();
    assert!(
        seen.iter().any(|a| a.contains(" worktree add ")),
        "the check's worktree came up through the driver's git: {seen:?}"
    );
    drop(seen);
    // The finished run is recorded as `task.checked` on the next pass.
    stage.checking.borrow_mut().get_mut("acme");
    stage.check("acme", &dir, &state, 10_001).unwrap();
    let after = state_of(&dir);
    assert_eq!(after.tasks["t1"].checked.len(), 1, "the check was recorded");
}

/// A `deliver` range resolve drives `rev-parse` and `merge-base`
/// through the driver's git.
#[test]
fn a_deliver_range_resolves_through_the_driver() {
    let root = tmp("range");
    let dir = studio_dir(&root, "acme");
    seed_with(&dir, false, false);
    std::fs::create_dir_all(dir.join("work").join("t1")).unwrap();
    let git = crate::driver::shared_git();
    let recorded: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&recorded);
    *git.borrow_mut() = Box::new(move |argv: &[&str], _cwd: &str| {
        seen.lock().unwrap().push(argv.join(" "));
        // `rev-parse` answers a sha. `merge-base --is-ancestor` says
        // "no" for the range's own end (a non-empty range) and "yes"
        // when the second end is the task branch, so the resolve
        // continues past both checks.
        let ancestor = argv.first() == Some(&"merge-base");
        let on_branch = argv.last().is_some_and(|b| b.starts_with("backlot/"));
        let code = match (ancestor, on_branch) {
            (true, false) => 1,
            _ => 0,
        };
        crate::workareas::GitOut {
            code,
            stdout: "sha-x\n".into(),
            stderr: String::new(),
        }
    });
    let driver = LocalDriver::with_git(git);
    let state = state_of(&dir);
    let resolved =
        crate::range::check_range_with(&driver, &dir, "acme", "t1", &state, "main..HEAD").unwrap();
    assert_eq!(resolved, "sha-x..sha-x");
    let seen = recorded.lock().unwrap();
    assert!(
        seen.iter().filter(|a| a.starts_with("rev-parse")).count() >= 2,
        "both ends resolve through the driver's git: {seen:?}"
    );
    assert!(
        seen.iter()
            .any(|a| a.starts_with("merge-base --is-ancestor")),
        "the branch check runs through the driver's git: {seen:?}"
    );
}

/// A prune drives its `worktree list`, `status` and `branch` calls
/// through the driver's git.
#[test]
fn a_prune_issues_its_git_through_the_driver() {
    let root = tmp("prune");
    let dir = studio_dir(&root, "acme");
    seed_with(&dir, false, false);
    // A finished studio has no open task: abandon `t1`, then finish.
    for kind in [
        backlot_model::testkit::task_state(
            "t1",
            backlot_model::event::TaskStateChange::Abandoned,
            "done",
        ),
        EventKind::StudioFinished {
            reason: "done".into(),
        },
    ] {
        crate::store::append(&dir, |_, _| {
            Ok::<_, std::convert::Infallible>(crate::store::Pending {
                event: NewEvent::new(EventBy::role("user"), kind),
                files: Vec::new(),
            })
        })
        .unwrap();
    }

    let git = crate::driver::shared_git();
    let recorded: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&recorded);
    *git.borrow_mut() = Box::new(move |argv: &[&str], _cwd: &str| {
        seen.lock().unwrap().push(argv.join(" "));
        crate::workareas::GitOut {
            code: 0,
            // A work tree, so the binding probe passes.
            stdout: "true\n".into(),
            stderr: String::new(),
        }
    });
    let driver = LocalDriver::with_git(git);
    crate::prune::run_with(&driver, &root, Some("acme".into()), false, true, false).unwrap();
    let seen = recorded.lock().unwrap();
    assert!(
        seen.iter()
            .any(|a| a.contains("rev-parse --is-inside-work-tree")),
        "the binding probe ran: {seen:?}"
    );
    assert!(
        seen.iter().any(|a| a.contains("worktree list --porcelain")),
        "the worktrees were listed: {seen:?}"
    );
}
