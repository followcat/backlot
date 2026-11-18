//! The SIGUSR1 path of the diagnostics snapshot, taken while a real run
//! loop is busy. The test owns this process's SIGUSR1 disposition, so its
//! checks run in order inside one test.

#[path = "common/loopcore.rs"]
mod loopcore;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hc_proto::client::AttachConn;
use hc_proto::endpoint::{Endpoint, Env, Local};
use hc_proto::stats::ConnStats;
use hc_proto::watch::{OpenRpc, WatchParams, Watcher};
use hc_tui::app::{App, Probe, UiEvent};
use hc_tui::diag::{Diag, ENV, Sources};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::{Value, json};
use tokio::sync::mpsc;

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hc-tui-diag-signal-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn sources(probe: Arc<Probe>, dir: &Path) -> Sources {
    let endpoint = Endpoint::Local(Local {
        session: "hctui-test-diag".into(),
        socket: dir.join("core.sock"),
        holder_socket: dir.join("core.holder.sock"),
        state: dir.to_path_buf(),
        config: dir.join("config.toml"),
    });
    Sources::new(
        probe,
        &endpoint,
        Arc::new(ConnStats::default()),
        Arc::new(ConnStats::default()),
        AttachConn::new(endpoint.clone(), None),
    )
}

fn disposition() -> libc::sighandler_t {
    let mut old: libc::sigaction = unsafe { std::mem::zeroed() };
    let status = unsafe { libc::sigaction(libc::SIGUSR1, std::ptr::null(), &mut old) };
    assert_eq!(status, 0);
    old.sa_sigaction
}

fn wait_for_snapshot(path: &Path) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(bytes) = std::fs::read(path)
            && let Ok(value) = serde_json::from_slice(&bytes)
        {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "no snapshot at {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Runs the real loop with two panes. One input event is sent and drawn,
/// then `pulse` runs while the loop is still live; the input then closes
/// and the loop returns.
async fn run_busy_loop(probe: Arc<Probe>, pulse: impl FnOnce() + Send + 'static) {
    let (sub_tx, mut subs) = mpsc::channel(4);
    let (attach_tx, mut attaches) = mpsc::channel(8);
    let rpc: OpenRpc = Arc::new(|| Box::pin(std::future::pending()));
    let watcher = Watcher::with_openers(rpc, loopcore::sub_opens(sub_tx), WatchParams::default());
    let config = hc_tui::config::Config::default();
    let shell = hc_tui::chrome::ClientShellConfig::from_config(
        &config,
        None,
        None,
        std::path::PathBuf::new(),
    );
    let mut app = App::new(
        watcher,
        loopcore::attach_opens(attach_tx),
        config.live_keybinds(),
        shell,
        Arc::clone(&probe),
    );
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let (ui_tx, mut ui) = mpsc::channel(8);
    let (_reply_tx, mut replies) = mpsc::channel(1);
    let driver = tokio::spawn(async move {
        let _session = loopcore::Session::connect(
            &mut subs,
            &mut attaches,
            4,
            json!({
                "seq": 1,
                "terminals": [
                    loopcore::terminal_json("t1", "one", 1),
                    loopcore::terminal_json("t2", "one", 2),
                ],
            }),
        )
        .await;
        let settled = loopcore::settled_draws(&probe).await;
        ui_tx
            .send(UiEvent::InputRejected("counted input"))
            .await
            .unwrap();
        loopcore::draws_beyond(&probe, settled).await;
        pulse();
        drop(ui_tx);
    });
    tokio::time::timeout(
        Duration::from_secs(15),
        app.run(&mut terminal, &mut ui, &mut replies),
    )
    .await
    .expect("the loop must stay responsive")
    .unwrap();
    driver.await.unwrap();
}

#[tokio::test]
async fn diagnostics_stay_off_unless_asked_and_sigusr1_writes_when_on() {
    let dir = scratch();
    let path = dir.join("snap.json");
    assert_eq!(disposition(), libc::SIG_DFL);

    let off = Diag::start(&Env::new(), sources(Arc::new(Probe::default()), &dir));
    assert!(off.is_none());
    run_busy_loop(Arc::new(Probe::default()), || {}).await;
    assert!(!path.exists(), "an unset run wrote a snapshot");
    assert_eq!(
        disposition(),
        libc::SIG_DFL,
        "an unset run installed a handler"
    );

    let probe = Arc::new(Probe::default());
    let mut env = Env::new();
    env.insert(ENV.into(), path.clone().into_os_string());
    let on = Diag::start(&env, sources(Arc::clone(&probe), &dir)).expect("diagnostics on");
    let pulse_path = path.clone();
    run_busy_loop(probe, move || {
        let sent = unsafe { libc::kill(libc::getpid(), libc::SIGUSR1) };
        assert_eq!(sent, 0);
        let snap = wait_for_snapshot(&pulse_path);
        assert!(
            snap["loop"]["wakeups"]["total"].as_u64().unwrap() > 0,
            "{snap}"
        );
        assert!(
            snap["loop"]["wakeups"]["input"].as_u64().unwrap() >= 1,
            "{snap}"
        );
        let mode = std::fs::metadata(&pulse_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    })
    .await;

    std::fs::remove_file(&path).unwrap();
    on.finish();
    assert_eq!(wait_for_snapshot(&path)["schema"], 1);
    let _ = std::fs::remove_dir_all(&dir);
}
