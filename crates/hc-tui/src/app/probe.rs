//! Instrumentation counters a test can hold across `run`, and the
//! diagnostics snapshot reads: loop wakeups and dirty requests per wake
//! kind, draws, host terminal output, stdin reads and the panes the loop
//! held at its last turn. Relaxed atomics only; nothing here allocates.

use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering::Relaxed;

use super::App;

/// Wake kinds, one counter slot each, in `WakeKind` order.
pub const WAKE_KINDS: usize = 8;
/// Pane roles, one `Probe::panes` slot each.
pub const PANE_ROLES: usize = 4;

/// Loop-side counters: wakeups and dirty requests per wake kind, draws,
/// elided draws, painted cells, host terminal output, stdin reads and the
/// panes the loop held at its last turn.
#[derive(Debug, Default)]
pub struct Probe {
    /// Redraws performed.
    pub draws: AtomicU64,
    /// Attach attempts made.
    pub attaches: AtomicU64,
    /// Times the select loop woke for any reason.
    pub wakeups: AtomicU64,
    /// Times a deadline sleep was armed.
    pub timers: AtomicU64,
    /// Times a changed `HostFacts` bundle actually re-seeded panes.
    pub host_facts: AtomicU64,
    /// Projection rebuilds actually run for a publish or focus change.
    pub projections: AtomicU64,
    /// VT cells copied into pane surfaces across all draws.
    pub cells_painted: AtomicU64,
    /// Select-loop wakes by kind, indexed like `WAKE_NAMES`.
    pub wake_kinds: [AtomicU64; WAKE_KINDS],
    /// Dirty requests taken while a turn of each wake kind ran.
    pub dirty_marks: [AtomicU64; WAKE_KINDS],
    /// Draws whose frame emitted nothing because it matched the last one.
    pub elided_draws: AtomicU64,
    /// Write calls that reached the host terminal, and their bytes.
    pub terminal_writes: AtomicU64,
    pub terminal_bytes: AtomicU64,
    /// Reads on stdin, and those that delivered no bytes.
    pub stdin_reads: AtomicU64,
    pub stdin_empty_reads: AtomicU64,
    /// Panes the loop held at its last turn, indexed like `PANE_ROLE_NAMES`.
    pub panes: [AtomicU64; PANE_ROLES],
    /// Set when diagnostics are on: the loop then publishes `panes` each turn.
    pub publish_panes: bool,
}

/// Host terminal output that counts each write call and the bytes it
/// accepted.
pub struct HostOutput<W> {
    inner: W,
    probe: Arc<Probe>,
}

impl<W: Write> HostOutput<W> {
    pub fn new(inner: W, probe: Arc<Probe>) -> HostOutput<W> {
        HostOutput { inner, probe }
    }
}

impl<W: Write> Write for HostOutput<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buf)?;
        self.probe.terminal_writes.fetch_add(1, Relaxed);
        self.probe.terminal_bytes.fetch_add(written as u64, Relaxed);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl App {
    /// Records the panes the loop holds by role. A floating pane (popup,
    /// peek, command overlay or persistent float) counts as `popup`.
    pub(super) fn publish_panes(&self) {
        let floating = self.popup.is_some() as u64
            + self.peek.is_some() as u64
            + self.command_overlay.is_some() as u64
            + self.floats.len() as u64;
        let roles = [
            self.pane.target().is_some() as u64,
            self.observers.len() as u64,
            self.size_leases.len() as u64,
            floating,
        ];
        for (slot, count) in self.probe.panes.iter().zip(roles) {
            slot.store(count, Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_output_counts_each_write_and_byte() {
        let probe = Arc::new(Probe::default());
        let mut out = HostOutput::new(Vec::new(), Arc::clone(&probe));
        out.write_all(b"abc").unwrap();
        out.write_all(b"de").unwrap();
        out.flush().unwrap();
        assert_eq!(out.inner, b"abcde");
        assert_eq!(probe.terminal_writes.load(Relaxed), 2);
        assert_eq!(probe.terminal_bytes.load(Relaxed), 5);
    }
}
