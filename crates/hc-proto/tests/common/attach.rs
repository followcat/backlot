//! Shared attach fixtures: params builders, injected shared connections
//! plus deadline-bounded open/join helpers.

use super::fakecore::{DuplexCore, pair};
use hc_proto::client::{Attach, AttachConn, ConnectParts};
use hc_proto::terminal::AttachParams;
use hc_proto::transport::{ReadHalf, WriteHalf};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinHandle;

pub const DEADLINE: Duration = Duration::from_secs(15);

/// A plain active-viewer attach request.
pub fn params() -> AttachParams {
    AttachParams {
        terminal: "t7".into(),
        observe: false,
        resize_notifications: false,
        quiet_human_release: false,
        history: 0,
        cols: 80,
        rows: 24,
        display: None,
        resumed: false,
        caller: None,
    }
}

/// An observer asking for RESIZE-before-paint notifications.
pub fn observer() -> AttachParams {
    AttachParams {
        observe: true,
        resize_notifications: true,
        quiet_human_release: false,
        ..params()
    }
}

/// An `AttachConn` over injected halves: each `connect` pops the next
/// prepared transport; `connects` counts how many were taken.
pub fn shared(pending: Arc<Mutex<VecDeque<ConnectParts>>>, connects: Arc<AtomicU64>) -> AttachConn {
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
pub fn shared_once() -> (AttachConn, DuplexCore, Arc<AtomicU64>) {
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

/// Joins the client side and returns it; the server half finishes inside.
pub async fn opening(read: ReadHalf, write: WriteHalf, p: &AttachParams) -> Attach {
    tokio::time::timeout(DEADLINE, Attach::open(read, write, None, p))
        .await
        .expect("attach open timed out")
        .expect("attach failed")
}

/// Runs the server task to completion (bounded by the test's own deadline).
pub async fn joined<T>(server: JoinHandle<T>) -> T {
    tokio::time::timeout(DEADLINE, server)
        .await
        .expect("fake core task timed out")
        .expect("fake core task panicked")
}
