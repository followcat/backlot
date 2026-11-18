//! Named document subscriptions (#273): what the actor asks the core for.
//!
//! `events.subscribe` gains `docs`. A core that names documents narrows
//! to those alone when no terminal selection is present, so hc-tui keeps
//! the list empty — and a fork with a terminal filter extends it through
//! `WatchParams::doc_patterns`.

use super::*;
use crate::events::SubscribeParams;
use crate::watch::{OpenRpc, OpenSub, SubAttempt, WatchParams, Watcher};
use std::time::Duration;

/// Starts a watcher whose subscription opener records the params it
/// receives and then parks (an attempt in flight, no connection).
fn recording(params: WatchParams) -> (Watcher, Arc<Mutex<Vec<SubscribeParams>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = Arc::clone(&seen);
    let rpc: OpenRpc = Arc::new(|| Box::pin(std::future::pending()));
    let sub: OpenSub = Arc::new(move |params| {
        record
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(params);
        Box::pin(std::future::pending::<SubAttempt>())
    });
    (Watcher::with_openers(rpc, sub, params), seen)
}

/// The first recorded attempt, waiting for the actor to make it.
async fn first(seen: &Arc<Mutex<Vec<SubscribeParams>>>) -> SubscribeParams {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(params) = seen
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .first()
            .cloned()
        {
            return params;
        }
        assert!(tokio::time::Instant::now() < deadline, "no attempt");
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

#[tokio::test]
async fn hc_tui_names_no_document_so_terminal_events_keep_flowing() {
    let (watcher, seen) = recording(WatchParams::default());
    let params = first(&seen).await;
    assert!(
        params.docs.is_empty(),
        "naming documents alone would silence the inventory: {:?}",
        params.docs
    );
    assert_eq!(
        serde_json::to_value(&params).unwrap(),
        serde_json::json!({})
    );
    watcher.stop();
}

#[tokio::test]
async fn a_fork_extends_the_subscription_in_one_place() {
    let mut params = WatchParams::default();
    params.subscribe.docs = vec!["herdr-app".into()];
    params.doc_patterns = vec![
        "backlot".into(),
        "studio/*".into(),
        "backlot".into(),
        "herdr-app".into(),
    ];
    let (watcher, seen) = recording(params);
    let request = first(&seen).await;
    assert_eq!(
        request.docs,
        ["herdr-app", "backlot", "studio/*"],
        "patterns are appended once, in order, behind the configured names"
    );
    watcher.stop();
}
