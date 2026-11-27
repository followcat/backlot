//! The installer pipeline and its process group, against real local
//! processes (`sh`, `sleep`, `echo`) — never the network.
use super::*;
use crate::test_support::TestDir;
use std::time::Instant;

#[test]
fn the_plan_pins_the_version_skips_the_core_and_names_the_directory() {
    let plan = plan(Path::new("/home/user/bin"), "0.1.13");
    assert_eq!(
        plan.pipeline,
        "curl -fsSL https://github.com/followcat/backlot/releases/latest/download/install.sh | BACKLOT_VERSION=v0.1.13 BACKLOT_SKIP_CORE=1 BACKLOT_INSTALL_DIR=/home/user/bin sh"
    );
    assert_eq!(
        plan.env,
        [
            ("BACKLOT_VERSION", "v0.1.13".to_owned()),
            ("BACKLOT_SKIP_CORE", "1".to_owned()),
            ("BACKLOT_INSTALL_DIR", "/home/user/bin".to_owned()),
        ]
    );
    assert_eq!(plan.bin, PathBuf::from("/home/user/bin/backlot"));
    assert_eq!(plan.echo[2], "    BACKLOT_INSTALL_DIR=/home/user/bin sh");
}

#[test]
fn a_directory_with_a_quote_stays_one_shell_word() {
    let plan = plan(Path::new("/home/user/it's"), "0.1.13");
    assert!(
        plan.pipeline
            .ends_with("BACKLOT_INSTALL_DIR='/home/user/it'\\''s' sh"),
        "{}",
        plan.pipeline
    );
}

#[tokio::test]
async fn output_streams_and_the_exit_code_follows() {
    let (tx, mut rx) = mpsc::channel(16);
    spawn_group("echo installed; echo curl: boom 1>&2; exit 3", tx, 7).unwrap();
    let mut lines = Vec::new();
    let mut exit = None;
    while let Some(event) = rx.recv().await {
        match event {
            UpdateEvent::Output { generation, line } => {
                assert_eq!(generation, 7);
                lines.push(line);
            }
            UpdateEvent::Exited { generation, code } => {
                assert_eq!(generation, 7);
                exit = Some(code);
                break;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    lines.sort();
    assert_eq!(lines, ["curl: boom", "installed"]);
    assert_eq!(exit, Some(Some(3)));
}

#[tokio::test]
async fn sigterm_reaches_every_process_in_the_group() {
    let (tx, mut rx) = mpsc::channel(16);
    let pgid = spawn_group("sleep 30 & sleep 30; echo never", tx, 1).unwrap();
    assert!(pgid > 0);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(group_alive(pgid), "the group is running before the signal");
    signal_group(pgid, libc::SIGTERM);
    let deadline = Instant::now() + Duration::from_secs(2);
    while group_alive(pgid) && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(!group_alive(pgid), "no member survives SIGTERM");
    let exit = tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(event) = rx.recv().await {
            if let UpdateEvent::Exited { code, .. } = event {
                return code;
            }
        }
        None
    })
    .await
    .expect("the exit event arrives");
    assert_eq!(exit, None, "the shell died from a signal, not a status");
}

#[tokio::test]
async fn a_group_that_survives_the_grace_period_is_killed() {
    let signals = std::sync::Mutex::new(Vec::new());
    reap_after(
        4242,
        Duration::ZERO,
        |_| true,
        |pgid, signal| signals.lock().unwrap().push((pgid, signal)),
    )
    .await;
    assert_eq!(*signals.lock().unwrap(), [(4242, libc::SIGKILL)]);

    let signals = std::sync::Mutex::new(Vec::new());
    reap_after(
        4242,
        Duration::ZERO,
        |_| false,
        |pgid, signal| signals.lock().unwrap().push((pgid, signal)),
    )
    .await;
    assert!(
        signals.lock().unwrap().is_empty(),
        "a group that exited is left alone"
    );
}

/// A binary that is still open for writing makes `exec` fail `ETXTBSY`
/// (a freshly written file whose descriptor another thread's forked
/// child still holds). The probe waits for the writer instead of
/// reporting no version.
#[tokio::test]
async fn the_version_probe_waits_out_a_binary_still_open_for_writing() {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    let dir = TestDir::new("hctui-probe-busy");
    std::fs::create_dir_all(&dir).unwrap();
    let bin = dir.join("backlot");
    let mut writer = std::fs::File::create(&bin).unwrap();
    writer
        .write_all(b"#!/bin/sh\necho 'backlot 0.1.14'\n")
        .unwrap();
    writer.flush().unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let closer = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(40)).await;
        drop(writer);
    });
    assert_eq!(probe_version(bin).await.as_deref(), Some("backlot 0.1.14"));
    closer.await.unwrap();
}

#[tokio::test]
async fn the_version_probe_reads_the_binary_and_a_failure_is_none() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TestDir::new("hctui-probe");
    std::fs::create_dir_all(&dir).unwrap();
    let ok = dir.join("backlot");
    std::fs::write(&ok, "#!/bin/sh\necho 'backlot 0.1.13'\n").unwrap();
    std::fs::set_permissions(&ok, std::fs::Permissions::from_mode(0o755)).unwrap();
    let bad = dir.join("backlot-broken");
    std::fs::write(&bad, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&bad, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(probe_version(ok).await.as_deref(), Some("backlot 0.1.13"));
    assert_eq!(
        probe_version(bad).await,
        None,
        "a failing check is no version"
    );
    assert_eq!(
        probe_version(dir.join("missing")).await,
        None,
        "a missing binary is no version"
    );
}

/// A hostile version string still lands in the pipeline as one quoted
/// shell word, exactly like a directory with a quote (review round 1).
#[test]
fn a_hostile_version_still_quotes_to_one_shell_word() {
    let plan = plan(Path::new("/home/user/bin"), "9.9.9.;id;");
    assert!(
        plan.pipeline.contains("BACKLOT_VERSION='v9.9.9.;id;'"),
        "{}",
        plan.pipeline
    );
    assert_eq!(
        plan.env,
        [
            ("BACKLOT_VERSION", "v9.9.9.;id;".to_owned()),
            ("BACKLOT_SKIP_CORE", "1".to_owned()),
            ("BACKLOT_INSTALL_DIR", "/home/user/bin".to_owned()),
        ],
        "the env list keeps the raw value — only the pipeline quotes"
    );
}
