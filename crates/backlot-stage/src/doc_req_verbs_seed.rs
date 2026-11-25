//! Shared fixtures for `doc_req_verbs_tests`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use backlot_model::event::{ActorIdentity, EventKind, StudioMode};
use backlot_model::hollywood::{
    CardAction, CardBase, HollywoodKind, NewHollywoodEvent, RoleMotion as HRMotion,
};
use backlot_model::{EventBy, NewEvent};

use crate::cli::{self, Env};
use crate::hsetup;
use crate::store::{self, HPending, Pending};
use crate::test_support::TestDir;

pub(crate) fn env_for(dir: &TestDir, fake_bin: &Path) -> Env {
    let mut vars = BTreeMap::new();
    vars.insert(
        "HOME".into(),
        dir.join("home").to_string_lossy().into_owned(),
    );
    vars.insert(
        "BACKLOT_CONFIG".into(),
        dir.join("config.toml").to_string_lossy().into_owned(),
    );
    vars.insert(
        "BACKLOT_STATE_DIR".into(),
        dir.join("state").to_string_lossy().into_owned(),
    );
    vars.insert(
        "BACKLOT_FAKE_RUNNER".into(),
        fake_bin.to_string_lossy().into_owned(),
    );
    Env { vars }
}

pub(crate) fn make_fake_runner(dir: &TestDir) -> std::path::PathBuf {
    let p = dir.join("fake-runner");
    fs::write(&p, "#!/bin/sh\necho '1.0'\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }
    p
}

pub(crate) fn fake_id(model: &str) -> ActorIdentity {
    ActorIdentity {
        runner: "fake".into(),
        runner_version: None,
        model: model.into(),
        effort: Some("medium".into()),
        permission_profile: "standard".into(),
    }
}

pub(crate) fn seed_full_world(base: &Path, env: &Env) -> (u64, u64, u64, u64) {
    let dir = store::studio_dir(base, "acme-shop");
    fs::create_dir_all(dir.join("repo")).unwrap();
    fs::write(dir.join("req-v1.md"), "# Req v1\nBuild feature.\n").unwrap();
    fs::write(dir.join("accept.md"), "# Accept\n- ok\n").unwrap();

    store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("user"),
                EventKind::StudioCreated {
                    title: "Acme Shop".into(),
                    repo: dir.join("repo").to_string_lossy().into_owned(),
                    members: Vec::new(),
                    no_git: false,
                    mode: StudioMode::Standard,
                    roles: Box::new(backlot_model::event::RoleConfigs {
                        producer: fake_id("model-f"),
                        director: fake_id("model-f"),
                        writer: fake_id("model-f"),
                    }),
                    candidates: vec![backlot_model::event::Candidate {
                        name: "actor-a".into(),
                        runner: "fake".into(),
                        runner_version: None,
                        model: "model-f".into(),
                        effort: Some("medium".into()),
                        permission_profile: "standard".into(),
                    }],
                    rework_cap: 3,
                    stall_limit_s: 300,
                    nudge_after_s: 60,
                    escalate_after_s: 120,
                    check_env: Vec::new(),
                    checks: Vec::new(),
                    task_budget_s: None,
                    limits: BTreeMap::new(),
                    push: Default::default(),
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();

    // producer started so `restart --person producer` succeeds cleanly.
    store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("stage"),
                EventKind::RoleStarted {
                    role: "producer".into(),
                    terminal: Some("t0".into()),
                    actor: Some(fake_id("model-f")),
                    session_id: Some("sess-p".into()),
                    reason: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();

    // writer: closed & reopened so `held = true` (`close` works cleanly).
    store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("stage"),
                EventKind::RoleStarted {
                    role: "writer".into(),
                    terminal: Some("t1".into()),
                    actor: Some(fake_id("model-f")),
                    session_id: Some("sess-w".into()),
                    reason: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("stage"),
                EventKind::RoleClosed {
                    role: "writer".into(),
                    reason: Some("finished".into()),
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("user"),
                EventKind::RoleReopen {
                    role: "writer".into(),
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();

    // director: closed with session_id so `reopen` works cleanly.
    store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("stage"),
                EventKind::RoleStarted {
                    role: "director".into(),
                    terminal: Some("t2".into()),
                    actor: Some(fake_id("model-f")),
                    session_id: Some("sess-d".into()),
                    reason: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("stage"),
                EventKind::RoleClosed {
                    role: "director".into(),
                    reason: Some("finished".into()),
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();

    // Stalled attention, req.version v1, and task t1.
    let stalled = store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("stage"),
                EventKind::AttentionRaised {
                    role: "producer".into(),
                    reason: backlot_model::event::STALLED.into(),
                    detail: "idle".into(),
                    needs_user: false,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("producer"),
                EventKind::ReqVersion {
                    version: 1,
                    file: "req/v1.md".into(),
                    summary: "v1".into(),
                },
            ),
            files: vec![store::PendingFile {
                src: dir.join("req-v1.md"),
                dest: "req/v1.md".into(),
                append: None,
            }],
        })
    })
    .unwrap();
    store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("director"),
                EventKind::TaskCreated {
                    task: "t1".into(),
                    title: "Fix bug".into(),
                    task_kind: "code".into(),
                    requirement_version: 1,
                    acceptance: "plan/t1.md".into(),
                    dependencies: Vec::new(),
                },
            ),
            files: vec![store::PendingFile {
                src: dir.join("accept.md"),
                dest: "plan/t1.md".into(),
                append: None,
            }],
        })
    })
    .unwrap();

    // [hollywood] config, live state, user tell, recast/setup/studio-new cards.
    hsetup::write_config(
        env,
        &crate::config::HollywoodCfg {
            runner: "fake".into(),
            model: "model-f".into(),
            effort: Some("medium".into()),
            key: None,
            default_cast: None,
        },
    )
    .unwrap();
    let hdir = store::hollywood_dir(base);
    store::append_hollywood(&hdir, |_, _| {
        Ok::<HPending, crate::cli::Fail>(HPending {
            event: NewHollywoodEvent::new(
                EventBy::role("stage"),
                HollywoodKind::RoleState {
                    state: HRMotion::Started,
                    terminal: Some("t-h".into()),
                    actor: Some(fake_id("model-f")),
                    session_id: Some("sess-h".into()),
                    reason: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    let user_tell = store::append_hollywood(&hdir, |_, _| {
        Ok::<HPending, crate::cli::Fail>(HPending {
            event: NewHollywoodEvent::new(
                EventBy::role("user"),
                HollywoodKind::MessageSent {
                    to: "hollywood".into(),
                    studio: None,
                    text: Some("initial queued tell".into()),
                    file: None,
                    source_seq: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    let recast_card = store::append_hollywood(&hdir, |_, _| {
        Ok::<HPending, crate::cli::Fail>(HPending {
            event: NewHollywoodEvent::new(
                EventBy::role("hollywood"),
                HollywoodKind::CardProposed {
                    action: CardAction::Recast {
                        studio: "acme-shop".into(),
                        person: "producer".into(),
                        actor: fake_id("model-alt"),
                    },
                    why: "upgrade".into(),
                    base: CardBase::RoleIdentity {
                        actor: fake_id("model-f"),
                    },
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    let setup_card = store::append_hollywood(&hdir, |_, _| {
        Ok::<HPending, crate::cli::Fail>(HPending {
            event: NewHollywoodEvent::new(
                EventBy::role("hollywood"),
                HollywoodKind::CardProposed {
                    action: CardAction::Setup {
                        runner: "fake".into(),
                        model: "model-alt".into(),
                        effort: Some("high".into()),
                        permission_profile: None,
                    },
                    why: "switch hollywood".into(),
                    base: CardBase::HollywoodIdentity {
                        actor: Some(fake_id("model-f")),
                    },
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    fs::create_dir_all(hdir.join("drafts/beta-tools")).unwrap();
    fs::write(
        hdir.join("drafts/beta-tools/cast.toml"),
        "[producer]\nrunner='fake'\nmodel='model-f'\npermission_profile='standard'\n\
         [director]\nrunner='fake'\nmodel='model-f'\npermission_profile='standard'\n\
         [writer]\nrunner='fake'\nmodel='model-f'\npermission_profile='standard'\n",
    )
    .unwrap();
    fs::write(
        hdir.join("drafts/beta-tools/brief.md"),
        "# Beta Tools\nBuild CLI helpers.\n",
    )
    .unwrap();
    let new_card = store::append_hollywood(&hdir, |_, _| {
        Ok::<HPending, crate::cli::Fail>(HPending {
            event: NewHollywoodEvent::new(
                EventBy::role("hollywood"),
                HollywoodKind::CardProposed {
                    action: CardAction::StudioNew {
                        studio: "beta-tools".into(),
                        title: "Beta Tools".into(),
                        repo: "/home/user/beta-tools".into(),
                        cast: "drafts/beta-tools/cast.toml".into(),
                        brief: "drafts/beta-tools/brief.md".into(),
                        no_git: true,
                    },
                    why: "spin up beta".into(),
                    base: CardBase::StudioAbsent,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();

    (
        stalled.seq,
        user_tell.seq,
        recast_card.seq,
        setup_card.seq.max(new_card.seq),
    )
}

pub(crate) fn cli_ok(env: &Env, args: &[&str]) -> String {
    let owned: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    cli::run(&owned, env).unwrap_or_else(|e| panic!("CLI {:?} failed: {}", args, e.message()))
}
