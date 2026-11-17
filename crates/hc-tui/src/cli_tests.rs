//! `cli` unit tests — kept in a sibling file so `cli.rs` stays under
//! the source length limit.
use super::*;
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

    let dir = std::env::temp_dir().join(format!("hc-tui-cli-{}", std::process::id()));
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
    let dir = std::env::temp_dir().join(format!("hc-tui-cli-{}", std::process::id()));
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
