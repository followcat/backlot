//! PR 5 (`design-capture`) stage tests: hook ingestion, taint filtering,
//! fallback hard-gate enforcement (`design next`), and `design_gate = off`.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use backlot_model::NewEvent;
use backlot_model::design::{DesignSlot, EntrySource};
use backlot_model::event::{ActorIdentity, EventBy, EventKind};
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use hc_proto::frame::{Frame, Kind};
use hc_proto::terminal::Status;
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, DuplexStream};

use crate::cli::{Env, Fail, run};
use crate::cli_testkit::{CAST_TOML, Scratch, fixture, scratch};
use crate::daemon::Stage;
use crate::hook::{run_io_env, run_io_with_delay};
use crate::reconcile::LiveTerminal;
use crate::sample::Watched;
use crate::signals::signals_path;
use crate::store::{self, Pending};

#[derive(Default)]
pub(super) struct FakeCore {
    pub(super) screen: String,
}

async fn serve(stream: DuplexStream, fake: Arc<Mutex<FakeCore>>) {
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = Reader::new(read);
    let _hello = reader.read_line().await.unwrap().unwrap();
    let hi = json!({
        "jsonrpc": "2.0", "id": 0,
        "result": {
            "generation": 1, "version": "fake-core 0.0", "framed": true,
            "core": {"id": "core-fake-1", "name": "fake-core"},
            "methods": ["terminal.read"], "features": [],
        }
    });
    write.write_all(format!("{hi}\n").as_bytes()).await.unwrap();
    while let Ok(Some(frame)) = reader.read_frame().await {
        if frame.channel != 0 || frame.kind != Kind::Data {
            continue;
        }
        let req: Value = serde_json::from_slice(&frame.payload).unwrap();
        let res = match req["method"].as_str().unwrap_or("") {
            "terminal.read" => json!({
                "text": fake.lock().unwrap().screen.clone(),
                "complete": true, "how": "screen",
            }),
            other => panic!("unexpected rpc method: {other}"),
        };
        let reply =
            serde_json::to_vec(&json!({ "jsonrpc": "2.0", "id": req["id"], "result": res }))
                .unwrap();
        write
            .write_all(&Frame::data(0, reply).encode())
            .await
            .unwrap();
    }
}

pub(super) async fn fake_stage(
    base: &Path,
    env_vars: BTreeMap<String, String>,
) -> (Stage, Arc<Mutex<FakeCore>>) {
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    let fake = Arc::new(Mutex::new(FakeCore::default()));
    tokio::spawn(serve(core_side, Arc::clone(&fake)));
    let (read, write) = tokio::io::split(client_side);
    let client = Client::open(Box::pin(read), Box::pin(write), None)
        .await
        .unwrap();
    let mut st = Stage::for_test(client);
    st.base = base.to_path_buf();
    st.state_dir = base.to_string_lossy().into_owned();
    st.env_vars = env_vars;
    (st, fake)
}

pub(super) fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn actor(runner: &str) -> ActorIdentity {
    ActorIdentity {
        runner: runner.into(),
        runner_version: None,
        model: "m".into(),
        effort: Some("high".into()),
        permission_profile: "standard".into(),
    }
}

pub(super) fn run_ok(env: &Env, args: &[&str]) -> String {
    let owned: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    run(&owned, env).unwrap_or_else(|e| panic!("expected Ok for {args:?}: {e:?}"))
}

pub(super) fn run_refused(env: &Env, args: &[&str]) -> String {
    let owned: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    match run(&owned, env) {
        Err(Fail::Refused(m)) => m,
        other => panic!("expected Refused for {args:?}, got {other:?}"),
    }
}

pub(super) fn setup_studio(
    gate: &str,
    open_waiting: bool,
    runner: &str,
) -> (Scratch, Env, PathBuf) {
    let (scratch, mut env) = scratch();
    let xdg = scratch.dir.join("xdg");
    fs::create_dir_all(&xdg).unwrap();
    env.vars
        .insert("XDG_CONFIG_HOME".into(), xdg.to_string_lossy().into_owned());
    let cast = fixture(
        &scratch,
        "cast.toml",
        &format!("design_gate = \"{gate}\"\n{CAST_TOML}"),
    );
    run_ok(
        &env,
        &[
            "studio",
            "new",
            "--title",
            "Acme Shop",
            "--repo",
            "/home/user/acme-shop",
            "--cast",
            &cast,
        ],
    );
    let req = fixture(&scratch, "req.md", "# Req\nBuild feature.\n");
    run_ok(
        &env,
        &[
            "req",
            "revise",
            "--as",
            "producer",
            "--file",
            &req,
            "--summary",
            "Initial",
        ],
    );
    let dir = store::studio_dir(&scratch.dir, "acme-shop");
    store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("stage"),
                EventKind::RoleStarted {
                    role: "producer".into(),
                    terminal: Some("bl-acme-shop-producer".into()),
                    actor: Some(actor(runner)),
                    session_id: Some("s-prod".into()),
                    reason: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    if open_waiting {
        advance_to_waiting_user(&scratch, &env);
    }
    (scratch, env, dir)
}

pub(super) fn advance_to_waiting_user(scratch: &Scratch, env: &Env) {
    run_ok(env, &["design", "open", "--as", "producer", "--req", "1"]);
    let w = fixture(scratch, "w.md", "Proposal\n");
    let d = fixture(scratch, "d.md", "Buildability\n");
    let p = fixture(scratch, "p.md", "Summary\n");
    run_ok(
        env,
        &[
            "design", "write", "--as", "writer", "--slot", "proposal", "--file", &w,
        ],
    );
    run_ok(
        env,
        &[
            "design",
            "write",
            "--as",
            "director",
            "--slot",
            "buildability",
            "--file",
            &d,
        ],
    );
    run_ok(
        env,
        &[
            "design", "write", "--as", "producer", "--slot", "summary", "--file", &p,
        ],
    );
}

pub(super) fn role_env(base_env: &Env, role: &str) -> Env {
    let mut vars = base_env.vars.clone();
    vars.insert("BACKLOT_ROLE".into(), role.into());
    Env { vars }
}

pub(super) fn run_hook_json(
    runner: Option<&str>,
    sig: &Path,
    env: &Env,
    cwd: &Path,
    payload: Value,
) {
    let body = payload.to_string();
    let mut args = vec!["--file".into(), sig.to_string_lossy().into_owned()];
    let mut out = Vec::new();
    if let Some("agy") = runner {
        args.extend(["--runner".into(), "agy".into()]);
        let (code, done) = run_io_with_delay(
            &args,
            env,
            cwd,
            Duration::from_millis(1500),
            None,
            &mut Cursor::new(body.as_bytes()),
            &mut out,
        );
        done.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(code, ExitCode::SUCCESS);
        return;
    }
    if let Some(r) = runner {
        args.extend(["--runner".into(), r.into()]);
    }
    let code = run_io_env(
        &args,
        env,
        cwd,
        Duration::from_millis(1500),
        &mut Cursor::new(body.as_bytes()),
        &mut out,
    );
    assert_eq!(code, ExitCode::SUCCESS);
    assert!(out.is_empty());
}

pub(super) fn emit_signal(dir: &Path, ts_ms: u64, event: &str, session_id: &str) {
    let sig = signals_path(dir, "producer");
    fs::create_dir_all(sig.parent().unwrap()).unwrap();
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&sig)
        .unwrap();
    writeln!(
        f,
        r#"{{"ts_ms":{ts_ms},"event":"{event}","session_id":"{session_id}"}}"#
    )
    .unwrap();
}

pub(super) fn term_for(status: Option<&str>) -> LiveTerminal {
    LiveTerminal {
        name: "bl-acme-shop-producer".into(),
        address: "t-prod".into(),
        cwd: "/home/user/acme-shop".into(),
        pid: Some(123),
        status: status.map(|s| Status {
            state: s.into(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

pub(super) async fn sample_prod(
    st: &Stage,
    dir: &Path,
    tm: &LiveTerminal,
    runner: &str,
    now_ms: u64,
) {
    let state = store::load_state(dir).unwrap();
    let w = Watched {
        dir,
        state: &state,
        role: "producer",
        runner,
        term: tm,
    };
    st.sample_work("acme-shop", &w, now_ms).await.unwrap();
}

#[test]
fn stage_ingests_hook_capture_ignores_taint_and_syncs_doc() {
    let rt = runtime();
    let (scratch, env, dir) = setup_studio("required", true, "claude");
    let (st, fake) = rt.block_on(fake_stage(&scratch.dir, env.vars.clone()));
    let prod_env = role_env(&env, "producer");
    let sig = signals_path(&dir, "producer");
    let tm = term_for(Some("waiting"));
    fake.lock().unwrap().screen = "> \n".into();
    let t0 = 1_700_000_000_000u64;

    emit_signal(&dir, t0, "Stop", "s-prod");
    rt.block_on(sample_prod(&st, &dir, &tm, "claude", t0 + 50));

    // Stage-sent turn (tainted) must NOT record as UserInput.
    st.taint_ask("acme-shop", &store::load_state(&dir).unwrap(), "producer");
    run_hook_json(
        None,
        &sig,
        &prod_env,
        &scratch.files,
        json!({"session_id": "s-prod", "hook_event_name": "UserPromptSubmit", "prompt": "synthetic continue"}),
    );
    rt.block_on(sample_prod(&st, &dir, &tm, "claude", t0 + 1_100));
    let round = &store::load_state(&dir).unwrap().design.unwrap().rounds[0];
    assert!(
        !round
            .entries
            .iter()
            .any(|e| e.slot == DesignSlot::UserInput)
    );

    // Settle back to Waiting, then untainted human prompt lands cleanly via EntrySource::Hook.
    emit_signal(&dir, t0 + 1_500, "Stop", "s-prod");
    rt.block_on(sample_prod(&st, &dir, &tm, "claude", t0 + 2_200));
    run_hook_json(
        None,
        &sig,
        &prod_env,
        &scratch.files,
        json!({"session_id": "s-prod", "hook_event_name": "UserPromptSubmit", "prompt": "Adopt token rotation"}),
    );
    rt.block_on(sample_prod(&st, &dir, &tm, "claude", t0 + 3_300));

    let state = store::load_state(&dir).unwrap();
    let ui: Vec<_> = state.design.as_ref().unwrap().rounds[0]
        .entries
        .iter()
        .filter(|e| e.slot == DesignSlot::UserInput)
        .collect();
    assert_eq!(ui.len(), 1, "{ui:?}");
    assert_eq!(ui[0].source, Some(EntrySource::Hook));
    assert!(
        fs::read_to_string(dir.join(&ui[0].file))
            .unwrap()
            .contains("Adopt token rotation")
    );
    let doc = fs::read_to_string(dir.join("design/v1.md")).unwrap();
    assert!(doc.contains("User input · hook"), "{doc}");
    assert!(doc.contains("Adopt token rotation"), "{doc}");
    run_ok(&env, &["design", "next", "--as", "producer"]);
    assert_eq!(store::load_state(&dir).unwrap().design.unwrap().round, 2);
}
