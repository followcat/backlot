// Derived from herdr-core crates/herdr-core/src/server.rs and viewers.rs (Apache-2.0).

//! Backend diagnostics on a dead or unopened shared connection: the
//! child's own exit status and stderr tail beat a bare `Closed` when the
//! peer plainly died — on a failed open and on an attach that landed
//! after the connection died. A real protocol answer is never replaced.

use super::conn::Conn;
use super::error::{AttachError, with_transport_diagnostics};
use crate::client::lock;
use crate::codec::{Reader, Writer};
use crate::transport::{self, Guard, ReadHalf, WriteHalf};

impl Conn {
    /// The spawned backend close's report — `Some` once `died` started
    /// it; an abort/panic reports as a clean default rather than failing.
    pub(super) async fn close_report(&self) -> Option<Result<transport::Closed, transport::Error>> {
        let task = lock(&self.close_task).take()?;
        Some(
            task.await
                .unwrap_or_else(|_| Ok(transport::Closed::default())),
        )
    }

    /// A dead connection's attach error gets the backend's real status
    /// and stderr tail when the peer plainly died — a real protocol
    /// answer is never replaced.
    pub(super) async fn diagnose(&self, err: AttachError) -> AttachError {
        if !self.dead() {
            return err;
        }
        match self.close_report().await {
            Some(report) => with_transport_diagnostics(err, report),
            None => err,
        }
    }
}

/// A negotiation failure with a backend behind it: the codec halves drop
/// first — otherwise the child never sees stdin EOF and `guard.close_eof()`
/// can only end it at its kill budget — then the child's own exit
/// status and stderr tail are kept when the peer actually died.
pub(super) async fn open_failed(
    err: AttachError,
    reader: Reader<ReadHalf>,
    writer: Writer<WriteHalf>,
    guard: Option<Guard>,
) -> AttachError {
    drop(reader);
    drop(writer);
    let Some(guard) = guard else { return err };
    with_transport_diagnostics(err, guard.close_eof().await)
}
