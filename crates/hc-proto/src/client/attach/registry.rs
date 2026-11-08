// Derived from herdr-core crates/herdr-core/src/server.rs and viewers.rs (Apache-2.0).

//! [`AttachConn`]: the lazy, single-flight registry a client holds per
//! endpoint — the first open connects, concurrent opens share that one
//! connect (and its one `hello`), a dead connection is rebuilt once by
//! the next open, and the last attach's end closes it all.

use super::conn::Conn;
use super::error::AttachError;
use crate::codec::{Reader, Writer};
use crate::endpoint::Endpoint;
use crate::terminal::AttachParams;
use crate::transport::{Guard, ReadHalf, WriteHalf};
use std::ffi::OsString;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio::sync::OnceCell;

use super::Attach;
use crate::client::lock;

/// A lazily connected shared attach connection for one endpoint: clone it
/// freely, every `attach` rides the same framed connection — one
/// transport, one `hello`, many channels. It connects on the first open
/// (concurrent opens share that one connect), rebuilds once on the first
/// open after the connection dies, and closes when its last attach ends.
/// The local Unix-socket path keeps [`Attach::connect`]; this is for
/// endpoints where the connect itself is expensive, like ssh.
#[derive(Clone)]
pub struct AttachConn {
    inner: Arc<Inner>,
}

struct Inner {
    connect: Connect,
    slot: Mutex<Stage>,
}

/// Where the shared connection's transport comes from.
enum Connect {
    /// A resolved endpoint — ssh for `Remote`, a socket for `Local`.
    Endpoint {
        endpoint: Endpoint,
        ssh_program: Option<OsString>,
    },
    /// Scripted transports for tests.
    Injected(Connector),
}

/// Single-flight connect state: `Connecting` keeps one `OnceCell` so every
/// concurrent open waits on the same in-flight connect.
enum Stage {
    Empty,
    Connecting(Arc<OnceCell<Arc<Conn>>>),
    Live(Arc<Conn>),
}

/// The halves an injected connector hands the connection.
#[doc(hidden)]
pub struct ConnectParts {
    /// Protocol bytes from the core.
    pub read: ReadHalf,
    /// Protocol bytes to the core.
    pub write: WriteHalf,
    /// The backend's lifetime, when there is one.
    pub guard: Option<Guard>,
}

/// Produces one transport — called again only after a death.
#[doc(hidden)]
pub type Connector = Arc<
    dyn Fn() -> Pin<Box<dyn Future<Output = Result<ConnectParts, AttachError>> + Send>>
        + Send
        + Sync,
>;

impl AttachConn {
    /// A shared attach connection lazily opened on `endpoint`.
    /// `ssh_program` overrides the `ssh` name for tests; production
    /// passes `None`.
    pub fn new(endpoint: Endpoint, ssh_program: Option<OsString>) -> AttachConn {
        AttachConn {
            inner: Arc::new(Inner {
                connect: Connect::Endpoint {
                    endpoint,
                    ssh_program,
                },
                slot: Mutex::new(Stage::Empty),
            }),
        }
    }

    /// A shared attach connection whose transports come from `connect`
    /// instead of an endpoint — tests inject scripted wires this way.
    #[doc(hidden)]
    pub fn with_connector<F, Fut>(connect: F) -> AttachConn
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<ConnectParts, AttachError>> + Send + 'static,
    {
        AttachConn {
            inner: Arc::new(Inner {
                connect: Connect::Injected(Arc::new(move || Box::pin(connect()))),
                slot: Mutex::new(Stage::Empty),
            }),
        }
    }

    /// One attach on the shared connection. The returned `Attach` is the
    /// same handle a dedicated [`Attach::connect`] returns: same API,
    /// same guarantees, only its channel is its own.
    pub async fn attach(&self, params: &AttachParams) -> Result<Attach, AttachError> {
        let conn = self.conn().await?;
        match conn.attach(params).await {
            Err(err) => Err(conn.diagnose(err).await),
            ok => ok,
        }
    }

    /// The live shared connection, or one freshly established. The
    /// `OnceCell` makes a burst of opens share a single connect —
    /// including the reconnect many panes trigger after a death.
    async fn conn(&self) -> Result<Arc<Conn>, AttachError> {
        let cell = {
            let mut slot = lock(&self.inner.slot);
            match &*slot {
                Stage::Live(conn) if !conn.dead() => return Ok(Arc::clone(conn)),
                Stage::Live(_) | Stage::Empty => {
                    let cell = Arc::new(OnceCell::new());
                    *slot = Stage::Connecting(Arc::clone(&cell));
                    cell
                }
                Stage::Connecting(cell) => Arc::clone(cell),
            }
        };
        match cell.get_or_try_init(|| self.open()).await {
            Ok(conn) => {
                let mut slot = lock(&self.inner.slot);
                if matches!(&*slot, Stage::Connecting(c) if Arc::ptr_eq(c, &cell)) {
                    *slot = Stage::Live(Arc::clone(conn));
                }
                Ok(Arc::clone(conn))
            }
            Err(err) => {
                let mut slot = lock(&self.inner.slot);
                if matches!(&*slot, Stage::Connecting(c) if Arc::ptr_eq(c, &cell)) {
                    *slot = Stage::Empty;
                }
                Err(err)
            }
        }
    }

    /// One fresh shared connection.
    async fn open(&self) -> Result<Arc<Conn>, AttachError> {
        match &self.inner.connect {
            Connect::Endpoint {
                endpoint,
                ssh_program,
            } => Conn::connect(endpoint, ssh_program.as_deref()).await,
            Connect::Injected(connector) => {
                let parts = connector().await?;
                Conn::negotiate(
                    Reader::new(parts.read),
                    Writer::new(parts.write),
                    parts.guard,
                )
                .await
            }
        }
    }
}
