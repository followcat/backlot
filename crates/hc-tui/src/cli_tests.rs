//! `cli` unit tests — kept in a sibling file so `cli.rs` stays under
//! the source length limit.
use super::*;
use crate::test_support::TestDir;
use hc_proto::endpoint::{Local, Remote};
use std::path::PathBuf;

fn env_with(pairs: &[(&str, &str)]) -> Env {
    pairs
        .iter()
        .map(|(k, v)| (OsString::from(k), OsString::from(v)))
        .collect()
}

fn local(socket: &str) -> Endpoint {
    Endpoint::Local(Local {
        session: "scratch".into(),
        socket: PathBuf::from(socket),
        holder_socket: PathBuf::from(format!("{socket}.holder.sock")),
        state: PathBuf::from("/scratch/state"),
        config: PathBuf::from("/scratch/config.toml"),
    })
}

/// A `--version` line carries the build id only when one exists.
#[test]
fn the_version_line_carries_the_build_when_present() {
    assert_eq!(version_line("0.3.0", None), "backlot 0.3.0");
    assert_eq!(version_line("0.3.0", Some("")), "backlot 0.3.0");
    assert_eq!(
        version_line("0.3.0", Some("abc123def456")),
        "backlot 0.3.0 (abc123def456)"
    );
    assert_eq!(
        version_line("0.3.0", Some("abc123def456-dirty")),
        "backlot 0.3.0 (abc123def456-dirty)"
    );
}

/// The id counts only when socket AND id both match the local core.
#[test]
fn caller_counts_only_on_a_matching_local_socket() {
    let env = env_with(&[
        ("HC_TERMINAL_ID", "t7"),
        ("HC_TERMINAL_SOCKET", "/run/scratch/hc.sock"),
    ]);
    assert_eq!(
        self_caller(&env, &local("/run/scratch/hc.sock")),
        Some("t7".into())
    );
    // A foreign session's socket: the id means nothing there.
    assert_eq!(self_caller(&env, &local("/run/other/hc.sock")), None);
}

/// A remote endpoint never inherits a local caller — an equal `tN`
/// on the other machine is a different terminal.
#[test]
fn caller_never_crosses_a_remote_endpoint() {
    let env = env_with(&[
        ("HC_TERMINAL_ID", "t7"),
        ("HC_TERMINAL_SOCKET", "/run/scratch/hc.sock"),
    ]);
    let remote = Endpoint::Remote(Remote {
        target: "host-a".into(),
        session: "scratch".into(),
    });
    assert_eq!(self_caller(&env, &remote), None);
}

/// `--upgrade` is a first-class action — and near-miss flags still
/// error rather than silently starting the client.
#[test]
fn upgrade_is_a_parse_action() {
    assert!(matches!(
        parse(vec!["--upgrade".to_owned()]).unwrap(),
        Action::Upgrade
    ));
    assert!(parse(vec!["--upgrde".to_owned()]).is_err());
    assert!(INSTALL_COMMAND.contains("install.sh"));
}

/// Both installer constants name backlot's own release script, never
/// hc-tui's: the pipeline the update card runs and the one-liner
/// `--upgrade` prints must not drift apart.
#[test]
fn the_installer_names_backlots_own_release_script() {
    assert!(INSTALL_URL.ends_with("/followcat/backlot/releases/latest/download/install.sh"));
    assert!(INSTALL_COMMAND.contains(INSTALL_URL));
    assert!(INSTALL_COMMAND.ends_with("| sh"));
    for text in [INSTALL_URL, INSTALL_COMMAND] {
        assert!(!text.contains("hc-tui"), "{text}");
        assert!(!text.contains("github.io"), "{text}");
    }
}

/// The studio's watch parameters name no documents in
/// `events.subscribe`: on 0.1.21+ cores that would narrow the
/// subscription to documents alone and silence terminal events. The
/// `backlot` document rides `documents` (the one place it is named),
/// and `doc_patterns` stays empty.
#[test]
fn studio_mode_names_no_documents_in_the_subscription() {
    let params = watch_params();
    assert_eq!(
        params.documents,
        [
            "herdr-app".to_owned(),
            "hc-tui".to_owned(),
            "backlot".to_owned()
        ]
    );
    assert!(params.doc_patterns.is_empty(), "{:?}", params.doc_patterns);
    assert!(
        params.subscribe_params().docs.is_empty(),
        "{:?}",
        params.subscribe_params().docs
    );
    // The raw subscription therefore serializes with no `docs` member.
    let wire = serde_json::to_value(params.subscribe_params()).unwrap();
    assert!(
        wire.get("docs").is_none(),
        "an empty document list must not be named: {wire}"
    );
}

/// Both endpoint kinds multiplex their attaches onto one shared
/// framed connection: a remote's share one ssh child, a local's one
/// Unix socket — either way a burst costs one transport + `hello`.
#[test]
fn local_and_remote_endpoints_get_the_shared_attach_connection() {
    let remote = Endpoint::Remote(Remote {
        target: "host-a".into(),
        session: "scratch".into(),
    });
    let _ = shared_attach(&remote, &None);
    let _ = shared_attach(&local("/run/scratch/hc.sock"), &None);
}

/// Two concurrent attaches on a local endpoint ride one socket: the
/// listener accepts once, `hello` negotiates once, and both
/// `terminal.attach` calls multiplex their own channels over it.
#[cfg(unix)]
#[tokio::test]
async fn local_attaches_share_one_socket_hello_and_two_channels() {
    use hc_proto::codec::Reader;
    use hc_proto::frame::{Frame, Kind};
    use hc_proto::terminal::AttachParams;
    use tokio::io::AsyncWriteExt;

    let dir = TestDir::new("backlot-cli");
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("shared-attach.sock");
    let _ = std::fs::remove_file(&socket);
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let endpoint = local(socket.to_str().unwrap());
    let conn = shared_attach(&endpoint, &None);
    let terminal = serde_json::json!({
        "address": "t7",
        "labels": {},
        "argv": ["sh"],
        "cwd": "/home/user",
        "state": "live",
        "created_ms": 1,
        "screen": "exact",
        "cols": 80,
        "rows": 24,
    });
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, mut write) = stream.into_split();
        let mut reader = Reader::new(read);
        let hello: serde_json::Value =
            serde_json::from_slice(&reader.read_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(hello["method"], "hello");
        let mut reply = serde_json::to_vec(&serde_json::json!({
            "jsonrpc": "2.0", "id": 0,
            "result": {
                "generation": 1, "version": "fake 0", "framed": true,
                "core": {"id": "c", "name": "c"},
                "methods": ["terminal.attach"], "features": [],
            }
        }))
        .unwrap();
        reply.push(b'\n');
        write.write_all(&reply).await.unwrap();
        // Both attach calls arrive on channel 0 of the same conn —
        // each answer names the stream channel it was allocated.
        for channel in 1u32..=2 {
            loop {
                let frame = reader.read_frame().await.unwrap().unwrap();
                if frame.channel == 0 && frame.kind == Kind::Data {
                    let request: serde_json::Value =
                        serde_json::from_slice(&frame.payload).unwrap();
                    assert_eq!(request["method"], "terminal.attach");
                    let reply = Frame::data(
                        0,
                        serde_json::to_vec(&serde_json::json!({
                            "jsonrpc": "2.0", "id": request["id"],
                            "result": {
                                "channel": channel, "terminal": &terminal,
                                "resize_notifications": true,
                                "automatic_input": false,
                            }
                        }))
                        .unwrap(),
                    );
                    write.write_all(&reply.encode()).await.unwrap();
                    break;
                }
            }
        }
        // No second connection may open for a second attach.
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(200), listener.accept())
                .await
                .is_err(),
            "a second attach opened its own socket"
        );
    });
    let params = |observe: bool| AttachParams {
        terminal: "t7".into(),
        observe,
        resize_notifications: true,
        quiet_human_release: !observe,
        history: 0,
        cols: 80,
        rows: 24,
        display: None,
        resumed: false,
        caller: None,
    };
    let active_params = params(false);
    let observer_params = params(true);
    let (active, observer) =
        tokio::join!(conn.attach(&active_params), conn.attach(&observer_params));
    let active = active.expect("active attach");
    let observer = observer.expect("observer attach");
    assert_ne!(active.channel(), observer.channel(), "two channels");
    server.await.unwrap();
    let _ = std::fs::remove_file(&socket);
}

/// The opener `run` hands to both the watcher and the operations
/// worker is the shared one: concurrent opens — as the watcher,
/// store and seen workers race them at startup — accept one
/// socket, never a connection each. The remote path takes the same
/// helper over ssh.
#[cfg(unix)]
#[tokio::test]
async fn shared_rpc_opens_one_connection_for_every_consumer() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let dir = TestDir::new("backlot-cli");
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("shared-rpc.sock");
    let _ = std::fs::remove_file(&socket);
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let endpoint = local(socket.to_str().unwrap());
    let rpc = shared_rpc(&endpoint, &None, Arc::default());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut hello = Vec::new();
        let mut buf = [0u8; 1024];
        loop {
            match stream.read(&mut buf).await.unwrap() {
                0 => panic!("the client hung up before hello"),
                n => {
                    hello.extend_from_slice(&buf[..n]);
                    if hello.contains(&b'\n') {
                        break;
                    }
                }
            }
        }
        let mut reply = serde_json::to_vec(&serde_json::json!({
            "jsonrpc": "2.0", "id": 0,
            "result": {
                "generation": 1, "version": "fake 0", "framed": true,
                "core": {"id": "c", "name": "c"}, "methods": [], "features": [],
            }
        }))
        .unwrap();
        reply.push(b'\n');
        stream.write_all(&reply).await.unwrap();
        // No second consumer may open its own connection.
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(200), listener.accept())
                .await
                .is_err(),
            "a second connection opened on a shared opener"
        );
    });
    let (a, b, c) = tokio::join!(rpc(), rpc(), rpc());
    assert!(a.is_ok() && b.is_ok() && c.is_ok());
    server.await.unwrap();
    let _ = std::fs::remove_file(&socket);
}

/// Missing socket proof (or missing id) is not a caller.
#[test]
fn caller_needs_the_verifying_socket_var() {
    let only_id = env_with(&[("HC_TERMINAL_ID", "t7")]);
    assert_eq!(self_caller(&only_id, &local("/run/scratch/hc.sock")), None);
    let only_socket = env_with(&[("HC_TERMINAL_SOCKET", "/run/scratch/hc.sock")]);
    assert_eq!(
        self_caller(&only_socket, &local("/run/scratch/hc.sock")),
        None
    );
}

#[test]
fn upgrade_installs_into_the_running_binary_directory_unless_configured() {
    use crate::update_install::upgrade_install_dir;
    let exe = std::path::Path::new("/home/user/bin/backlot");
    let dir = std::path::PathBuf::from("/home/user/bin");
    assert_eq!(upgrade_install_dir(None, Some(exe)), Some(dir.clone()));
    assert_eq!(
        upgrade_install_dir(Some(std::ffi::OsStr::new("")), Some(exe)),
        Some(dir)
    );
    assert_eq!(
        upgrade_install_dir(Some(std::ffi::OsStr::new("/opt/hc")), Some(exe)),
        None
    );
}

/// Issue #44: a package-managed or dev-build `--upgrade` refuses before
/// the installer runs, naming its own update command.
#[test]
fn managed_and_dev_build_upgrades_are_refused_with_their_own_commands() {
    let managed = [
        (
            crate::install_source::InstallSource::Mise {
                tool: "github:followcat/backlot".into(),
            },
            "mise upgrade --bump github:followcat/backlot",
        ),
        (
            crate::install_source::InstallSource::Homebrew,
            "brew upgrade backlot",
        ),
        (
            crate::install_source::InstallSource::Nix,
            "Nix configuration",
        ),
        (crate::install_source::InstallSource::Cargo, "cargo"),
        (crate::install_source::InstallSource::DevBuild, "dev build"),
    ];
    for (source, expected) in managed {
        let ran = std::cell::Cell::new(false);
        let status = upgrade_with(&source, None, None, &|_| {
            ran.set(true);
            0
        });
        assert_eq!(status, 1, "{source:?}");
        assert!(!ran.get(), "{source:?} must not run the installer");
        assert!(
            source.refusal().unwrap().contains(expected),
            "{source:?} refusal names its command"
        );
    }
}

/// An explicit `BACKLOT_INSTALL_DIR` is the user's own choice: the
/// installer runs as asked even on a managed install.
#[test]
fn an_explicit_install_dir_runs_the_installer_as_asked() {
    let ran = std::cell::Cell::new(false);
    let status = upgrade_with(
        &crate::install_source::InstallSource::Mise {
            tool: "github:followcat/backlot".into(),
        },
        Some(std::ffi::OsStr::new("/opt/hc")),
        Some(std::path::PathBuf::from("/home/user/bin")),
        &|dir| {
            ran.set(true);
            assert_eq!(dir, None, "the env var already names the dir");
            0
        },
    );
    assert_eq!(status, 0);
    assert!(ran.get());
}

/// A direct install still upgrades the running binary's directory; an
/// empty `BACKLOT_INSTALL_DIR` counts as unset.
#[test]
fn a_direct_upgrade_installs_into_the_running_directory() {
    let dir = std::path::Path::new("/home/user/bin");
    for configured in [None, Some(std::ffi::OsStr::new(""))] {
        let ran = std::cell::Cell::new(false);
        let status = upgrade_with(
            &crate::install_source::InstallSource::Direct,
            configured,
            Some(dir.to_path_buf()),
            &|got| {
                ran.set(true);
                assert_eq!(got.as_deref(), Some(dir));
                0
            },
        );
        assert_eq!(status, 0);
        assert!(ran.get());
    }
}
