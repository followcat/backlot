//! Shared attach fixtures: params builders plus deadline-bounded
//! open/join helpers.

use hc_proto::client::Attach;
use hc_proto::terminal::AttachParams;
use hc_proto::transport::{ReadHalf, WriteHalf};
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
