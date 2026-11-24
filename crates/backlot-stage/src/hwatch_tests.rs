//! Tests for Hollywood signal reading in `hwatch.rs` and summary
//! projection in `daemon.rs`: `<hdir>/roles/hollywood/signals.jsonl`
//! combines ahead of `structured_state` and `screen_work`, and
//! `Stage::summary_view` publishes `hollywood.work` + `pending`.

use std::collections::BTreeMap;
use std::fs;
use std::sync::{Arc, Mutex};

use backlot_model::EventBy;
use backlot_model::event::{ActorIdentity, ApprovalDecision};
use backlot_model::hollywood::{
    HOLLYWOOD, HOLLYWOOD_TERMINAL, HollywoodKind, NewHollywoodEvent, RoleMotion,
};
use backlot_model::summary::Work;
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use hc_proto::frame::{Frame, Kind};
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, DuplexStream};

use crate::config::HollywoodCfg;
use crate::daemon::Stage;
use crate::hstage::hkey;
use crate::hwatch::{hollywood_prompt, hsignal_work};
use crate::prompt_fixtures::{CMD_FULL, CMD_MEDIUM, CMD_NARROW, CMD_WIDE};
use crate::reconcile::LiveTerminal;
use crate::signals::signals_path;
use crate::store::{self, HPending};
use crate::test_support::TestDir;

async fn serve_read(stream: DuplexStream, screen: Arc<Mutex<String>>) {
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = Reader::new(read);
    let hello = reader.read_line().await.unwrap().unwrap();
    let req: Value = serde_json::from_slice(&hello).unwrap();
    assert_eq!(req["method"], "hello");
    write
        .write_all(
            serde_json::to_string(&json!({
                "jsonrpc": "2.0", "id": 0,
                "result": {
                    "generation": 1, "version": "fake-core 0.0",
                    "framed": true,
                    "core": {"id": "core-fake-1", "name": "fake-core"},
                    "methods": ["terminal.read"],
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
        let req: Value = serde_json::from_slice(&frame.payload).unwrap();
        assert_eq!(req["method"], "terminal.read");
        let reply = serde_json::to_vec(&json!({
            "jsonrpc": "2.0",
            "id": req["id"],
            "result": {
                "text": screen.lock().unwrap().clone(),
                "complete": true,
                "how": "screen",
            }
        }))
        .unwrap();
        write
            .write_all(&Frame::data(0, reply).encode())
            .await
            .unwrap();
    }
}

#[test]
fn hsignal_work_combines_signals_jsonl_structured_state_and_screen() {
    let tmp = TestDir::new("backlot-hwatch-unit");
    let hdir = store::hollywood_dir(tmp.path());
    let sig_file = signals_path(&hdir, HOLLYWOOD);
    fs::create_dir_all(sig_file.parent().unwrap()).unwrap();

    let mut term = LiveTerminal {
        name: HOLLYWOOD_TERMINAL.into(),
        address: "t-hw".into(),
        ..Default::default()
    };

    // No signals yet -> structured_state ("working") overrides idle screen.
    term.status = Some(serde_json::from_value(json!({ "state": "working" })).unwrap());
    assert_eq!(
        hsignal_work(&hdir, Some("sid-1"), &term, "claude", "> "),
        Work::Working
    );

    // Session's `Stop` signal overrides stale structured "working" to `Waiting`.
    fs::write(
        &sig_file,
        concat!(
            r#"{"session_id":"other","ts_ms":1,"event":"PreToolUse"}"#,
            "\n",
            r#"{"session_id":"sid-1","ts_ms":2,"event":"Stop"}"#,
            "\n"
        ),
    )
    .unwrap();
    assert_eq!(
        hsignal_work(&hdir, Some("sid-1"), &term, "claude", "> "),
        Work::Waiting
    );

    // Session's `UserPromptSubmit` / `PreToolUse` reads `Working` even on a calm screen.
    fs::write(
        &sig_file,
        r#"{"session_id":"sid-1","ts_ms":3,"event":"UserPromptSubmit"}"#,
    )
    .unwrap();
    term.status = None;
    assert_eq!(
        hsignal_work(&hdir, Some("sid-1"), &term, "claude", "> "),
        Work::Working
    );

    // `Blocked` on any channel wins.
    term.waiting_for_person = true;
    assert_eq!(
        hsignal_work(&hdir, Some("sid-1"), &term, "claude", "> "),
        Work::Blocked
    );
}

#[tokio::test]
async fn stage_hsample_reads_signals_and_summary_publishes_work_and_pending() {
    let tmp = TestDir::new("backlot-hwatch-stage");
    let hdir = store::hollywood_dir(tmp.path());
    let sig_file = signals_path(&hdir, HOLLYWOOD);
    fs::create_dir_all(sig_file.parent().unwrap()).unwrap();

    // Record live Hollywood role with session_id `sid-hw` and an undelivered user tell.
    store::append_hollywood(&hdir, |_, _| {
        Ok::<HPending, std::convert::Infallible>(HPending {
            event: NewHollywoodEvent::new(
                EventBy::role("stage"),
                HollywoodKind::RoleState {
                    state: RoleMotion::Started,
                    terminal: Some("t-hw".into()),
                    actor: None,
                    session_id: Some("sid-hw".into()),
                    reason: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    store::append_hollywood(&hdir, |_, _| {
        Ok::<HPending, std::convert::Infallible>(HPending {
            event: NewHollywoodEvent::new(
                EventBy::role("user"),
                HollywoodKind::MessageSent {
                    to: "hollywood".into(),
                    studio: None,
                    text: Some("check studios".into()),
                    file: None,
                    source_seq: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();

    fs::write(
        &sig_file,
        r#"{"session_id":"sid-hw","ts_ms":10,"event":"PreToolUse","tool_name":"Bash"}"#,
    )
    .unwrap();

    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    let screen = Arc::new(Mutex::new("> ".to_string()));
    tokio::spawn(serve_read(core_side, Arc::clone(&screen)));
    let (read, write) = tokio::io::split(client_side);
    let client = Client::open(Box::pin(read), Box::pin(write), None)
        .await
        .unwrap();
    let mut stage = Stage::for_test(client);
    stage.base = tmp.path().to_path_buf();
    *stage.hcfg.borrow_mut() = Some(HollywoodCfg {
        runner: "claude".into(),
        model: "sonnet".into(),
        effort: None,
        key: None,
        default_cast: None,
    });
    stage.hrefresh(&hdir);

    let terminals = vec![LiveTerminal {
        name: HOLLYWOOD_TERMINAL.into(),
        address: "t-hw".into(),
        ..Default::default()
    }];
    stage.hsample(&hdir, &terminals, "claude", 5_000).await;
    assert_eq!(stage.work.borrow().get(&hkey()), Some(&Work::Working));

    let view = stage.summary_view(&BTreeMap::new());
    let hw = view.hollywood.expect("hollywood section");
    assert_eq!(hw.work, Some(Work::Working));
    assert!(hw.pending);
}

/// A neutral cmd permission prompt (the S12a capture): the
/// `Execute Shell Command` title, the `needs to execute` sentence and
/// the numbered picker under a full-width rule.
fn cmd_prompt_screen(command: &str) -> String {
    format!(
        "◇ Conjuring… esc to interrupt • 5s • ↓ 219\n\
         ─────────────────────────────────────────\n\
         Execute Shell Command\n\
         Command Code needs to execute this shell command:\n\
         \x20 {command}\n\
         ❯ 1. Yes\n\
         \x20 2. Yes, don't ask again for `cmd` commands in this project\n\
         \x20 3. No, tell Command Code what to do differently\n"
    )
}

/// The prompt is read through the wrapping: the 21-column capture,
/// the 60-column re-wrap and the wide screen all name the whole
/// command, not the fragment under the sentence.
#[test]
fn hollywood_prompt_reads_cmd_at_every_width() {
    for screen in [CMD_NARROW, CMD_MEDIUM, CMD_WIDE] {
        assert_eq!(
            hollywood_prompt("cmd", screen, Work::Blocked),
            Some(CMD_FULL.to_string()),
            "{screen}"
        );
    }
}

/// A stage over a live Hollywood whose terminal answers `screen`.
async fn hstage_with_screen(tag: &str, screen: String) -> (Stage, TestDir, std::path::PathBuf) {
    let tmp = TestDir::new(tag);
    let hdir = store::hollywood_dir(tmp.path());
    store::append_hollywood(&hdir, |_, _| {
        Ok::<HPending, std::convert::Infallible>(HPending {
            event: NewHollywoodEvent::new(
                EventBy::role("stage"),
                HollywoodKind::RoleState {
                    state: RoleMotion::Started,
                    terminal: Some("t-hw".into()),
                    actor: None,
                    session_id: Some("sid-hw".into()),
                    reason: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    let screen = Arc::new(Mutex::new(screen));
    tokio::spawn(serve_read(core_side, screen));
    let (read, write) = tokio::io::split(client_side);
    let client = Client::open(Box::pin(read), Box::pin(write), None)
        .await
        .unwrap();
    let mut stage = Stage::for_test(client);
    stage.base = tmp.path().to_path_buf();
    *stage.hcfg.borrow_mut() = Some(HollywoodCfg {
        runner: "cmd".into(),
        model: "model-a".into(),
        effort: None,
        key: None,
        default_cast: None,
    });
    stage.hrefresh(&hdir);
    (stage, tmp, hdir)
}

/// A live, working Hollywood publishes `doing`, the terminal's own
/// reported status read exactly as a role's is.
#[tokio::test]
async fn a_live_working_hollywood_publishes_doing() {
    let (stage, _tmp, hdir) = hstage_with_screen(
        "backlot-hw-doing",
        "◇ Conjuring… esc to interrupt • 5s\n".into(),
    )
    .await;
    let terminals = vec![LiveTerminal {
        name: HOLLYWOOD_TERMINAL.into(),
        address: "t-hw".into(),
        ..Default::default()
    }];
    stage.hsample(&hdir, &terminals, "cmd", 5_000).await;
    let doing = BTreeMap::from([("t-hw".to_string(), "reading the ledgers".to_string())]);
    let view = stage.summary_view(&doing);
    let hw = view.hollywood.expect("hollywood section");
    assert_eq!(hw.work, Some(Work::Working));
    assert_eq!(hw.doing.as_deref(), Some("reading the ledgers"));
    assert!(!hw.attention_user);
    assert_eq!(hw.attention_detail, None);
}

/// A cmd permission prompt on Hollywood's screen publishes the
/// user-owned prompt — the same fields a role's `attention` carries.
#[tokio::test]
async fn a_cmd_prompt_on_hollywood_publishes_a_user_owned_prompt() {
    let (stage, _tmp, hdir) =
        hstage_with_screen("backlot-hw-prompt", cmd_prompt_screen("backlot --help")).await;
    let terminals = vec![LiveTerminal {
        name: HOLLYWOOD_TERMINAL.into(),
        address: "t-hw".into(),
        ..Default::default()
    }];
    stage.hsample(&hdir, &terminals, "cmd", 5_000).await;
    let view = stage.summary_view(&BTreeMap::new());
    let hw = view.hollywood.expect("hollywood section");
    assert_eq!(hw.work, Some(Work::Blocked));
    assert!(hw.attention_user);
    assert_eq!(
        hw.attention.as_deref(),
        Some("permission prompt — needs the user")
    );
    assert_eq!(hw.attention_detail.as_deref(), Some("backlot --help"));
    // Waiting after a reply publishes no prompt: a calm screen
    // clears it.
    let (stage, _tmp, hdir) = hstage_with_screen("backlot-hw-calm", "> ".into()).await;
    stage.hsample(&hdir, &terminals, "cmd", 5_000).await;
    let view = stage.summary_view(&BTreeMap::new());
    let hw = view.hollywood.expect("hollywood section");
    assert!(!hw.attention_user);
    assert_eq!(hw.attention_detail, None);
}

/// A core that answers `terminal.read` from `screen` and records
/// every `terminal.keys` key.
async fn serve_prompt(
    stream: DuplexStream,
    screen: Arc<Mutex<String>>,
    keys: Arc<Mutex<Vec<String>>>,
) {
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = Reader::new(read);
    let hello = reader.read_line().await.unwrap().unwrap();
    let req: Value = serde_json::from_slice(&hello).unwrap();
    assert_eq!(req["method"], "hello");
    let hello = json!({
        "jsonrpc": "2.0", "id": 0,
        "result": {
            "generation": 1, "version": "fake-core 0.0",
            "framed": true,
            "core": {"id": "core-fake-1", "name": "fake-core"},
            "methods": ["terminal.read", "terminal.keys"],
            "features": [],
        }
    });
    write
        .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
        .await
        .unwrap();
    write.write_all(b"\n").await.unwrap();
    while let Ok(Some(frame)) = reader.read_frame().await {
        if frame.channel != 0 || frame.kind != Kind::Data {
            continue;
        }
        let req: Value = serde_json::from_slice(&frame.payload).unwrap();
        let reply = match req["method"].as_str() {
            Some("terminal.read") => json!({
                "jsonrpc": "2.0",
                "id": req["id"],
                "result": {
                    "text": screen.lock().unwrap().clone(),
                    "complete": true,
                    "how": "screen",
                }
            }),
            Some("terminal.keys") => {
                for key in req["params"]["keys"].as_array().into_iter().flatten() {
                    keys.lock()
                        .unwrap()
                        .push(key.as_str().unwrap_or_default().to_string());
                }
                json!({"jsonrpc": "2.0", "id": req["id"], "result": {}})
            }
            _ => json!({
                "jsonrpc": "2.0",
                "id": req["id"],
                "error": {"code": -32601, "message": "not offered"},
            }),
        };
        write
            .write_all(&Frame::data(0, serde_json::to_vec(&reply).unwrap()).encode())
            .await
            .unwrap();
    }
}

/// A stage over live Hollywood on cmd whose terminal reports
/// `kind: permission` / `state: blocked` while its screen says
/// nothing the prompt classifiers can read.
async fn hstage_structured_blocked(
    tag: &str,
    screen: &str,
) -> (
    Stage,
    TestDir,
    std::path::PathBuf,
    Vec<LiveTerminal>,
    Arc<Mutex<Vec<String>>>,
) {
    let tmp = TestDir::new(tag);
    let hdir = store::hollywood_dir(tmp.path());
    store::append_hollywood(&hdir, |_, _| {
        Ok::<_, std::convert::Infallible>(HPending {
            event: NewHollywoodEvent::new(
                EventBy::role("stage"),
                HollywoodKind::RoleState {
                    state: RoleMotion::Started,
                    terminal: Some("t-hw".into()),
                    actor: None,
                    session_id: Some("sid-hw".into()),
                    reason: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    let keys = Arc::new(Mutex::new(Vec::new()));
    tokio::spawn(serve_prompt(
        core_side,
        Arc::new(Mutex::new(screen.to_string())),
        Arc::clone(&keys),
    ));
    let (read, write) = tokio::io::split(client_side);
    let client = Client::open(Box::pin(read), Box::pin(write), None)
        .await
        .unwrap();
    let mut stage = Stage::for_test(client);
    stage.base = tmp.path().to_path_buf();
    *stage.hcfg.borrow_mut() = Some(HollywoodCfg {
        runner: "cmd".into(),
        model: "model-a".into(),
        effort: None,
        key: None,
        default_cast: None,
    });
    stage.hrefresh(&hdir);
    let term = LiveTerminal {
        name: HOLLYWOOD_TERMINAL.into(),
        address: "t-hw".into(),
        status: Some(
            serde_json::from_value(json!({"kind": "permission", "state": "blocked"})).unwrap(),
        ),
        ..Default::default()
    };
    (stage, tmp, hdir, vec![term], keys)
}

/// The terminal's structured status alone names the prompt: a screen
/// the classifiers cannot parse still publishes the user-owned row,
/// with the bare `permission prompt` detail.
#[tokio::test]
async fn a_structured_prompt_with_an_unreadable_screen_is_published() {
    let (stage, _tmp, hdir, terms, _keys) =
        hstage_structured_blocked("backlot-hw-structured", "> ").await;
    stage.hsample(&hdir, &terms, "cmd", 5_000).await;
    let view = stage.summary_view(&BTreeMap::new());
    let hw = view.hollywood.expect("hollywood section");
    assert_eq!(hw.work, Some(Work::Blocked));
    assert!(hw.attention_user);
    assert_eq!(hw.attention_detail.as_deref(), Some("permission prompt"));
}

/// A recorded answer reaches that prompt too: `enter` goes out once
/// and never again on the same approval.
#[tokio::test]
async fn a_recorded_allow_answers_a_structured_prompt_once() {
    let (stage, _tmp, hdir, terms, keys) =
        hstage_structured_blocked("backlot-hw-structured-answer", "> ").await;
    stage.hsample(&hdir, &terms, "cmd", 5_000).await;
    assert!(keys.lock().unwrap().is_empty(), "no key without a decision");

    store::append_hollywood(&hdir, |_, _| {
        Ok::<_, std::convert::Infallible>(HPending {
            event: NewHollywoodEvent::new(
                EventBy::role("user"),
                HollywoodKind::RoleApproval {
                    decision: ApprovalDecision::Allow,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    stage.hrefresh(&hdir);
    stage.hsample(&hdir, &terms, "cmd", 6_100).await;
    assert_eq!(keys.lock().unwrap().as_slice(), &["enter"]);

    stage.hsample(&hdir, &terms, "cmd", 7_200).await;
    assert_eq!(keys.lock().unwrap().as_slice(), &["enter"]);
}

/// The user's recorded answer sends the runner's run-once key —
/// `enter` on cmd's `❯ 1. Yes`, never option 2 — and only once.
#[tokio::test]
async fn hollywood_answers_its_prompt_with_the_one_time_key() {
    let tmp = TestDir::new("backlot-hw-answer");
    let hdir = store::hollywood_dir(tmp.path());
    store::append_hollywood(&hdir, |_, _| {
        Ok::<HPending, std::convert::Infallible>(HPending {
            event: NewHollywoodEvent::new(
                EventBy::role("stage"),
                HollywoodKind::RoleState {
                    state: RoleMotion::Started,
                    terminal: Some("t-hw".into()),
                    actor: None,
                    session_id: Some("sid-hw".into()),
                    reason: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    let keys = Arc::new(Mutex::new(Vec::new()));
    tokio::spawn(serve_prompt(
        core_side,
        Arc::new(Mutex::new(cmd_prompt_screen("backlot --help"))),
        Arc::clone(&keys),
    ));
    let (read, write) = tokio::io::split(client_side);
    let client = Client::open(Box::pin(read), Box::pin(write), None)
        .await
        .unwrap();
    let mut stage = Stage::for_test(client);
    stage.base = tmp.path().to_path_buf();
    *stage.hcfg.borrow_mut() = Some(HollywoodCfg {
        runner: "cmd".into(),
        model: "model-a".into(),
        effort: None,
        key: None,
        default_cast: None,
    });
    stage.hrefresh(&hdir);
    let terminals = vec![LiveTerminal {
        name: HOLLYWOOD_TERMINAL.into(),
        address: "t-hw".into(),
        ..Default::default()
    }];

    // The prompt shows; no decision yet — nothing goes out.
    stage.hsample(&hdir, &terminals, "cmd", 5_000).await;
    assert!(keys.lock().unwrap().is_empty(), "no key without a decision");

    // The user answers from the bar: the approval lands after the
    // prompt's first sighting.
    store::append_hollywood(&hdir, |_, _| {
        Ok::<HPending, std::convert::Infallible>(HPending {
            event: NewHollywoodEvent::new(
                EventBy::role("user"),
                HollywoodKind::RoleApproval {
                    decision: ApprovalDecision::Allow,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    stage.hrefresh(&hdir);
    stage.hsample(&hdir, &terminals, "cmd", 6_100).await;
    assert_eq!(keys.lock().unwrap().as_slice(), &["enter"]);

    // A slow screen never sends the same approval twice.
    stage.hsample(&hdir, &terminals, "cmd", 7_200).await;
    assert_eq!(keys.lock().unwrap().as_slice(), &["enter"]);
}

/// A stage whose core answers `hello` and nothing else — `hdiscover`
/// never calls it.
async fn quiet_stage() -> Stage {
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        let (read, mut write) = tokio::io::split(core_side);
        let mut reader = Reader::new(read);
        let hello = reader.read_line().await.unwrap().unwrap();
        let request: Value = serde_json::from_slice(&hello).unwrap();
        assert_eq!(request["method"], "hello");
        let reply = serde_json::to_vec(&json!({
            "jsonrpc": "2.0", "id": request["id"],
            "result": {
                "generation": 1, "version": "fake-core 0.0",
                "framed": true,
                "core": {"id": "core-fake-1", "name": "fake-core"},
                "methods": [], "features": [],
            }
        }))
        .unwrap();
        write.write_all(&reply).await.unwrap();
        write.write_all(b"\n").await.unwrap();
        while let Ok(Some(_)) = reader.read_frame().await {}
    });
    let (read, write) = tokio::io::split(client_side);
    let client = Client::open(Box::pin(read), Box::pin(write), None)
        .await
        .unwrap();
    Stage::for_test(client)
}

/// Hollywood on cmd starts with no session — cmd mints it — so the
/// watch pass finds it under
/// `~/.commandcode/projects/<slug(hollywood cwd)>/` and records it on
/// a `role.state started`, exactly as a studio role's discovery does.
/// The devin conversation the runner change closed must not stand in
/// the way.
#[tokio::test]
async fn stage_hdiscover_records_a_cmd_session_after_a_start() {
    let tmp = TestDir::new("backlot-hdiscover-cmd");
    let hdir = store::hollywood_dir(tmp.path());
    let home = tmp.join("home");
    let cwd = hdir.to_string_lossy().into_owned();

    let actor = |runner: &str| ActorIdentity {
        runner: runner.into(),
        runner_version: None,
        model: "m".into(),
        effort: None,
        permission_profile: "standard".into(),
    };
    // devin's conversation, closed on a runner change, then cmd
    // started without a session.
    crate::hstage::record_role(
        &hdir,
        RoleMotion::Started,
        Some("t-devin".into()),
        Some(actor("devin")),
        Some("beaded-harp".into()),
        None,
    )
    .unwrap();
    crate::hstage::record_role(
        &hdir,
        RoleMotion::Closed,
        None,
        None,
        None,
        Some("runner changed"),
    )
    .unwrap();
    crate::hstage::record_role(
        &hdir,
        RoleMotion::Started,
        Some("t-cmd".into()),
        Some(actor("cmd")),
        None,
        None,
    )
    .unwrap();

    // A fake cmd projects tree: `<slug(cwd)>/<uuid>.jsonl` whose first
    // line names the cwd.
    let uuid = "55555555-5555-4555-8555-555555555555";
    let projects = home.join(".commandcode/projects");
    let slug_dir = projects.join(crate::runners::cmd::slug(&cwd));
    fs::create_dir_all(&slug_dir).unwrap();
    fs::write(
        slug_dir.join(format!("{uuid}.jsonl")),
        format!("{{\"type\":\"session\",\"version\":3,\"id\":\"{uuid}\",\"cwd\":\"{cwd}\"}}\n"),
    )
    .unwrap();

    let mut stage = quiet_stage().await;
    stage.base = tmp.path().to_path_buf();
    stage
        .env_vars
        .insert("HOME".into(), home.to_string_lossy().into_owned());
    stage.hrefresh(&hdir);
    // The reducer cleared the old runner's id; discovery has room.
    assert_eq!(stage.hstate.borrow().role.session_id, None);

    let terminals = vec![LiveTerminal {
        name: HOLLYWOOD_TERMINAL.into(),
        address: "t-cmd".into(),
        ..Default::default()
    }];
    stage.hdiscover(&hdir, &terminals);
    let state = store::load_hstate(&hdir).unwrap();
    assert_eq!(state.role.session_id.as_deref(), Some(uuid));
}

/// A `question` dialog on Hollywood (`agy` plan-mode question or generic
/// picker) surfaces `question — needs the user` with `attention_user =
/// true`, `can_answer_prompt = false`, and suppresses approval keys.
#[tokio::test]
async fn a_question_dialog_on_hollywood_is_user_owned_without_approval_keys() {
    let picker = include_str!("../tests/fixtures/agy/plan-question.txt");
    let (stage, _tmp, hdir, mut terminals, keys) =
        hstage_structured_blocked("backlot-hw-question", picker).await;
    terminals[0].status =
        Some(serde_json::from_value(json!({"kind": "question", "state": "blocked"})).unwrap());
    stage.hsample(&hdir, &terminals, "claude", 5_000).await;
    let view = stage.summary_view(&BTreeMap::new());
    let hw = view.hollywood.expect("hollywood section");
    assert_eq!(hw.work, Some(Work::Blocked));
    assert!(hw.attention_user);
    assert!(!hw.can_answer_prompt);
    assert_eq!(hw.attention.as_deref(), Some("question — needs the user"));
    assert!(
        hw.attention_detail
            .as_deref()
            .is_some_and(|d| d.contains("Question 1/1:")),
        "{:?}",
        hw.attention_detail
    );

    // Even if a user approval were recorded, Hollywood must never press
    // `enter` into a `question` dialog.
    store::append_hollywood(&hdir, |_, _| {
        Ok::<_, std::convert::Infallible>(HPending {
            event: NewHollywoodEvent::new(
                EventBy::role("user"),
                HollywoodKind::RoleApproval {
                    decision: ApprovalDecision::Allow,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    stage.hrefresh(&hdir);
    stage.hsample(&hdir, &terminals, "claude", 7_000).await;
    assert!(keys.lock().unwrap().is_empty());
}
