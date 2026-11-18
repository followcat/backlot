//! The dedicated subscription opener: each attempt opens a fresh
//! connection, counted into the caller's stats, and reports a live
//! subscription, an unsupported core or a failure.

use super::{OpenSub, SubAttempt};
use crate::client::CallError;
use crate::endpoint::Endpoint;
use crate::events::SubscribeParams;
use crate::subscribe::{SubscribeError, Subscription};
use std::ffi::OsStr;
use std::sync::Arc;

/// Opens the dedicated subscription on `endpoint` for each attempt, with
/// every attempt's transport counted into `stats`.
pub fn subscription_opener(
    endpoint: &Endpoint,
    ssh_program: Option<&OsStr>,
    stats: Arc<crate::stats::ConnStats>,
) -> OpenSub {
    let ssh = ssh_program.map(|s| s.to_os_string());
    let sub_endpoint = endpoint.clone();
    let sub: OpenSub = Arc::new(move |subscribe: SubscribeParams| {
        let endpoint = sub_endpoint.clone();
        let ssh = ssh.clone();
        let stats = Arc::clone(&stats);
        Box::pin(async move {
            match Subscription::connect_counted(&endpoint, ssh.as_deref(), &subscribe, &stats).await
            {
                Ok(subscription) => SubAttempt::Subscribed(Box::new(subscription)),
                Err(SubscribeError::Call(CallError::Unsupported { reason, .. })) => {
                    SubAttempt::Unsupported(reason)
                }
                Err(e) => SubAttempt::Failed(e),
            }
        })
    });
    sub
}
