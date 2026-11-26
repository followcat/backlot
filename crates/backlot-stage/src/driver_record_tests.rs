//! Fail-first proof of the seam: the same stage actions issue the same
//! driver calls, in the same order, as today's direct client and git
//! calls. A [`recorder::RecordingDriver`] wraps a
//! [`crate::driver::LocalDriver`], forwards each call to it — so
//! behaviour is unchanged — and appends the call's name to a shared log.
//! Each test drives one real stage action against a scratch studio and
//! asserts the exact sequence.
//!
//! The sequences are the contract a later `HcRemoteDriver` must keep:
//! the order the stage makes the calls in, not the wire's spelling.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use backlot_model::event::EventKind;
use backlot_model::state::StudioState;
use backlot_model::{EventBy, NewEvent};

use crate::daemon::Stage;
use crate::reconcile::LiveTerminal;
use crate::test_support::TestDir;
use recorder::{RecordingDriver, client_on_fake_core};

/// The calls one test recorded, in order.
type Log = Arc<Mutex<Vec<String>>>;

// ---------------------------------------------------------------------
// Scratch studios.
// ---------------------------------------------------------------------

fn tmp(name: &str) -> TestDir {
    TestDir::new(&format!("backlot-driver-{name}"))
}

fn studio_dir(base: &Path, name: &str) -> PathBuf {
    crate::store::studio_dir(base, name)
}

/// A `fake` runner so `plan` answers a launch instead of an
/// unsupported-runner attention.
fn actor() -> backlot_model::event::ActorIdentity {
    backlot_model::event::ActorIdentity {
        runner: "fake".into(),
        runner_version: None,
        model: "m".into(),
        effort: Some("high".into()),
        permission_profile: "standard".into(),
    }
}

/// A studio: one task, a cast, and optionally a declared stage check.
/// `no_git` keeps a test off a real repository; the git-bound shape is
/// what `range` and `prune` need.
fn seed(dir: &Path, with_check: bool) {
    seed_with(dir, with_check, true);
}

fn seed_with(dir: &Path, with_check: bool, no_git: bool) {
    std::fs::create_dir_all(dir.join("repo")).unwrap();
    let repo = dir.join("repo").to_string_lossy().into_owned();
    let mut created = EventKind::StudioCreated {
        title: "acme".into(),
        repo: repo.clone(),
        members: Vec::new(),
        no_git,
        mode: backlot_model::event::StudioMode::Standard,
        roles: Box::new(backlot_model::event::RoleConfigs {
            producer: actor(),
            director: actor(),
            writer: actor(),
        }),
        candidates: vec![backlot_model::event::Candidate {
            name: "actor-a".into(),
            runner: actor().runner,
            runner_version: None,
            model: actor().model,
            effort: actor().effort,
            permission_profile: actor().permission_profile,
        }],
        rework_cap: 2,
        stall_limit_s: 600,
        nudge_after_s: 180,
        escalate_after_s: 600,
        check_env: Vec::new(),
        checks: Vec::new(),
        task_budget_s: None,
        limits: Default::default(),
        max_actors: backlot_model::event::default_max_actors(),
        push: Default::default(),
        design_gate: backlot_model::event::DesignGate::Off,
    };
    if with_check {
        let EventKind::StudioCreated { checks, .. } = &mut created else {
            unreachable!()
        };
        *checks = vec![backlot_model::testkit::check_decl("unit", "true", 60)];
    }
    for (role, kind) in [
        ("user", created),
        (
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "first".into(),
            },
        ),
        (
            "director",
            backlot_model::testkit::task_created("t1", "feature", 1),
        ),
        (
            "director",
            EventKind::TaskCast {
                task: "t1".into(),
                candidate: "actor-a".into(),
                actor: actor(),
                reason: "picked".into(),
                priority: None,
            },
        ),
        (
            "actor-t1-c1",
            backlot_model::testkit::delivered("t1", 1, "a", 1),
        ),
    ] {
        crate::store::append(dir, |_, _| {
            Ok::<_, std::convert::Infallible>(crate::store::Pending {
                event: NewEvent::new(EventBy::role(role), kind),
                files: Vec::new(),
            })
        })
        .unwrap();
    }
}

fn state_of(dir: &Path) -> StudioState {
    crate::store::load_state(dir).unwrap()
}

/// A stage on the fake core whose driver is the recorder; every stage
/// field the action needs is filled in.
async fn recorded_stage(base: &Path) -> (Stage, Log) {
    let client = client_on_fake_core().await;
    let mut stage = Stage::for_test(client);
    let (driver, log) = RecordingDriver::new(stage.git.clone());
    stage.driver = driver;
    stage.base = base.to_path_buf();
    stage.exe_dir = base.join("exe").to_string_lossy().into_owned();
    stage.path_env = "/usr/bin".into();
    stage.state_dir = base.to_string_lossy().into_owned();
    (stage, log)
}

fn calls(log: &Log) -> Vec<String> {
    log.lock().unwrap().clone()
}

fn nothing_blocked() -> std::collections::BTreeSet<(String, String)> {
    std::collections::BTreeSet::new()
}

/// A live terminal record for a role, its cwd the studio's repo.
fn live_terminal(name: &str, address: &str, dir: &Path) -> LiveTerminal {
    LiveTerminal {
        name: name.into(),
        address: address.into(),
        argv: Vec::new(),
        cwd: dir.join("repo").to_string_lossy().into_owned(),
        labels: BTreeMap::new(),
        status: None,
        waiting_for_person: false,
        activity_revision: None,
        pid: None,
    }
}

#[path = "driver_record/git.rs"]
mod git_tests;
#[path = "driver_record/recorder.rs"]
mod recorder;
#[path = "driver_record/terminal.rs"]
mod terminal_tests;
