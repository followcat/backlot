//! Fail-first and verification tests for `<state_dir>/accounts.json`.

use std::collections::BTreeSet;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use backlot_model::event::{ActorIdentity, EventKind, RoleConfigs, StudioMode};
use backlot_model::summary::QuotaWindow;
use backlot_model::{EventBy, NewEvent};
use hc_proto::client::Client;
use tokio::io::{AsyncWriteExt, DuplexStream};

use super::*;
use crate::daemon::Stage;
use crate::reconcile::LiveTerminal;
use crate::sample::Watched;
use crate::store::{self, Pending};
use crate::test_support::TestDir;

fn actor(runner: &str, model: &str) -> ActorIdentity {
    ActorIdentity {
        runner: runner.into(),
        runner_version: None,
        model: model.into(),
        effort: None,
        permission_profile: "standard".into(),
    }
}

fn studio_dir_with_started_role(base: &Path, name: &str, role: &str, runner: &str) -> PathBuf {
    let dir = store::studio_dir(base, name);
    fs::create_dir_all(&dir).unwrap();
    store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("user"),
                EventKind::StudioCreated {
                    title: name.into(),
                    repo: "/repo".into(),
                    members: Vec::new(),
                    no_git: true,
                    mode: StudioMode::Standard,
                    roles: Box::new(RoleConfigs {
                        producer: actor(runner, "opus-5-5"),
                        director: actor(runner, "sonnet-5"),
                        writer: actor(runner, "sonnet-5"),
                    }),
                    candidates: Vec::new(),
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
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("stage"),
                EventKind::RoleStarted {
                    role: role.into(),
                    terminal: Some(format!("bl-{name}-{role}")),
                    actor: Some(actor(runner, "sonnet-5")),
                    session_id: Some("sess-1".into()),
                    reason: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    dir
}

async fn dummy_stage(base: &Path) -> Stage {
    async fn serve_min(stream: DuplexStream) {
        use hc_proto::codec::Reader;
        let (read, mut write) = tokio::io::split(stream);
        let mut reader = Reader::new(read);
        let _ = reader.read_line().await;
        let reply = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 0,
            "result": {
                "generation": 1,
                "version": "fake-core 0.0",
                "framed": true,
                "core": {"id": "core-fake-1", "name": "fake-core"},
                "methods": [],
                "features": [],
            }
        })
        .to_string();
        let _ = write.write_all(reply.as_bytes()).await;
        let _ = write.write_all(b"\n").await;
        while reader.read_frame().await.ok().flatten().is_some() {}
    }

    let (core_stream, client_stream) = tokio::io::duplex(4096);
    tokio::spawn(serve_min(core_stream));
    let (read, write) = tokio::io::split(client_stream);
    let client = Client::open(Box::pin(read), Box::pin(write), None)
        .await
        .unwrap();
    let mut stage = Stage::for_test(client);
    stage.base = base.to_path_buf();
    stage.state_dir = base.to_string_lossy().into_owned();
    stage
}

#[test]
fn missing_or_corrupt_accounts_file_tolerates_cleanly_and_saves_atomically() {
    let dir = TestDir::new("backlot-accounts-store");

    // Missing file: empty default, no error.
    let (loaded, warn) = load(&dir);
    assert_eq!(warn, None);
    assert!(loaded.accounts.is_empty());

    // Corrupt file: tolerated without crashing and reports diagnostic string.
    fs::write(path(&dir), "{ not valid json").unwrap();
    let (loaded, warn) = load(&dir);
    assert!(warn.is_some(), "expected warning for corrupt accounts.json");
    assert!(loaded.accounts.is_empty());

    // Observe roles + record quota stall -> atomic write leaves no temp files behind.
    let mut file = AccountsFile::default();
    assert!(file.observe_role(
        "claude:default",
        "claude",
        "default",
        "hollywood",
        "hollywood",
        "opus-5-5"
    ));
    assert!(file.observe_role(
        "claude:default",
        "claude",
        "default",
        "acme-shop",
        "actor-t2-c1",
        "sonnet-5"
    ));
    assert!(file.record_stall(
        "claude:default",
        "claude",
        "default",
        "acme-shop/actor-t2-c1",
        1_795_651_200_000,
        Some(1_795_669_200_000),
        QuotaWindow::Short,
        "waiting until 14:10",
    ));
    save(&dir, &file).unwrap();

    let (reloaded, warn2) = load(&dir);
    assert_eq!(warn2, None);
    assert_eq!(reloaded.schema, ACCOUNTS_SCHEMA);
    let entry = reloaded
        .accounts
        .get("claude:default")
        .expect("saved claude:default");
    assert!(entry.stalled);
    assert_eq!(entry.stalled_since_ms, Some(1_795_651_200_000));
    assert_eq!(entry.last_stall_ms, Some(1_795_651_200_000));
    assert_eq!(entry.reset_at_ms, Some(1_795_669_200_000));
    assert_eq!(entry.window, Some(QuotaWindow::Short));
    assert_eq!(entry.stalled_by, vec!["acme-shop/actor-t2-c1"]);
    assert_eq!(entry.roles.len(), 2);

    // No temp siblings remain and mode is 0o600 on unix.
    let entries: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(entries, vec![ACCOUNTS_FILE.to_string()]);
    #[cfg(unix)]
    {
        let mode = fs::metadata(path(&dir)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}

#[test]
fn stalled_clears_by_reset_time_recovery_and_retains_last_stall() {
    let mut file = AccountsFile::default();
    let t0 = 1_795_651_200_000u64;
    let reset = t0 + 4 * 3_600_000;
    assert!(file.record_stall(
        "claude:default",
        "claude",
        "default",
        "acme-shop/actor-t2-c1",
        t0,
        Some(reset),
        QuotaWindow::Short,
        "5h limit; resets 14:10",
    ));
    assert!(file.record_stall(
        "claude:default",
        "claude",
        "default",
        "hollywood",
        t0 + 1_000,
        Some(reset),
        QuotaWindow::Short,
        "5h limit; resets 14:10",
    ));
    // Clearing only one role keeps account stalled until the last role recovers.
    assert!(file.clear_role_stall("acme-shop/actor-t2-c1"));
    assert!(file.accounts["claude:default"].stalled);
    assert_eq!(file.accounts["claude:default"].stalled_by, vec!["hollywood"]);

    assert!(file.clear_role_stall("hollywood"));
    let entry = &file.accounts["claude:default"];
    assert!(!entry.stalled);
    assert_eq!(entry.stalled_since_ms, None);
    assert_eq!(entry.last_stall_ms, Some(t0 + 1_000));
    assert_eq!(entry.reset_at_ms, Some(reset));

    // Re-stall and clear via reset time passing (`now_ms >= reset_at_ms`).
    assert!(file.record_stall(
        "claude:default",
        "claude",
        "default",
        "acme-shop/actor-t2-c1",
        t0 + 2_000,
        Some(reset),
        QuotaWindow::Short,
        "5h limit",
    ));
    let active = BTreeSet::from(["acme-shop/actor-t2-c1".to_string()]);
    assert!(!file.reconcile(reset - 1, &active, None));
    assert!(file.accounts["claude:default"].stalled);

    assert!(file.reconcile(reset, &active, None));
    let entry = &file.accounts["claude:default"];
    assert!(!entry.stalled);
    assert!(entry.stalled_by.is_empty());
    assert_eq!(entry.last_stall_ms, Some(t0 + 2_000));
    assert_eq!(entry.reset_at_ms, Some(reset));
}

#[tokio::test]
async fn stage_restart_and_manual_attention_clear_sync_accounts_json() {
    let base = TestDir::new("backlot-accounts-restart");
    let dir = studio_dir_with_started_role(&base, "acme-shop", "actor-t1-c1", "claude");
    let st1 = dummy_stage(&base).await;

    let t0 = {
        let ts = ((86_400_000u64 * 20_000) / 1000) as libc::time_t;
        let mut local: libc::tm = unsafe { std::mem::zeroed() };
        unsafe { libc::localtime_r(&ts, &mut local) };
        local.tm_hour = 9;
        local.tm_min = 10;
        local.tm_sec = 0;
        local.tm_isdst = -1;
        (unsafe { libc::mktime(&mut local) } as u64) * 1000
    };
    let expected_reset = t0 + 5 * 3_600_000;

    let term = LiveTerminal {
        address: "t1".into(),
        status: Some(hc_proto::terminal::Status {
            state: "waiting".into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let state = store::load_state(&dir).unwrap();
    let watched = Watched {
        dir: &dir,
        state: &state,
        role: "actor-t1-c1",
        runner: "claude",
        term: &term,
    };
    st1.rate_limit(
        "acme-shop",
        &watched,
        "Session limit reached · resets 2:10pm\nContinuing automatically at 2:10pm\n",
        backlot_model::summary::Work::Waiting,
        t0,
    )
    .await
    .unwrap();

    // Persisted immediately to `<state_dir>/accounts.json`.
    let (disk1, _) = load(&base);
    let acct1 = disk1
        .accounts
        .get("claude:default")
        .expect("rate_limit must persist claude:default stall");
    assert!(acct1.stalled);
    assert_eq!(acct1.reset_at_ms, Some(expected_reset));
    assert_eq!(acct1.stalled_by, vec!["acme-shop/actor-t1-c1"]);

    // Fresh Stage instance (simulating stage daemon restart) reloads accounts.json
    // and keeps `stalled: true` past the 1h clamp while before `expected_reset`.
    let mut st2 = dummy_stage(&base).await;
    st2.sync_accounts_at(t0 + 65 * 60_000);
    let (disk2, _) = load(&base);
    assert!(disk2.accounts["claude:default"].stalled);
    assert_eq!(disk2.accounts["claude:default"].reset_at_ms, Some(expected_reset));

    // Manual `attention clear <seq>` clears the persisted stall too.
    let seq = store::load_state(&dir)
        .unwrap()
        .open_attention()
        .next()
        .unwrap()
        .seq;
    let caller = crate::cli::Caller {
        studio: "acme-shop".into(),
        role: "user".into(),
        task: None,
        session: None,
        via: None,
    };
    crate::verbs::attention_clear(&base, &caller, seq, true).unwrap();
    let (disk3, _) = load(&base);
    assert!(!disk3.accounts["claude:default"].stalled);
    assert_eq!(disk3.accounts["claude:default"].last_stall_ms, Some(t0));
    assert_eq!(disk3.accounts["claude:default"].reset_at_ms, Some(expected_reset));
}
