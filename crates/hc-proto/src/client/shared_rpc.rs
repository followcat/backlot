//! The shared framed RPC opener: one lazily connected [`Client`] every
//! `OpenRpc` consumer on an endpoint rides — the watcher's refreshes,
//! the layout store and the seen writer — modeled on [`AttachConn`]'s
//! single-flight registry. Concurrent callers share one in-flight
//! connect; a live client is handed out until the wire itself dies
//! (`Client::is_closed`), then the next call rebuilds once.
//!
//! [`AttachConn`]: crate::client::AttachConn

use super::{Client, Error, lock};
use crate::endpoint::Endpoint;
use crate::watch::{BoxFuture, OpenRpc};
use std::ffi::OsString;
use std::sync::{Arc, Mutex};
use tokio::sync::OnceCell;

/// Where the shared client's transport comes from.
enum Connect {
    /// A resolved endpoint — ssh for `Remote`, a socket for `Local`.
    Endpoint {
        endpoint: Endpoint,
        ssh_program: Option<OsString>,
        stats: Arc<crate::stats::ConnStats>,
    },
    /// Scripted openers for tests.
    Injected(Opener),
}

/// Produces one fully negotiated client — called again only after the
/// shared wire died.
type Opener = Arc<dyn Fn() -> BoxFuture<Result<Client, Error>> + Send + Sync>;

/// Single-flight connect state: `Connecting` keeps one `OnceCell` so
/// every concurrent call waits on the same in-flight connect.
enum Stage {
    Empty,
    Connecting(Arc<OnceCell<Client>>),
    Live(Client),
}

struct Inner {
    connect: Connect,
    slot: Mutex<Stage>,
}

impl Inner {
    /// The live shared client, or one freshly connected. A cached
    /// client is reused while its wire lives — a per-call failure on
    /// one consumer never spoils it for the others — and the `OnceCell`
    /// makes a burst of concurrent calls share a single reconnect.
    async fn client(&self) -> Result<Client, Error> {
        let cell = {
            let mut slot = lock(&self.slot);
            match &*slot {
                Stage::Live(client) if !client.is_closed() => return Ok(client.clone()),
                Stage::Live(_) | Stage::Empty => {
                    let cell = Arc::new(OnceCell::new());
                    *slot = Stage::Connecting(Arc::clone(&cell));
                    cell
                }
                Stage::Connecting(cell) => Arc::clone(cell),
            }
        };
        match cell.get_or_try_init(|| self.open()).await {
            Ok(client) => {
                let mut slot = lock(&self.slot);
                if matches!(&*slot, Stage::Connecting(c) if Arc::ptr_eq(c, &cell)) {
                    *slot = Stage::Live(client.clone());
                }
                Ok(client.clone())
            }
            Err(err) => {
                let mut slot = lock(&self.slot);
                if matches!(&*slot, Stage::Connecting(c) if Arc::ptr_eq(c, &cell)) {
                    *slot = Stage::Empty;
                }
                Err(err)
            }
        }
    }

    /// One fresh connect.
    async fn open(&self) -> Result<Client, Error> {
        match &self.connect {
            Connect::Endpoint {
                endpoint,
                ssh_program,
                stats,
            } => Client::connect_counted(endpoint, ssh_program.as_deref(), stats).await,
            Connect::Injected(opener) => opener().await,
        }
    }
}

/// The `OpenRpc` closure the registry hands out.
fn opener(connect: Connect) -> OpenRpc {
    let inner = Arc::new(Inner {
        connect,
        slot: Mutex::new(Stage::Empty),
    });
    Arc::new(move || {
        let inner = Arc::clone(&inner);
        Box::pin(async move { inner.client().await })
    })
}

/// The shared framed RPC connection for `endpoint`: every call returns
/// a clone of the one negotiated [`Client`] while its wire lives, so
/// the watcher's refreshes, layout actions and background seen writes
/// all multiplex onto a single transport (one ssh child, one `hello`).
/// `ssh_program` overrides the `ssh` name for tests; production passes
/// `None`.
pub fn shared_rpc(endpoint: Endpoint, ssh_program: Option<OsString>) -> OpenRpc {
    shared_rpc_counted(endpoint, ssh_program, Arc::default())
}

/// [`shared_rpc`] with every connection it opens counted into `stats`.
pub fn shared_rpc_counted(
    endpoint: Endpoint,
    ssh_program: Option<OsString>,
    stats: Arc<crate::stats::ConnStats>,
) -> OpenRpc {
    opener(Connect::Endpoint {
        endpoint,
        ssh_program,
        stats,
    })
}

/// A shared RPC opener whose clients come from `connect` instead of an
/// endpoint — tests inject scripted or counted wires this way.
#[doc(hidden)]
pub fn shared_rpc_with<F, Fut>(connect: F) -> OpenRpc
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<Client, Error>> + Send + 'static,
{
    opener(Connect::Injected(Arc::new(move || Box::pin(connect()))))
}
