//! Instrumentation counters a test can hold across `run`.

/// Loop-side counters for tests: draws, attaches, wakeups, timers,
/// host-fact reseeds, projection rebuilds and painted cells.
#[derive(Debug, Default)]
pub struct Probe {
    /// Redraws performed.
    pub draws: std::sync::atomic::AtomicU64,
    /// Attach attempts made.
    pub attaches: std::sync::atomic::AtomicU64,
    /// Times the select loop woke for any reason.
    pub wakeups: std::sync::atomic::AtomicU64,
    /// Times a deadline sleep was armed.
    pub timers: std::sync::atomic::AtomicU64,
    /// Times a changed `HostFacts` bundle actually re-seeded panes.
    pub host_facts: std::sync::atomic::AtomicU64,
    /// Projection rebuilds actually run for a publish or focus change.
    pub projections: std::sync::atomic::AtomicU64,
    /// VT cells copied into pane surfaces across all draws.
    pub cells_painted: std::sync::atomic::AtomicU64,
}
