//! Quiet human release negotiation and byte-preserving transport.

mod common;

use common::attach::{joined, observer, opening, params};
use common::fakecore::{ALL_METHODS, attach_result, pair};
use common::minimal_terminal;
use hc_proto::client::AttachError;
use hc_proto::frame::{Frame, Kind};
use serde_json::json;

#[tokio::test]
async fn quiet_release_requires_request_hello_and_affirmative_reply() {
    for feature in [false, true] {
        for confirmation in [
            None,
            Some(json!(null)),
            Some(json!(false)),
            Some(json!(true)),
        ] {
            for request in [false, true] {
                let (mut core, read, write) = pair(64 * 1024);
                let (seen, received) = tokio::sync::oneshot::channel();
                let expected = request && feature && confirmation == Some(json!(true));
                let confirmation = confirmation.clone();
                let server = tokio::spawn(async move {
                    let mut result = attach_result(1, minimal_terminal(), false, true);
                    if let Some(flag) = confirmation {
                        result["quiet_human_release"] = flag;
                    }
                    let features: &[&str] = if feature {
                        &["quiet_human_release", "automatic_input"]
                    } else {
                        &["automatic_input"]
                    };
                    let attach = core.serve_attach(ALL_METHODS, features, result).await;
                    assert_eq!(
                        attach["params"]["quiet_human_release"] == true,
                        request && feature
                    );
                    let ordinary = core.read_frame().await;
                    assert_eq!(ordinary.kind, Kind::Data);
                    assert_eq!(ordinary.payload, b"\x1b[97;1:3u");
                    let release = core.read_frame().await;
                    assert_eq!(
                        release.kind,
                        if expected {
                            Kind::QuietHumanRelease
                        } else {
                            Kind::Data
                        }
                    );
                    // The tag comes from semantics, not a byte-pattern detector.
                    assert_eq!(release.payload, b"opaque human release");
                    let _ = seen.send(());
                    core.expect_eof().await;
                });
                let mut p = params();
                p.quiet_human_release = request;
                let attach = opening(read, write, &p).await;
                assert_eq!(attach.quiet_human_release(), expected);
                assert!(attach.automatic_input());
                attach.send_input(b"\x1b[97;1:3u").await.unwrap();
                attach.send_release(b"opaque human release").await.unwrap();
                received.await.unwrap();
                drop(attach);
                joined(server).await;
            }
        }
    }
}

#[tokio::test]
async fn quiet_release_chunks_and_drains_before_close() {
    let (mut core, read, write) = pair(1024);
    let payload = vec![b'x'; 40_001];
    let expected = payload.clone();
    let server = tokio::spawn(async move {
        let mut result = attach_result(1, minimal_terminal(), false, false);
        result["quiet_human_release"] = json!(true);
        core.serve_attach(ALL_METHODS, &["quiet_human_release"], result)
            .await;
        let mut collected = Vec::new();
        while collected.len() < expected.len() {
            let frame = core.read_frame().await;
            assert_eq!((frame.channel, frame.kind), (1, Kind::QuietHumanRelease));
            assert!(frame.payload.len() <= 16 * 1024);
            collected.extend(frame.payload);
        }
        assert_eq!(collected, expected);
        assert_eq!(core.read_frame().await.kind, Kind::Close);
        core.send_stream(1, 2, b"").await;
        core.expect_eof().await;
    });
    let mut p = params();
    p.quiet_human_release = true;
    let attach = opening(read, write, &p).await;
    attach.send_release(b"").await.unwrap();
    attach.send_release(&payload).await.unwrap();
    attach.drain_data().await.unwrap();
    attach.detach().await.unwrap();
    joined(server).await;
}

#[tokio::test]
async fn observe_cannot_negotiate_or_send_human_release() {
    let (mut core, read, write) = pair(64 * 1024);
    let server = tokio::spawn(async move {
        let mut result = attach_result(1, minimal_terminal(), true, true);
        result["quiet_human_release"] = json!(true);
        let request = core
            .serve_attach(ALL_METHODS, &["quiet_human_release"], result)
            .await;
        assert_ne!(request["params"]["quiet_human_release"], true);
        core.expect_eof().await;
    });
    let mut p = observer();
    p.quiet_human_release = true;
    let attach = opening(read, write, &p).await;
    assert!(!attach.quiet_human_release());
    assert!(matches!(
        attach.send_release(b"x").await,
        Err(AttachError::Observe)
    ));
    drop(attach);
    joined(server).await;
}

#[test]
fn kind_seven_accepts_arbitrary_chunk_boundaries() {
    assert_eq!(Kind::from_u8(7), Kind::QuietHumanRelease);
    for bytes in [b"".as_slice(), b"\x1b[", b"97;1:3", b"u"] {
        let frame = Frame::new(1, Kind::QuietHumanRelease, bytes.to_vec());
        assert_eq!(frame.encode()[4], 7);
    }
}
