//! The `backlot` summary document's write policy. The stage is the
//! only writer; it writes with the CAS version, only when the view
//! changed, at most once every [`MIN_GAP_MS`]. [`Throttle`] is pure —
//! callers inject `now_ms` — so the burst behaviour is unit-tested
//! without a core.

use backlot_model::summary::Summary;
use hc_proto::client::{CallError, Client};
use hc_proto::doc::{DocGetParams, DocSetParams};

use crate::cli::Fail;

/// The core document name; fixed because watchers subscribe once.
pub const DOC_NAME: &str = "backlot";

/// At most one write per two seconds; core rewrites all documents in
/// one file, so writes stay rare.
pub const MIN_GAP_MS: u64 = 2_000;

/// The newest desired summary plus what the core last confirmed.
#[derive(Debug, Default)]
pub struct Throttle {
    /// The latest view a tick produced, stamp ignored.
    latest: Option<Summary>,
    /// The last view actually written.
    written: Option<Summary>,
    /// When the last write landed.
    last_ms: u64,
}

impl Throttle {
    /// A tick's rebuilt view.
    pub fn note(&mut self, summary: Summary) {
        self.latest = Some(summary);
    }

    /// The summary to write, when the view changed and the gap allows.
    pub fn due(&self, now_ms: u64) -> Option<Summary> {
        let latest = self.latest.as_ref()?;
        if self.written.as_ref().is_some_and(|w| latest.same_as(w)) {
            return None;
        }
        match self.written {
            None => Some(latest.clone()),
            Some(_) if now_ms >= self.last_ms + MIN_GAP_MS => Some(latest.clone()),
            _ => None,
        }
    }

    /// The write landed: the latest view is now the written one.
    pub fn wrote(&mut self, now_ms: u64) {
        self.written = self.latest.clone();
        self.last_ms = now_ms;
    }
}

/// Write the document if the throttle allows. A CAS conflict just
/// defers to the next tick (the stage is the only writer, so one is
/// a race with a restarting stage's first write — a fresh read each
/// attempt keeps it correct). Returns whether a write landed.
pub async fn publish(client: &Client, throttle: &mut Throttle, now_ms: u64) -> Result<bool, Fail> {
    let Some(mut summary) = throttle.due(now_ms) else {
        return Ok(false);
    };
    summary.updated_ms = now_ms;
    let got = client
        .doc_get(DocGetParams {
            name: DOC_NAME.into(),
            on: None,
        })
        .await
        .map_err(|e| Fail::refused(format!("doc.get failed: {e}")))?;
    let value = serde_json::to_value(&summary)
        .map_err(|e| Fail::refused(format!("summary does not serialize: {e}")))?;
    match client
        .doc_set(DocSetParams {
            name: DOC_NAME.into(),
            on: None,
            value,
            version: got.version,
        })
        .await
    {
        Ok(_) => {
            throttle.wrote(now_ms);
            Ok(true)
        }
        // A conflict defers; a dead wire ends the stage.
        Err(e @ CallError::Rpc(_)) => {
            eprintln!("stage: doc.set conflict: {e}");
            Ok(false)
        }
        Err(e) => Err(Fail::refused(format!("doc.set failed: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use backlot_model::summary::SUMMARY_SCHEMA;

    fn view(n: usize) -> Summary {
        Summary {
            schema: SUMMARY_SCHEMA.into(),
            updated_ms: 0,
            studios: (0..n)
                .map(|i| backlot_model::summary::StudioSummary {
                    id: format!("s{i}"),
                    title: format!("studio {i}"),
                    state: "live".into(),
                    requirement_version: 1,
                    mode: backlot_model::event::StudioMode::Standard,
                    open_reviews: 0,
                    waiting: 0,
                    roles: Vec::new(),
                })
                .collect(),
        }
    }

    /// A burst of changes yields at most one write per two seconds.
    #[test]
    fn a_burst_of_changes_writes_at_most_once_per_two_seconds() {
        let mut t = Throttle::default();
        // First write is immediate.
        t.note(view(1));
        assert!(t.due(1_000).is_some());
        t.wrote(1_000);
        // Changes inside the gap stay pending.
        t.note(view(2));
        assert!(t.due(1_500).is_none());
        t.note(view(3));
        assert!(t.due(2_999).is_none());
        assert!(t.due(3_001).is_some());
        t.wrote(3_001);
        // An identical view is never rewritten.
        assert!(t.due(9_000).is_none());
        // The next real change waits out the gap again.
        t.note(view(4));
        assert!(t.due(4_000).is_none());
        assert!(t.due(5_002).is_some());
    }

    /// `updated_ms` never counts as a change.
    #[test]
    fn the_stamp_is_not_a_change() {
        let mut t = Throttle::default();
        let mut a = view(1);
        a.updated_ms = 7;
        t.note(a);
        t.wrote(1_000);
        let mut same = view(1);
        same.updated_ms = 999;
        t.note(same);
        assert!(t.due(9_000).is_none());
    }
}
