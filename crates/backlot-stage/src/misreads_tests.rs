//! Fail-first regression tests for four screen misreads captured from real sessions:
//! 1. agy startup `Signing in...` screen misread as `permission prompt`
//! 2. agy producer screen with user's unsent draft misread as `question`
//! 3. cmd `Tool Permission` outside-workspace dialog misread as `question`
//! 4. cmd `Create File` permission dialog misread as `question`

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use backlot_model::event::{
    ActorIdentity, ApprovalDecision, EventKind, PERMISSION_PROMPT, QUESTION,
};
use backlot_model::summary::Work;
use backlot_model::{EventBy, NewEvent};
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use hc_proto::frame::{Frame, Kind};
use hc_proto::terminal::Status;
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, DuplexStream};

use crate::daemon::Stage;
use crate::handoff::DEFAULT_TIMEOUT_MS;
use crate::permit::{self, approval_keys};
use crate::question;
use crate::reconcile::LiveTerminal;
use crate::runners::work::recognized_permission;
use crate::runners::{self, dialog};
use crate::sample::Watched;
use crate::store::{self, Pending};
use crate::test_support::TestDir;

const AGY_SIGNING_IN: &str = include_str!("../tests/fixtures/agy/agy-signing-in.txt");
const AGY_PRODUCER_DRAFT: &str = include_str!("../tests/fixtures/agy/agy-producer-draft.txt");
const AGY_IDLE_LAUNCH: &str = include_str!("../tests/fixtures/agy/idle-launch.txt");
const AGY_PLAN_QUESTION: &str = include_str!("../tests/fixtures/agy/plan-question.txt");
const AGY_FOLDER_TRUST: &str = include_str!("../tests/fixtures/agy/folder-trust.txt");
const CMD_OUTSIDE_WORKSPACE: &str = include_str!("../tests/fixtures/cmd/cmd-outside-workspace.txt");
const CMD_CREATE_FILE: &str = include_str!("../tests/fixtures/cmd/cmd-create-file.txt");

#[derive(Default)]
struct FakeCore {
    screen: String,
    keys: Vec<String>,
}

async fn serve(stream: DuplexStream, fake: Arc<Mutex<FakeCore>>) {
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = Reader::new(read);
    let hello = reader.read_line().await.unwrap().unwrap();
    let request: Value = serde_json::from_slice(&hello).unwrap();
    assert_eq!(request["method"], "hello");
    write
        .write_all(
            serde_json::to_string(&json!({
                "jsonrpc": "2.0", "id": 0,
                "result": {
                    "generation": 1, "version": "fake-core 0.0",
                    "framed": true,
                    "core": {"id": "core-fake-1", "name": "fake-core"},
                    "methods": ["terminal.read", "terminal.keys"],
                    "features": [],
                }
            }))
            .unwrap()
            .as_bytes(),
        )
        .await
        .unwrap();
    write.write_all(b"\n").await.unwrap();
    while let Ok(Some(frame)) = reader.read_frame().await {
        if frame.channel != 0 || frame.kind != Kind::Data {
            continue;
        }
        let request: Value = serde_json::from_slice(&frame.payload).unwrap();
        let result = match request["method"].as_str().unwrap_or("") {
            "terminal.read" => json!({
                "text": fake.lock().unwrap().screen.clone(),
                "complete": true,
                "how": "screen",
            }),
            "terminal.keys" => {
                let keys = request["params"]["keys"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|k| k.as_str().unwrap().to_string())
                    .collect::<Vec<_>>();
                fake.lock().unwrap().keys.extend(keys);
                json!({})
            }
            other => panic!("unexpected hc call: {other}"),
        };
        let reply = serde_json::to_vec(&json!({
            "jsonrpc": "2.0", "id": request["id"], "result": result,
        }))
        .unwrap();
        write
            .write_all(&Frame::data(0, reply).encode())
            .await
            .unwrap();
    }
}

async fn fake_stage() -> (Stage, Arc<Mutex<FakeCore>>) {
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    let fake = Arc::new(Mutex::new(FakeCore::default()));
    tokio::spawn(serve(core_side, Arc::clone(&fake)));
    let (read, write) = tokio::io::split(client_side);
    let client = Client::open(Box::pin(read), Box::pin(write), None)
        .await
        .unwrap();
    (Stage::for_test(client), fake)
}

fn actor_identity(runner: &str) -> ActorIdentity {
    ActorIdentity {
        runner: runner.into(),
        runner_version: None,
        model: "plain".into(),
        effort: None,
        permission_profile: "standard".into(),
    }
}

fn setup_studio(dir: &Path, producer_runner: &str, actor_runner: &str) {
    let append = |by: &str, kind: EventKind| {
        let by = by.to_string();
        store::append(dir, move |_, _| {
            Ok::<_, std::convert::Infallible>(Pending {
                event: NewEvent::new(EventBy::role(by), kind),
                files: Vec::new(),
            })
        })
        .unwrap();
    };
    append(
        "user",
        EventKind::StudioCreated {
            title: "acme-shop".into(),
            repo: "/home/user/acme-shop".into(),
            members: Vec::new(),
            no_git: false,
            mode: backlot_model::event::StudioMode::Standard,
            roles: Box::new(backlot_model::event::RoleConfigs {
                producer: actor_identity(producer_runner),
                director: actor_identity("fake"),
                writer: actor_identity("fake"),
            }),
            candidates: vec![backlot_model::event::Candidate {
                name: "actor-a".into(),
                runner: actor_runner.into(),
                runner_version: None,
                model: "plain".into(),
                effort: None,
                permission_profile: "standard".into(),
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
        },
    );
    append(
        "director",
        EventKind::ReqVersion {
            version: 1,
            file: "req/v1.md".into(),
            summary: "first".into(),
        },
    );
    append(
        "director",
        EventKind::TaskCreated {
            task: "t1".into(),
            title: "front".into(),
            task_kind: "feature".into(),
            requirement_version: 1,
            acceptance: "plan/a.md".into(),
            dependencies: Vec::new(),
        },
    );
    append(
        "director",
        EventKind::TaskCast {
            task: "t1".into(),
            candidate: "actor-a".into(),
            actor: actor_identity(actor_runner),
            reason: "picked".into(),
            priority: None,
        },
    );
}

fn live_term(role: &str) -> LiveTerminal {
    LiveTerminal {
        name: format!("bl-acme-shop-{role}"),
        address: role.into(),
        argv: Vec::new(),
        cwd: "/home/user/acme-shop".into(),
        labels: BTreeMap::new(),
        ..Default::default()
    }
}

async fn sample_role(
    stage: &Stage,
    dir: &Path,
    term: &LiveTerminal,
    role: &str,
    runner: &str,
    now_ms: u64,
) {
    let state = store::load_state(dir).unwrap();
    let watched = Watched {
        dir,
        state: &state,
        role,
        runner,
        term,
    };
    stage
        .sample_work("acme-shop", &watched, now_ms)
        .await
        .unwrap();
}

fn raised_events(dir: &Path) -> Vec<backlot_model::event::Event> {
    store::load_events(dir)
        .unwrap()
        .0
        .into_iter()
        .filter(|e| e.kind.name() == "attention.raised")
        .collect()
}

fn cleared_events(dir: &Path) -> Vec<backlot_model::event::Event> {
    store::load_events(dir)
        .unwrap()
        .0
        .into_iter()
        .filter(|e| e.kind.name() == "attention.cleared")
        .collect()
}

fn approve(dir: &Path, role: &str, by: &str, decision: ApprovalDecision) {
    let (role, by) = (role.to_string(), by.to_string());
    store::append(dir, move |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role(by),
                EventKind::RoleApproval { role, decision },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
}

#[tokio::test]
async fn agy_signing_in_stays_silent_until_timeout_then_raises_auth_error() {
    assert_eq!(
        runners::work_state("agy", AGY_SIGNING_IN),
        Work::Working,
        "Signing in... screen must classify as Working"
    );
    assert!(!recognized_permission("agy", AGY_SIGNING_IN));
    assert!(!dialog::generic_dialog(AGY_SIGNING_IN));
    assert!(!permit::in_episode(Work::Working, AGY_SIGNING_IN));
    assert!(!question::in_episode(
        "agy",
        Work::Blocked,
        None,
        AGY_SIGNING_IN,
        None
    ));

    let dir = TestDir::new("backlot-misread-signin");
    setup_studio(&dir, "agy", "cmd");
    let mut term = live_term("producer");
    term.status = Some(Status {
        state: "blocked".into(),
        msg: "sign in".into(),
        kind: Some("auth".into()),
        ..Default::default()
    });
    let (stage, fake) = fake_stage().await;
    fake.lock().unwrap().screen = AGY_SIGNING_IN.into();
    let t0 = 1_000_000u64;

    // Transient startup sign-in (past permission EPISODE_MIN_MS and up to timeout - 1s):
    // must raise zero attentions (never `permission prompt`), and keep Working.
    for offset in [0, 1_000, 2_000, 10_000, DEFAULT_TIMEOUT_MS - 1_000] {
        sample_role(&stage, &dir, &term, "producer", "agy", t0 + offset).await;
        assert_eq!(
            stage
                .work
                .borrow()
                .get(&("acme-shop".into(), "producer".into()))
                .copied(),
            Some(Work::Working),
            "at offset {offset}ms agy sign-in must look Working"
        );
        assert!(
            raised_events(&dir).is_empty(),
            "unexpected attention before timeout at offset {offset}ms: {:?}",
            raised_events(&dir)
        );
    }

    // Past the timeout: raises `auth error` (`needs_user = true`), never `permission prompt`.
    sample_role(
        &stage,
        &dir,
        &term,
        "producer",
        "agy",
        t0 + DEFAULT_TIMEOUT_MS,
    )
    .await;
    sample_role(
        &stage,
        &dir,
        &term,
        "producer",
        "agy",
        t0 + DEFAULT_TIMEOUT_MS + 1_000,
    )
    .await;
    let raised = raised_events(&dir);
    assert_eq!(
        raised.len(),
        1,
        "expected single auth error attention: {raised:?}"
    );
    let EventKind::AttentionRaised {
        reason, needs_user, ..
    } = &raised[0].kind
    else {
        panic!("expected AttentionRaised");
    };
    assert_eq!(reason, "auth error");
    assert!(*needs_user);

    // Once sign-in completes and the screen moves to idle launch, clear `auth error`.
    fake.lock().unwrap().screen = AGY_IDLE_LAUNCH.into();
    term.status = None;
    sample_role(
        &stage,
        &dir,
        &term,
        "producer",
        "agy",
        t0 + DEFAULT_TIMEOUT_MS + 2_000,
    )
    .await;
    assert_eq!(cleared_events(&dir).len(), 1);
}

#[tokio::test]
async fn agy_producer_draft_is_waiting_not_question_or_permission() {
    assert_eq!(
        runners::work_state("agy", AGY_PRODUCER_DRAFT),
        Work::Waiting,
        "unsent draft in composer must read as Waiting"
    );
    assert!(!dialog::generic_dialog(AGY_PRODUCER_DRAFT));
    assert!(!question::in_episode(
        "agy",
        Work::Waiting,
        None,
        AGY_PRODUCER_DRAFT,
        None
    ));
    assert!(!question::in_episode(
        "agy",
        Work::Blocked,
        None,
        AGY_PRODUCER_DRAFT,
        None
    ));
    assert_ne!(
        dialog::question_summary(AGY_PRODUCER_DRAFT).as_deref(),
        Some("复测，均需另行明确范围与授权。")
    );

    assert!(dialog::has_non_empty_draft(AGY_PRODUCER_DRAFT));
    assert!(!dialog::has_non_empty_draft(AGY_IDLE_LAUNCH));
    assert!(!dialog::has_non_empty_draft(include_str!(
        "../tests/fixtures/agy/idle-after-turn.txt"
    )));
    assert!(!dialog::has_non_empty_draft(AGY_PLAN_QUESTION));
    assert!(!dialog::has_non_empty_draft(AGY_FOLDER_TRUST));

    // Real agy plan question and generic question controls must stay `question`.
    assert!(question::in_episode(
        "agy",
        Work::Blocked,
        None,
        AGY_PLAN_QUESTION,
        None
    ));
    let generic_q = "Which format?\n> 1. JSON\n  2. YAML\nEnter to select\n";
    assert!(dialog::generic_dialog(generic_q));
    assert!(question::in_episode(
        "agy",
        Work::Blocked,
        None,
        generic_q,
        None
    ));

    // Folder trust fixture (#188) stays a recognized permission prompt.
    assert_eq!(runners::work_state("agy", AGY_FOLDER_TRUST), Work::Blocked);
    assert!(recognized_permission("agy", AGY_FOLDER_TRUST));
    assert!(!question::in_episode(
        "agy",
        Work::Blocked,
        None,
        AGY_FOLDER_TRUST,
        None
    ));

    // End-to-end sample on producer with and without hc `waiting_for_person`:
    // never raises `question` or `permission prompt`.
    for waiting_for_person in [false, true] {
        let dir = TestDir::new("backlot-misread-draft");
        setup_studio(&dir, "agy", "cmd");
        let mut term = live_term("producer");
        term.waiting_for_person = waiting_for_person;
        let (stage, fake) = fake_stage().await;
        fake.lock().unwrap().screen = AGY_PRODUCER_DRAFT.into();
        let t0 = 2_000_000u64;
        for offset in [0, 1_000, 2_000, 3_000] {
            sample_role(&stage, &dir, &term, "producer", "agy", t0 + offset).await;
        }
        assert!(
            raised_events(&dir).is_empty(),
            "waiting_for_person={waiting_for_person} raised attention on draft: {:?}",
            raised_events(&dir)
        );
    }
}

async fn assert_cmd_permission_flow(name: &str, screen: &str, expected_detail_sub: Option<&str>) {
    assert_eq!(
        runners::work_state("cmd", screen),
        Work::Blocked,
        "{name} must read as Blocked"
    );
    assert!(
        recognized_permission("cmd", screen),
        "{name} must match recognized_permission(\"cmd\", ..)"
    );
    assert!(
        !question::in_episode("cmd", Work::Blocked, None, screen, None),
        "{name} must not enter question::in_episode"
    );
    assert!(permit::in_episode(Work::Blocked, screen));
    assert!(
        permit::prompt_block("cmd", screen).is_some(),
        "{name} must extract a prompt_block for the director"
    );
    assert_eq!(approval_keys("cmd", ApprovalDecision::Allow), Some("enter"));
    assert_eq!(approval_keys("cmd", ApprovalDecision::Deny), Some("esc"));

    let dir = TestDir::new("backlot-misread-cmd");
    setup_studio(&dir, "agy", "cmd");
    let term = live_term("actor-t1-c1");
    let (stage, fake) = fake_stage().await;
    fake.lock().unwrap().screen = screen.into();
    let t0 = 3_000_000u64;
    for offset in [0, 1_000, 2_000] {
        sample_role(&stage, &dir, &term, "actor-t1-c1", "cmd", t0 + offset).await;
    }
    let raised = raised_events(&dir);
    assert_eq!(
        raised.len(),
        1,
        "{name}: expected 1 attention, got {raised:?}"
    );
    let EventKind::AttentionRaised {
        reason,
        needs_user,
        detail,
        ..
    } = &raised[0].kind
    else {
        panic!("expected AttentionRaised");
    };
    assert_eq!(
        reason, PERMISSION_PROMPT,
        "{name}: got {reason} instead of {PERMISSION_PROMPT}"
    );
    assert_ne!(reason, QUESTION);
    assert!(
        !needs_user,
        "{name}: actor permission must route to director"
    );
    assert!(!detail.is_empty(), "{name}: expected non-empty detail");
    if let Some(sub) = expected_detail_sub {
        assert!(
            detail.contains(sub),
            "{name} detail missing {sub:?}: {detail}"
        );
    }

    approve(&dir, "actor-t1-c1", "director", ApprovalDecision::Allow);
    sample_role(&stage, &dir, &term, "actor-t1-c1", "cmd", t0 + 3_000).await;
    assert_eq!(fake.lock().unwrap().keys.as_slice(), ["enter"]);
}

#[tokio::test]
async fn cmd_outside_workspace_is_actor_permission_prompt() {
    // Control: a genuine cmd non-permission choice dialog stays `question`.
    let real_cmd_q =
        "Which format should we use?\n❯ 1. JSON\n  2. YAML\n↑/↓ navigate · enter select\n";
    assert_eq!(runners::work_state("cmd", real_cmd_q), Work::Blocked);
    assert!(!recognized_permission("cmd", real_cmd_q));
    assert!(question::in_episode(
        "cmd",
        Work::Blocked,
        None,
        real_cmd_q,
        None
    ));

    assert_cmd_permission_flow(
        "outside-workspace",
        CMD_OUTSIDE_WORKSPACE,
        Some("Outside the workspace"),
    )
    .await;
}

#[tokio::test]
async fn cmd_create_file_is_actor_permission_prompt() {
    let detail = runners::prompt_detail("cmd", CMD_CREATE_FILE)
        .expect("Create File dialog should yield prompt_detail");
    assert!(
        detail.contains("create scripts/v3/group.py?"),
        "unexpected Create File detail: {detail}"
    );

    assert_cmd_permission_flow("create-file", CMD_CREATE_FILE, Some("scripts/v3/group.py")).await;
}
