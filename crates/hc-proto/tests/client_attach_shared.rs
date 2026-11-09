//! The shared attach connection: many `Attach` handles on one injected
//! transport — channel routing, per-channel teardown and failure
//! containment, single-flight connect and rebuild — plus the fake-ssh
//! proof that two opens spawn exactly one child and one `hello`.

mod common;

use common::attach::{DEADLINE, joined, observer, params};
use common::fakecore::{ALL_METHODS, DuplexCore, attach_result, pair};
use common::minimal_terminal;
use hc_proto::client::{AttachConn, ConnectParts, Output};
use hc_proto::frame::{Frame, Kind};
use serde_json::json;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

/// An `AttachConn` over injected halves: each `connect` pops the next
/// prepared transport; `connects` counts how many were taken.
fn shared(pending: Arc<Mutex<VecDeque<ConnectParts>>>, connects: Arc<AtomicU64>) -> AttachConn {
    AttachConn::with_connector(move || {
        let pending = Arc::clone(&pending);
        let connects = Arc::clone(&connects);
        async move {
            connects.fetch_add(1, Ordering::SeqCst);
            let parts = pending
                .lock()
                .unwrap()
                .pop_front()
                .expect("a connect with no prepared transport");
            Ok(parts)
        }
    })
}

/// A one-transport shared connection plus its fake core.
fn shared_once() -> (AttachConn, DuplexCore, Arc<AtomicU64>) {
    let (core, read, write) = pair(64 * 1024);
    let connects = Arc::new(AtomicU64::new(0));
    let conn = shared(
        Arc::new(Mutex::new(VecDeque::from([ConnectParts {
            read,
            write,
            guard: None,
        }]))),
        Arc::clone(&connects),
    );
    (conn, core, connects)
}

/// Replies to `req` with its attach result plus `paint` in one write —
/// the reply and the first stream bytes land in a single read.
fn reply_and_paint(req: &serde_json::Value, channel: u32, paint: &[u8]) -> Vec<u8> {
    let reply = Frame::data(
        0,
        serde_json::to_vec(&json!({
            "jsonrpc": "2.0", "id": req["id"],
            "result": attach_result(channel, minimal_terminal(), false, false),
        }))
        .unwrap(),
    );
    let mut write = reply.encode();
    write.extend_from_slice(&Frame::data(channel, paint.to_vec()).encode());
    write
}

#[tokio::test]
async fn two_opens_share_one_connect_one_hello_and_first_paint_survives() {
    let (conn, mut core, connects) = shared_once();
    let server = tokio::spawn(async move {
        core.serve_hello(ALL_METHODS).await;
        let req = core.read_request().await;
        assert_eq!(req["method"], "terminal.attach");
        let paint1 = reply_and_paint(&req, 1, b"paint-one");
        core.send(&paint1).await;
        let req = core.read_request().await;
        assert_eq!(req["method"], "terminal.attach");
        let paint2 = reply_and_paint(&req, 2, b"paint-two");
        core.send(&paint2).await;
        core.drain_until_eof().await;
        core
    });
    let p = params();
    let o = observer();
    let (a, b) = tokio::join!(conn.attach(&p), conn.attach(&o));
    let a = a.expect("attach one");
    let b = b.expect("attach two");
    assert_eq!(connects.load(Ordering::SeqCst), 1, "two opens, one connect");
    assert_eq!((a.channel(), b.channel()), (1, 2), "one conn, two channels");
    // Frames written right after each attach reply are held per channel
    // until install — the first paint is never lost.
    assert_eq!(a.recv().await, Some(Output::Data(b"paint-one".to_vec())));
    assert_eq!(b.recv().await, Some(Output::Data(b"paint-two".to_vec())));
    drop(a);
    drop(b);
    joined(server).await;
}

#[tokio::test]
async fn the_same_terminal_attaches_active_and_observing_on_one_connection() {
    let (conn, mut core, connects) = shared_once();
    let server = tokio::spawn(async move {
        core.serve_hello(ALL_METHODS).await;
        // One `terminal.attach` per open; the active and the observer of
        // the same address get their own channels on the one connection.
        let mut seen = (None, None);
        for _ in 0..2 {
            let req = core.read_request().await;
            assert_eq!(req["method"], "terminal.attach");
            assert_eq!(req["params"]["terminal"], "t7");
            if req["params"]["observe"] == true {
                core.reply(
                    &req["id"],
                    attach_result(9, minimal_terminal(), true, false),
                )
                .await;
                seen.1 = Some(req);
            } else {
                core.reply(
                    &req["id"],
                    attach_result(4, minimal_terminal(), false, false),
                )
                .await;
                seen.0 = Some(req);
            }
        }
        assert_eq!(seen.0.unwrap()["params"]["observe"], json!(null));
        core.send_stream(4, 0, b"active-bytes").await;
        core.send_stream(9, 0, b"observer-bytes").await;
        // Each keeps working while the other's channel ends.
        let close = core.read_frame().await;
        assert_eq!((close.channel, close.kind), (4, Kind::Close));
        core.send_stream(9, 0, b"observer-more").await;
        core.drain_until_eof().await;
        core
    });
    let p = params();
    let o = observer();
    let (active, obs) = tokio::join!(conn.attach(&p), conn.attach(&o));
    let active = active.expect("active attach");
    let obs = obs.expect("observer attach");
    assert_eq!(connects.load(Ordering::SeqCst), 1);
    assert_eq!((active.channel(), obs.channel()), (4, 9));
    assert_eq!(
        active.recv().await,
        Some(Output::Data(b"active-bytes".to_vec()))
    );
    assert_eq!(
        obs.recv().await,
        Some(Output::Data(b"observer-bytes".to_vec()))
    );
    drop(active);
    assert_eq!(
        obs.recv().await,
        Some(Output::Data(b"observer-more".to_vec()))
    );
    drop(obs);
    joined(server).await;
}

#[tokio::test]
async fn detach_and_drop_close_only_their_own_channel() {
    let (conn, mut core, _) = shared_once();
    let server = tokio::spawn(async move {
        core.serve_hello(ALL_METHODS).await;
        for channel in [1u32, 2] {
            let req = core.read_request().await;
            core.reply(
                &req["id"],
                attach_result(channel, minimal_terminal(), false, false),
            )
            .await;
        }
        // Dropping attach 1 sends its CLOSE while the conn lives on.
        let close = core.read_frame().await;
        assert_eq!((close.channel, close.kind), (1, Kind::Close));
        core.send_stream(2, 0, b"still-alive").await;
        // Detaching attach 2 sends exactly channel 2's CLOSE; its ack
        // closes the stream, and the last release ends the connection.
        let close = core.read_frame().await;
        assert_eq!((close.channel, close.kind), (2, Kind::Close));
        core.send_stream(2, 2, b"").await;
        core.expect_eof().await;
    });
    let p = params();
    let (a, b) = tokio::join!(conn.attach(&p), conn.attach(&p));
    let (a, b) = (a.unwrap(), b.unwrap());
    drop(a);
    assert_eq!(b.recv().await, Some(Output::Data(b"still-alive".to_vec())));
    let ended = b.detach().await.expect("detach");
    assert_eq!(ended.reason, Some(hc_proto::client::CloseReason::Empty));
    joined(server).await;
}

#[tokio::test]
async fn a_cancelled_open_closes_its_late_reply_channel() {
    let (conn, mut core, _) = shared_once();
    let (req_seen, req_seen_rx) = oneshot::channel::<()>();
    let (reply_go, reply_go_rx) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        core.serve_hello(ALL_METHODS).await;
        // The anchor attach on channel 5 keeps the connection alive.
        let req = core.read_request().await;
        core.reply(
            &req["id"],
            attach_result(5, minimal_terminal(), false, false),
        )
        .await;
        // The doomed open's request is in flight.
        let doomed = core.read_request().await;
        req_seen.send(()).unwrap();
        reply_go_rx.await.unwrap();
        // The caller is gone; the reply still lands and its channel
        // must get a CLOSE — never a leaked viewer.
        core.reply(
            &doomed["id"],
            attach_result(7, minimal_terminal(), false, false),
        )
        .await;
        let close = core.read_frame().await;
        assert_eq!(
            (close.channel, close.kind),
            (7, Kind::Close),
            "a late attach reply must close its channel"
        );
        core.send_stream(5, 0, b"anchor-out").await;
        core.drain_until_eof().await;
    });
    let anchor = conn.attach(&params()).await.expect("anchor attach");
    let doomed = tokio::spawn({
        let conn = conn.clone();
        let p = params();
        async move {
            let _ = conn.attach(&p).await;
        }
    });
    req_seen_rx.await.expect("doomed request never seen");
    doomed.abort();
    reply_go.send(()).unwrap();
    assert_eq!(
        anchor.recv().await,
        Some(Output::Data(b"anchor-out".to_vec()))
    );
    drop(anchor);
    joined(server).await;
}

#[tokio::test]
async fn a_stalled_consumer_delays_neither_channels_nor_channel0() {
    let (conn, mut core, connects) = shared_once();
    let server = tokio::spawn(async move {
        core.serve_hello(ALL_METHODS).await;
        for channel in [1u32, 2] {
            let req = core.read_request().await;
            core.reply(
                &req["id"],
                attach_result(channel, minimal_terminal(), false, false),
            )
            .await;
        }
        // Channel 1's consumer never drains: a big backlog pile builds.
        core.send_stream(1, 0, &vec![b'x'; 128 * 1024]).await;
        // Channel 2 traffic and a channel-0 attach reply still flow.
        core.send_stream(2, 0, b"unblocked").await;
        let req = core.read_request().await;
        core.reply(
            &req["id"],
            attach_result(3, minimal_terminal(), false, false),
        )
        .await;
        core.drain_until_eof().await;
    });
    let p = params();
    let (a, b) = tokio::join!(conn.attach(&p), conn.attach(&p));
    let (a, b) = (a.unwrap(), b.unwrap());
    // `a` is never recvd while its backlog fills — `b` and a third open
    // proceed regardless.
    assert_eq!(b.recv().await, Some(Output::Data(b"unblocked".to_vec())));
    let c = conn.attach(&p).await.expect("third attach");
    assert_eq!(c.channel(), 3);
    assert_eq!(connects.load(Ordering::SeqCst), 1);
    // And the stalled backlog is intact when the consumer finally drains.
    let mut drained = 0usize;
    while let Some(Output::Data(bytes)) = a.recv().await {
        drained += bytes.len();
        if drained >= 128 * 1024 {
            break;
        }
    }
    assert_eq!(drained, 128 * 1024);
    drop(a);
    drop(b);
    drop(c);
    joined(server).await;
}

#[tokio::test]
async fn backed_up_input_cannot_starve_another_channels_input_or_close() {
    // A small transport buffer stalls the writer: channel 1's queued
    // input backs up while channel 2 still gets its frames through.
    let (core, read, write) = pair(4 * 1024);
    let connects = Arc::new(AtomicU64::new(0));
    let conn = shared(
        Arc::new(Mutex::new(VecDeque::from([ConnectParts {
            read,
            write,
            guard: None,
        }]))),
        Arc::clone(&connects),
    );
    let mut core = core;
    let (gate, gate_rx) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        core.serve_hello(ALL_METHODS).await;
        for channel in [1u32, 2] {
            let req = core.read_request().await;
            core.reply(
                &req["id"],
                attach_result(channel, minimal_terminal(), false, false),
            )
            .await;
        }
        gate_rx.await.unwrap();
        // Drain until channel 2's CLOSE arrives: its input must land
        // even though channel 1's queue had more waiting.
        let mut saw_input1 = false;
        let mut saw_input2 = false;
        let close2 = loop {
            let frame = core.read_frame().await;
            match (frame.channel, frame.kind) {
                (1, Kind::Data) => saw_input1 = true,
                (2, Kind::Data) => {
                    assert_eq!(frame.payload, b"small".to_vec());
                    saw_input2 = true;
                }
                (2, Kind::Close) => break true,
                _ => {}
            }
        };
        assert!(close2);
        assert!(saw_input1, "channel 1 input was written too");
        assert!(saw_input2, "channel 2 input was not starved");
        core.send_stream(2, 2, b"").await;
        core.drain_until_eof().await;
    });
    let p = params();
    let (a, b) = tokio::join!(conn.attach(&p), conn.attach(&p));
    let (a, b) = (a.unwrap(), b.unwrap());
    a.send_input(&vec![b'a'; 128 * 1024]).await.unwrap();
    b.send_input(b"small").await.unwrap();
    gate.send(()).unwrap();
    // Channel 2's frame is written in the same fair round as channel 1's.
    tokio::time::timeout(DEADLINE, b.drain_data())
        .await
        .expect("channel 2's input fence never drained")
        .expect("channel 2's input was undeliverable");
    let ended = b.detach().await.expect("detach");
    assert_eq!(ended.reason, Some(hc_proto::client::CloseReason::Empty));
    drop(a);
    joined(server).await;
}

#[tokio::test]
async fn credit_overflow_closes_only_that_channel() {
    let (conn, mut core, _) = shared_once();
    let server = tokio::spawn(async move {
        core.serve_hello(ALL_METHODS).await;
        for channel in [1u32, 2] {
            let req = core.read_request().await;
            core.reply(
                &req["id"],
                attach_result(channel, minimal_terminal(), false, false),
            )
            .await;
        }
        // Channel 1 outruns the receive credit window the backlog bound
        // models — a protocol violation for that channel alone.
        for _ in 0..3 {
            core.send_stream(1, 0, &vec![b'x'; 128 * 1024]).await;
        }
        let close = core.read_frame().await;
        assert_eq!((close.channel, close.kind), (1, Kind::Close));
        core.send_stream(2, 0, b"untouched").await;
        core.drain_until_eof().await;
    });
    let p = params();
    let (a, b) = tokio::join!(conn.attach(&p), conn.attach(&p));
    let (a, b) = (a.unwrap(), b.unwrap());
    let failed = tokio::time::timeout(DEADLINE, async {
        loop {
            match a.recv().await {
                Some(Output::Data(_)) => {}
                Some(Output::Failed(why)) => break why,
                other => panic!("channel 1 expected a failure, got {other:?}"),
            }
        }
    })
    .await
    .expect("channel 1 never failed");
    assert!(failed.contains("credit"), "{failed}");
    assert!(a.is_closed());
    // The violation was channel-scoped: channel 2 still works.
    assert_eq!(b.recv().await, Some(Output::Data(b"untouched".to_vec())));
    assert!(!b.is_closed());
    drop(a);
    drop(b);
    joined(server).await;
}

#[tokio::test]
async fn transport_death_ends_every_attach_and_the_next_open_rebuilds_once() {
    let (core1, read1, write1) = pair(64 * 1024);
    let (core2, read2, write2) = pair(64 * 1024);
    let connects = Arc::new(AtomicU64::new(0));
    let conn = shared(
        Arc::new(Mutex::new(VecDeque::from([
            ConnectParts {
                read: read1,
                write: write1,
                guard: None,
            },
            ConnectParts {
                read: read2,
                write: write2,
                guard: None,
            },
        ]))),
        Arc::clone(&connects),
    );
    let mut core1 = core1;
    let server1 = tokio::spawn(async move {
        core1.serve_hello(ALL_METHODS).await;
        for channel in [1u32, 2] {
            let req = core1.read_request().await;
            core1
                .reply(
                    &req["id"],
                    attach_result(channel, minimal_terminal(), false, false),
                )
                .await;
        }
        core1 // dropped: the transport dies under both attaches
    });
    let p = params();
    let (a, b) = tokio::join!(conn.attach(&p), conn.attach(&p));
    let (a, b) = (a.unwrap(), b.unwrap());
    joined(server1).await;
    for attach in [&a, &b] {
        let event = tokio::time::timeout(DEADLINE, attach.recv())
            .await
            .expect("attach never reported the dead transport")
            .expect("attach ended without an event");
        assert!(matches!(event, Output::Failed(_)), "{event:?}");
        assert!(attach.is_closed());
    }
    // Every pane recovering at once still produces exactly one reconnect.
    let mut core2 = core2;
    let server2 = tokio::spawn(async move {
        core2.serve_hello(ALL_METHODS).await;
        for channel in [3u32, 4, 5] {
            let req = core2.read_request().await;
            core2
                .reply(
                    &req["id"],
                    attach_result(channel, minimal_terminal(), false, false),
                )
                .await;
        }
        core2.drain_until_eof().await;
    });
    let (c, d, e) = tokio::join!(conn.attach(&p), conn.attach(&p), conn.attach(&p));
    let (c, d, e) = (c.unwrap(), d.unwrap(), e.unwrap());
    assert_eq!(connects.load(Ordering::SeqCst), 2, "one rebuild, not three");
    assert_eq!(
        {
            let mut channels = [c.channel(), d.channel(), e.channel()];
            channels.sort();
            channels
        },
        [3, 4, 5]
    );
    drop(a);
    drop(b);
    drop(c);
    drop(d);
    drop(e);
    joined(server2).await;
}

#[cfg(unix)]
mod ssh {
    use crate::common::attach::{observer, params};
    use crate::common::fakecore;
    use hc_proto::client::AttachConn;
    use hc_proto::endpoint::{Endpoint, Remote};
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::time::Duration;

    const DEADLINE: Duration = Duration::from_secs(20);

    struct Fixtures {
        dir: PathBuf,
        pidfile: PathBuf,
        two_seen: PathBuf,
        spawns: PathBuf,
    }

    fn fixtures() -> &'static Fixtures {
        static FIXTURES: std::sync::OnceLock<Fixtures> = std::sync::OnceLock::new();
        FIXTURES.get_or_init(|| {
            let dir = std::env::temp_dir().join(format!("hc-tui-shared-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let pidfile = dir.join("ssh.pid");
            let two_seen = dir.join("attach2.seen");
            let spawns = dir.join("ssh.spawns");
            // One spawn marker, one `hello` answered, then two bytes of
            // framed attach requests — proving both opens rode this one
            // child — before sleeping like `hc connect` would.
            let reply = serde_json::to_string(&json!({
                "jsonrpc": "2.0", "id": 0,
                "result": fakecore::hello_result(fakecore::ALL_METHODS),
            }))
            .unwrap();
            let path = dir.join("ssh-count");
            std::fs::write(
                &path,
                format!(
                    "#!/bin/sh\nprintf '%s\\n' $$ >> '{spawns}'\nprintf '%s' $$ > '{pid}'\nIFS= read -r line\nprintf '%s\\n' '{reply}'\ndd bs=1 count=1 of=/dev/null 2>/dev/null\ndd bs=1 count=1 of=/dev/null 2>/dev/null\nprintf x > '{seen}'\nexec sleep 60\n",
                    spawns = spawns.display(),
                    pid = pidfile.display(),
                    seen = two_seen.display(),
                ),
            )
            .unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            unsafe {
                libc::atexit(remove_fixtures);
            }
            Fixtures {
                dir,
                pidfile,
                two_seen,
                spawns,
            }
        })
    }

    extern "C" fn remove_fixtures() {
        let dir = std::env::temp_dir().join(format!("hc-tui-shared-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(dir);
    }

    fn reaped(pid: i32) -> bool {
        let err = unsafe { libc::kill(pid, 0) };
        err == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    }

    async fn deadline_wait(done: impl Fn() -> bool) {
        tokio::time::timeout(DEADLINE, async {
            while !done() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("deadline exceeded");
    }

    /// Two opens over `Endpoint::Remote` spawn exactly one ssh child that
    /// answers exactly one `hello` — the framed requests of both opens
    /// ride the same transport.
    #[tokio::test]
    async fn two_opens_spawn_one_ssh_child_and_one_hello() {
        let fx = fixtures();
        let ssh = fx.dir.join("ssh-count");
        let endpoint = Endpoint::Remote(Remote {
            target: "test-host".into(),
            session: "test".into(),
        });
        let conn = AttachConn::new(endpoint, Some(ssh.into_os_string()));
        let p = params();
        let o = observer();
        let mut opens = Vec::new();
        for params in [p, o] {
            let conn = conn.clone();
            opens.push(tokio::spawn(async move {
                let _ = conn.attach(&params).await;
            }));
        }
        // Both attach requests reached the one child.
        deadline_wait(|| fx.two_seen.exists()).await;
        let spawns = std::fs::read_to_string(&fx.spawns).unwrap();
        assert_eq!(
            spawns.lines().count(),
            1,
            "two opens spawned more than one ssh child"
        );
        for open in &opens {
            open.abort();
        }
        drop(conn);
        drop(opens);
        let pid: i32 = std::fs::read_to_string(&fx.pidfile)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        deadline_wait(|| reaped(pid)).await;
    }
}
