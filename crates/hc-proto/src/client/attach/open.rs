// Derived from herdr-core crates/herdr-core/src/server.rs and viewers.rs (Apache-2.0).

//! Opening a dedicated attach connection: transport connect, the
//! negotiated `hello`, then `terminal.attach` on its channel-0. The
//! connection machinery — shared by [`AttachConn`]'s multiplexed wires —
//! lives in [`super::conn`]: its cleanup on any failure aborts the tasks
//! and closes the backend so a cancelled open still kills and reaps an
//! ssh child. An attach error on the fresh wire gets its exit status and
//! stderr tail when the peer plainly died.

use super::Attach;
use super::conn::Conn;
use super::error::AttachError;
use crate::endpoint::Endpoint;
use crate::terminal::AttachParams;
use crate::transport::{Guard, ReadHalf, WriteHalf};
use std::ffi::OsStr;

impl Attach {
    /// Opens `endpoint`, negotiates the framed wire and issues
    /// `terminal.attach`. `ssh_program` overrides the `ssh` name for
    /// tests; production passes `None`.
    pub async fn connect(
        endpoint: &Endpoint,
        ssh_program: Option<&OsStr>,
        params: &AttachParams,
    ) -> Result<Attach, AttachError> {
        let conn = Conn::connect(endpoint, ssh_program).await?;
        match conn.attach(params).await {
            Err(err) => Err(conn.diagnose(err).await),
            ok => ok,
        }
    }

    /// Attaches over an injected byte stream — tests use in-memory wires.
    /// `guard` owns the backend's lifetime when there is one.
    pub async fn open(
        read: ReadHalf,
        write: WriteHalf,
        guard: Option<Guard>,
        params: &AttachParams,
    ) -> Result<Attach, AttachError> {
        let conn = Conn::negotiate(
            crate::codec::Reader::new(read),
            crate::codec::Writer::new(write),
            guard,
        )
        .await?;
        match conn.attach(params).await {
            Err(err) => Err(conn.diagnose(err).await),
            ok => ok,
        }
    }
}
