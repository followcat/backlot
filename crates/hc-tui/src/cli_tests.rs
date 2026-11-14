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

/// Only a remote endpoint's attaches multiplex onto the shared
/// framed connection; a local endpoint keeps `Attach::connect`.
#[test]
fn only_a_remote_endpoint_gets_the_shared_attach_connection() {
    let remote = Endpoint::Remote(Remote {
        target: "host-a".into(),
        session: "scratch".into(),
    });
    assert!(shared_attach(&remote, &None).is_some());
    assert!(shared_attach(&local("/run/scratch/hc.sock"), &None).is_none());
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
    let rpc = shared_rpc(&endpoint, &None);
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
