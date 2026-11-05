// Derived from herdr-core schema/README.md and crates/herdr-core/src/api.rs (Apache-2.0).

//! The client's call surface: a raw `call`/`call_timeout` plus typed
//! wrappers for every method the API declares. Capability gating is
//! `hello`'s method list first, then — once `rpc.discover` lands — the
//! core's own declaration; a missing method is `Unsupported`, never a
//! wire call.

use super::call::{self, DiscoveryCells};
use super::error::CallError;
use super::{Client, lock};
use crate::OpenRpcDocument;
use crate::doc::{DocGetParams, DocGetResult, DocSetParams, DocSetResult};
use crate::machine::MachineListResult;
use crate::method;
use crate::terminal::{
    ListParams, ListResult, ReadParams, ReadResult, RunParams, RunResult, Terminal, TerminalParams,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

/// Default per-call deadline; remote calls (a redialing `doc.get`) can be
/// slow, so this is generous. `call_timeout` overrides it per call.
const CALL_TIMEOUT: Duration = Duration::from_secs(120);

impl Client {
    /// Re-runs `rpc.discover` when the core offers it and records the
    /// outcome — bounded, and a noted limitation rather than a failure.
    pub async fn refresh_discovery(&self) {
        let cells: DiscoveryCells = (
            Arc::clone(&self.shared.discovery),
            Arc::clone(&self.shared.discovery_methods),
            Arc::clone(&self.shared.discovery_note),
        );
        call::discover(&self.shared.hello, &self.shared.wire, &cells).await;
    }

    /// One raw call by method name — the generic entry typed calls use, and
    /// a test hook for methods without typed wrappers. Params go by-name;
    /// `None` omits the `params` member.
    pub async fn call(&self, method: &str, params: Option<Value>) -> Result<Value, CallError> {
        self.call_timeout(method, params, CALL_TIMEOUT).await
    }

    /// [`call`](Self::call) with a per-call deadline covering both the
    /// enqueue (a full queue behind a stalled peer) and the reply wait.
    /// Dropping this future — whatever phase it is in — removes the pending
    /// entry, so a cancelled caller frees its slot immediately.
    pub async fn call_timeout(
        &self,
        method: &str,
        params: Option<Value>,
        deadline: Duration,
    ) -> Result<Value, CallError> {
        if let Some(reason) = self.explain_unavailable(method) {
            return Err(CallError::Unsupported {
                method: method.into(),
                reason,
            });
        }
        call::wire_call(&self.shared.wire, method, params, deadline).await
    }

    /// Why `method` cannot run, or `None` when it can: `hello`'s list first,
    /// then — once discovery has landed — the core's own declaration.
    pub fn explain_unavailable(&self, method: &str) -> Option<String> {
        if let Some(reason) = self.capabilities().explain_unavailable(method) {
            return Some(reason);
        }
        if let Some(names) = lock(&self.shared.discovery_methods).as_ref()
            && !names.contains(method)
        {
            return Some(format!(
                "core {}'s rpc.discover does not declare `{method}`",
                self.core().name
            ));
        }
        None
    }

    /// A typed call: serialize params, deserialize the result shape.
    async fn call_typed<P, R>(&self, method: &str, params: &P) -> Result<R, CallError>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        let params = serde_json::to_value(params).map_err(CallError::Shape)?;
        let result = self.call(method, Some(params)).await?;
        serde_json::from_value(result).map_err(CallError::Shape)
    }

    /// `terminal.list`: live terminals, optionally all and label-filtered.
    pub async fn terminal_list(&self, params: ListParams) -> Result<ListResult, CallError> {
        self.call_typed(method::TERMINAL_LIST, &params).await
    }

    /// `terminal.get`: one terminal's facts.
    pub async fn terminal_get(&self, params: TerminalParams) -> Result<Terminal, CallError> {
        self.call_typed(method::TERMINAL_GET, &params).await
    }

    /// `terminal.run`: start a program in a new terminal. State-changing —
    /// never replayed automatically.
    pub async fn terminal_run(&self, params: RunParams) -> Result<RunResult, CallError> {
        self.call_typed(method::TERMINAL_RUN, &params).await
    }

    /// `terminal.read`: the screen plus history as text.
    pub async fn terminal_read(&self, params: ReadParams) -> Result<ReadResult, CallError> {
        self.call_typed(method::TERMINAL_READ, &params).await
    }

    /// `terminal.kill`: end a terminal's program; answers with its record.
    /// State-changing — never replayed automatically.
    pub async fn terminal_kill(&self, params: TerminalParams) -> Result<Terminal, CallError> {
        self.call_typed(method::TERMINAL_KILL, &params).await
    }

    /// `machine.list`: machines this core has added.
    pub async fn machine_list(&self) -> Result<MachineListResult, CallError> {
        let result = self.call(method::MACHINE_LIST, None).await?;
        serde_json::from_value(result).map_err(CallError::Shape)
    }

    /// `doc.get`: an opaque JSON document and its version.
    pub async fn doc_get(&self, params: DocGetParams) -> Result<DocGetResult, CallError> {
        self.call_typed(method::DOC_GET, &params).await
    }

    /// `doc.set`: opaque, versioned compare-and-set; no automatic replay.
    pub async fn doc_set(&self, params: DocSetParams) -> Result<DocSetResult, CallError> {
        self.call_typed(method::DOC_SET, &params).await
    }

    /// `rpc.discover`: the whole API as an OpenRPC document, kept opaque.
    pub async fn rpc_discover(&self) -> Result<OpenRpcDocument, CallError> {
        self.call(method::RPC_DISCOVER, None).await
    }
}
