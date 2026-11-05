//! Pane state types: role/spec identity, attach generations, and the
//! Idle→Opening→Live→Down→Gone machine the runtime drives.
use super::*;

/// What the pane is allowed to do — fixed for the attach's life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The focused terminal: types, resizes, reports focus.
    Active,
    /// A visible pane size holder: never sends human input or focus.
    SizeLease,
    /// A preview while browsing the selector: never types or resizes.
    Observe,
}

/// Which terminal, in which role. Size/resume are per-attach details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    /// The localized terminal address (`t7` or `host-b/t7`).
    pub terminal: String,
    /// Active viewer or observer.
    pub role: Role,
}

/// An attach-actor or writer report, tagged with its attach generation.
pub enum PaneEv {
    /// The attach negotiated; the `Attach` rides back for the owner.
    Opened {
        generation: u64,
        attach: Arc<Attach>,
    },
    /// A stream event, in wire order.
    Out { generation: u64, out: Output },
    /// The open itself failed (connect, hello, or a refused attach).
    /// `unsupported` marks a `CallError::Unsupported`: the core has no
    /// `terminal.attach` — the feature is disabled visibly, never
    /// retried into a hot loop.
    Failed {
        generation: u64,
        why: String,
        unsupported: bool,
    },
    /// The stream drained after Close/Failed — the wire is over.
    Ended { generation: u64 },
    /// The writer completed a send — pending work may now fit.
    WriterProgress { generation: u64 },
    /// A writer send failed; the stream is dead or refusing.
    WriterFailed { generation: u64, why: String },
    /// A parked route's ordered close finished — its `Closing` entry
    /// retires now, while idle, not on the next `unpark`.
    RouteClosed { route: u64, error: Option<String> },
}

impl PaneEv {
    pub(crate) fn route(&self) -> u64 {
        match self {
            Self::Opened { generation, .. }
            | Self::Out { generation, .. }
            | Self::Failed { generation, .. }
            | Self::Ended { generation }
            | Self::WriterProgress { generation }
            | Self::WriterFailed { generation, .. } => *generation,
            Self::RouteClosed { route, .. } => *route,
        }
    }
}

impl Pane {
    pub(crate) fn owns_route(&self, route: u64) -> bool {
        route == self.generation
            || self.parked_has(route)
            || self.closing.iter().any(|c| c.route == route)
    }
}

/// How an attach connection is opened — `Attach::connect` in production,
/// `Attach::open` over injected wires in tests.
pub type OpenAttach =
    Arc<dyn Fn(AttachParams) -> crate::app::BoxFut<Result<Attach, AttachError>> + Send + Sync>;
