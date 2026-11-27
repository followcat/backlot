//! Timing percentile and pause-cluster calculations for studio ledgers.

use std::collections::{BTreeMap, BTreeSet};

use crate::event::{Event, EventKind, TaskStateChange};
use crate::metrics::in_window;
use crate::metrics::types::StudioTimingMetrics;
use crate::state::{StudioState, TaskStatus};

/// Raw timing sample bags collected from one studio so the aggregate report
/// can pool samples rather than taking a median of medians.
#[derive(Debug, Default)]
pub(crate) struct StudioTimingSamples {
    pub opening_ms: Option<u64>,
    pub review_latencies_ms: Vec<u64>,
    pub wall_clocks_ms: Vec<u64>,
    pub pause_clusters: usize,
    pub paused_tasks_in_clusters: usize,
}

/// Linear-interpolated percentile in `0.0..=1.0` over a slice (sorted internally).
pub(crate) fn percentile_ms(samples: &[u64], q: f64) -> Option<u64> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    if sorted.len() == 1 {
        return Some(sorted[0]);
    }
    let pos = q.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    let frac = pos - lo as f64;
    let val = sorted[lo] as f64 + frac * (sorted[hi] as f64 - sorted[lo] as f64);
    Some(val.round() as u64)
}

/// Compute opening time, review latency p50/p90, accepted task wall-clock p50,
/// and clusters of `>= 2` consecutive `task.state=paused` events.
pub(crate) fn compute_timing(
    state: &StudioState,
    events: &[Event],
    since_ms: Option<u64>,
) -> (StudioTimingMetrics, StudioTimingSamples) {
    let mut samples = StudioTimingSamples::default();

    let first_req_ts = events.iter().find_map(|e| match &e.kind {
        EventKind::ReqVersion { .. } => Some(e.ts_ms),
        _ => None,
    });
    let first_dispatch = state
        .messages
        .iter()
        .filter(|m| m.purpose.as_deref() == Some("dispatch"))
        .min_by_key(|m| m.seq);
    if let (Some(req_ts), Some(disp)) = (first_req_ts, first_dispatch)
        && in_window(disp.ts_ms, since_ms)
    {
        samples.opening_ms = Some(disp.ts_ms.saturating_sub(req_ts));
    }

    for t in state.tasks.values() {
        let deliv_ts: BTreeMap<u32, u64> = t.deliveries.iter().map(|d| (d.n, d.ts_ms)).collect();
        for r in &t.reviews {
            if in_window(r.ts_ms, since_ms)
                && let Some(&d_ts) = deliv_ts.get(&r.delivery)
            {
                samples
                    .review_latencies_ms
                    .push(r.ts_ms.saturating_sub(d_ts));
            }
        }
        if t.status == TaskStatus::Accepted
            && let (Some(first_cast), Some(closed_ts)) = (t.casts.first(), t.closed_ts_ms)
            && in_window(closed_ts, since_ms)
        {
            samples
                .wall_clocks_ms
                .push(closed_ts.saturating_sub(first_cast.ts_ms));
        }
    }

    let mut run_len = 0usize;
    let mut run_end_ts = 0u64;
    for e in events {
        if matches!(e.kind, EventKind::Unknown { .. }) {
            continue;
        }
        if matches!(
            e.kind,
            EventKind::TaskState {
                state: TaskStateChange::Paused,
                ..
            }
        ) {
            run_len += 1;
            run_end_ts = e.ts_ms;
        } else if run_len > 0 {
            if run_len >= 2 && in_window(run_end_ts, since_ms) {
                samples.pause_clusters += 1;
                samples.paused_tasks_in_clusters += run_len;
            }
            run_len = 0;
        }
    }
    if run_len >= 2 && in_window(run_end_ts, since_ms) {
        samples.pause_clusters += 1;
        samples.paused_tasks_in_clusters += run_len;
    }

    let metrics = StudioTimingMetrics {
        opening_ms: samples.opening_ms,
        review_latency_p50_ms: percentile_ms(&samples.review_latencies_ms, 0.50),
        review_latency_p90_ms: percentile_ms(&samples.review_latencies_ms, 0.90),
        review_latency_samples: samples.review_latencies_ms.len(),
        task_wall_clock_p50_ms: percentile_ms(&samples.wall_clocks_ms, 0.50),
        task_wall_clock_samples: samples.wall_clocks_ms.len(),
        pause_clusters: samples.pause_clusters,
        paused_tasks_in_clusters: samples.paused_tasks_in_clusters,
    };
    (metrics, samples)
}

/// First `dispatch` seq recorded per `requirement_version` in `&events`.
pub(crate) fn first_dispatch_by_req_version(
    state: &StudioState,
    events: &[Event],
) -> BTreeMap<u32, u64> {
    let active_dispatches: BTreeSet<u64> = state
        .messages
        .iter()
        .filter(|m| m.purpose.as_deref() == Some("dispatch"))
        .map(|m| m.seq)
        .collect();
    let mut current_ver: Option<u32> = None;
    let mut out = BTreeMap::new();
    for e in events {
        match &e.kind {
            EventKind::ReqVersion { version, .. } => {
                current_ver = Some(*version);
            }
            EventKind::MessageQueued { .. } if active_dispatches.contains(&e.seq) => {
                if let Some(ver) = current_ver {
                    out.entry(ver).or_insert(e.seq);
                }
            }
            _ => {}
        }
    }
    out
}
