//! The SIGUSR1 path of the diagnostics snapshot. The test owns this
//! process's SIGUSR1 disposition, so its checks run in order in one test.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

use hc_proto::client::AttachConn;
use hc_proto::endpoint::{Endpoint, Env, Local};
use hc_proto::stats::ConnStats;
use hc_tui::app::Probe;
use hc_tui::diag::{Diag, ENV, Sources};
use serde_json::Value;

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

#[test]
fn diagnostics_stay_off_unless_asked_and_sigusr1_writes_when_on() {
    let dir = scratch();
    let off = Diag::start(&Env::new(), sources(Arc::new(Probe::default()), &dir));
    assert!(off.is_none());
    assert_eq!(
        disposition(),
        libc::SIG_DFL,
        "an unset run installed a handler"
    );

    let path = dir.join("snap.json");
    let probe = Arc::new(Probe::default());
    let mut env = Env::new();
    env.insert(ENV.into(), path.clone().into_os_string());
    let on = Diag::start(&env, sources(Arc::clone(&probe), &dir)).expect("diagnostics on");
    probe.wakeups.store(3, Relaxed);
    let sent = unsafe { libc::kill(libc::getpid(), libc::SIGUSR1) };
    assert_eq!(sent, 0);
    let snap = wait_for_snapshot(&path);
    assert_eq!(snap["loop"]["wakeups"]["total"], 3, "{snap}");
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);

    std::fs::remove_file(&path).unwrap();
    on.finish();
    assert_eq!(wait_for_snapshot(&path)["schema"], 1);
    let _ = std::fs::remove_dir_all(&dir);
}
