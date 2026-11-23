//! Tests for Hollywood signal reading in `hwatch.rs` and summary
//! projection in `daemon.rs`: `<hdir>/roles/hollywood/signals.jsonl`
//! combines ahead of `structured_state` and `screen_work`, and
//! `Stage::summary_view` publishes `hollywood.work` + `pending`.

use std::collections::BTreeMap;
use std::fs;
use std::sync::{Arc, Mutex};

use backlot_model::EventBy;
use backlot_model::event::ActorIdentity;
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
use crate::hwatch::hsignal_work;
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
