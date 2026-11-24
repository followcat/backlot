//! Per-role liveness evidence track (D4): replaces the three independent
//! screen, hook-signal, and activity-revision clocks with one evidence ledger
//! over six observable sources. A role stalls only when none of the six moved
//! within the threshold (3x threshold when the deepest child waits on an inode
//! lock).

use std::collections::BTreeMap;
use std::path::PathBuf;

use backlot_model::caps::revision_stalls;
use backlot_model::state::RoleLastEvent;

use crate::liveness_diff::{
    LockWait, ProcSnap, Source, SourceStamp, WorkAreaRead, WorkAreaSnap, deepest_descendant,
    diff_procs, diff_work_area, to_proc_map, update_lock_wait,
};
use crate::liveness_probe::ProcRead;
use crate::runners::normalise_screen;
use crate::signals::Signal;
use crate::workareas::GitRun;

/// Minimum interval between process-tree walks per role (10 s).
pub(crate) const PROC_SAMPLE_EVERY_MS: u64 = 10_000;

/// Minimum interval between `git status --porcelain` probes per role (60 s).
pub(crate) const WORK_AREA_STATUS_EVERY_MS: u64 = 60_000;

/// Multiplier applied to `stall_limit_s` when the deepest descendant is blocked
/// in a kernel inode lock (`wchan.contains("lock_inode_wait")`).
pub(crate) const LOCK_WAIT_MULTIPLIER: u64 = 3;

#[cfg(test)]
pub(crate) use crate::liveness_probe::{
    ROOT_CPU_FLOOR_JIFFIES_PER_SEC, ROOT_IO_FLOOR_BYTES_PER_SEC,
};

/// Inputs gathered during one `sample_work` pass for a `(studio, role)` pair.
pub(crate) struct ObserveInput<'a> {
    pub now_ms: u64,
    pub runner: &'a str,
    pub eligible: bool,
    pub raw_screen: &'a str,
    pub raw_same_since_ms: u64,
    pub signal: Option<&'a Signal>,
    pub terminal_working: bool,
    pub activity_revision: Option<u64>,
    pub local_machine: bool,
    pub pid: Option<u32>,
    pub work_roots: &'a [PathBuf],
    pub ledger: Option<&'a RoleLastEvent>,
}

/// Cumulative liveness state for one live `(studio, role)`.
#[derive(Debug, Clone)]
pub(crate) struct Evidence {
    norm_screen: String,
    norm_screen_since_ms: u64,
    raw_same_since_ms: u64,
    last_signal_ts_ms: Option<u64>,
    last_revision: Option<u64>,
    last_ledger_seq: Option<u64>,
    baseline_ledger_ts_ms: Option<u64>,
    last_root_pid: Option<u32>,
    last_proc_sample_ms: Option<u64>,
    procs: BTreeMap<u32, ProcSnap>,
    lock_wait: Option<LockWait>,
    last_status_sample_ms: Option<u64>,
    work_area: Option<WorkAreaSnap>,
    last_status_hash: Option<u64>,
    eligible_since_ms: Option<u64>,
    stall_raised_ms: Option<u64>,
    last_moved_ms: u64,
    last_moved: Option<(Source, SourceStamp)>,
    moved_sources: BTreeMap<Source, SourceStamp>,
}

impl Evidence {
    /// Initialise a fresh track on first sighting at `input.now_ms`.
    pub(crate) fn new(
        input: &ObserveInput<'_>,
        proc_read: &ProcRead,
        work_read: &WorkAreaRead,
        git: &mut GitRun<'_>,
    ) -> Self {
        let now_ms = input.now_ms;
        let rev_active = revision_stalls(input.runner) && input.terminal_working;
        let mut ev = Self {
            norm_screen: normalise_screen(input.runner, input.raw_screen),
            norm_screen_since_ms: now_ms,
            raw_same_since_ms: input.raw_same_since_ms,
            last_signal_ts_ms: input.signal.map(|s| s.ts_ms),
            last_revision: rev_active.then_some(input.activity_revision).flatten(),
            last_ledger_seq: input.ledger.map(|e| e.seq),
            baseline_ledger_ts_ms: input.ledger.map(|e| e.ts_ms),
            last_root_pid: if input.local_machine { input.pid } else { None },
            last_proc_sample_ms: None,
            procs: BTreeMap::new(),
            lock_wait: None,
            last_status_sample_ms: None,
            work_area: None,
            last_status_hash: None,
            eligible_since_ms: input.eligible.then_some(now_ms),
            stall_raised_ms: None,
            last_moved_ms: now_ms,
            last_moved: None,
            moved_sources: BTreeMap::new(),
        };
        if let Some(sig) = input.signal {
            let stamp = SourceStamp::new(sig.ts_ms.min(now_ms), &sig.event, "hook");
            ev.last_moved = Some((Source::Hook, stamp.clone()));
            ev.record_source(Source::Hook, stamp);
        }
        if input.local_machine
            && let Some(pid) = input.pid
        {
            ev.last_proc_sample_ms = Some(now_ms);
            let tree = proc_read(pid);
            ev.lock_wait = update_lock_wait(None, &tree, now_ms);
            if let Some(d) = deepest_descendant(&tree) {
                ev.last_moved = Some((Source::Process, SourceStamp::new(now_ms, &d.argv, "CPU")));
            }
            ev.procs = to_proc_map(tree);
        }
        if input.local_machine && !input.work_roots.is_empty() {
            ev.last_status_sample_ms = Some(now_ms);
            let snap = work_read(input.work_roots, true, git);
            ev.last_status_hash = snap.status_hash;
            ev.work_area = Some(snap);
        }
        if ev.last_moved.is_none() {
            ev.last_moved = Some((
                Source::Screen,
                SourceStamp::new(now_ms, "initial screen", "screen"),
            ));
        }
        ev
    }

    /// Observe all six sources on a subsequent sample and update movement times.
    pub(crate) fn observe(
        &mut self,
        input: &ObserveInput<'_>,
        proc_read: &ProcRead,
        work_read: &WorkAreaRead,
        git: &mut GitRun<'_>,
    ) {
        let now_ms = input.now_ms;
        self.raw_same_since_ms = input.raw_same_since_ms;
        if input.eligible {
            self.eligible_since_ms.get_or_insert(now_ms);
        } else {
            self.eligible_since_ms = None;
        }

        let mut moved: Vec<(Source, SourceStamp)> = Vec::new();
        self.check_hook(input, now_ms, &mut moved);
        self.check_revision(input, now_ms, &mut moved);
        self.check_screen(input, now_ms, &mut moved);
        if input.local_machine {
            self.check_procs(input, now_ms, proc_read, &mut moved);
            self.check_work_area(input.work_roots, now_ms, work_read, git, &mut moved);
        }
        if let Some(ev) = input.ledger
            && self.last_ledger_seq.is_none_or(|prev| ev.seq > prev)
        {
            self.last_ledger_seq = Some(ev.seq);
            moved.push((Source::Ledger, SourceStamp::new(now_ms, &ev.kind, "ledger")));
        }

        moved.sort_by_key(|(src, _)| *src);
        if let Some((src, stamp)) = moved.first() {
            self.last_moved_ms = self.last_moved_ms.max(stamp.at_ms);
            self.last_moved = Some((*src, stamp.clone()));
        }
        for (src, stamp) in moved {
            self.record_source(src, stamp);
        }
    }

    fn check_hook(
        &mut self,
        input: &ObserveInput<'_>,
        now_ms: u64,
        moved: &mut Vec<(Source, SourceStamp)>,
    ) {
        if let Some(sig) = input.signal {
            if self.last_signal_ts_ms.is_none_or(|prev| sig.ts_ms > prev) {
                self.last_signal_ts_ms = Some(sig.ts_ms);
                let at_ms = self
                    .stall_raised_ms
                    .map_or(sig.ts_ms.min(now_ms), |r| sig.ts_ms.max(r + 1).min(now_ms));
                moved.push((Source::Hook, SourceStamp::new(at_ms, &sig.event, "hook")));
            }
        } else {
            self.last_signal_ts_ms = None;
        }
    }

    fn check_revision(
        &mut self,
        input: &ObserveInput<'_>,
        now_ms: u64,
        moved: &mut Vec<(Source, SourceStamp)>,
    ) {
        let active = revision_stalls(input.runner) && input.terminal_working;
        match input.activity_revision.filter(|_| active) {
            None => self.last_revision = None,
            Some(rev) => {
                if self.last_revision.is_some_and(|prev| prev != rev) {
                    moved.push((
                        Source::Revision,
                        SourceStamp::new(now_ms, format!("revision {rev}"), "revision"),
                    ));
                }
                self.last_revision = Some(rev);
            }
        }
    }

    fn check_screen(
        &mut self,
        input: &ObserveInput<'_>,
        now_ms: u64,
        moved: &mut Vec<(Source, SourceStamp)>,
    ) {
        let norm = normalise_screen(input.runner, input.raw_screen);
        if norm != self.norm_screen {
            self.norm_screen = norm;
            self.norm_screen_since_ms = now_ms;
            moved.push((
                Source::Screen,
                SourceStamp::new(now_ms, "screen output", "screen"),
            ));
        }
    }

    fn check_procs(
        &mut self,
        input: &ObserveInput<'_>,
        now_ms: u64,
        proc_read: &ProcRead,
        moved: &mut Vec<(Source, SourceStamp)>,
    ) {
        match input.pid {
            None => {
                self.last_root_pid = None;
                self.procs.clear();
                self.lock_wait = None;
            }
            Some(p) if self.last_root_pid != Some(p) => {
                self.last_root_pid = Some(p);
                self.last_proc_sample_ms = Some(now_ms);
                let tree = proc_read(p);
                self.lock_wait = update_lock_wait(None, &tree, now_ms);
                self.procs = to_proc_map(tree);
            }
            Some(p)
                if self
                    .last_proc_sample_ms
                    .is_none_or(|t| now_ms.saturating_sub(t) >= PROC_SAMPLE_EVERY_MS) =>
            {
                let dt_ms = now_ms.saturating_sub(self.last_proc_sample_ms.unwrap_or(now_ms));
                self.last_proc_sample_ms = Some(now_ms);
                let tree = proc_read(p);
                let has_hook = input.signal.is_some();
                if let Some(stamp) = diff_procs(&self.procs, &tree, p, dt_ms, now_ms, has_hook) {
                    moved.push((Source::Process, stamp));
                }
                self.lock_wait = update_lock_wait(self.lock_wait.take(), &tree, now_ms);
                self.procs = to_proc_map(tree);
            }
            Some(_) => {}
        }
    }

    fn check_work_area(
        &mut self,
        work_roots: &[PathBuf],
        now_ms: u64,
        work_read: &WorkAreaRead,
        git: &mut GitRun<'_>,
        moved: &mut Vec<(Source, SourceStamp)>,
    ) {
        if work_roots.is_empty() {
            return;
        }
        let run_status = self
            .last_status_sample_ms
            .is_none_or(|t| now_ms.saturating_sub(t) >= WORK_AREA_STATUS_EVERY_MS);
        if run_status {
            self.last_status_sample_ms = Some(now_ms);
        }
        let snap = work_read(work_roots, run_status, git);
        if let Some(reason) = diff_work_area(self.work_area.as_ref(), self.last_status_hash, &snap)
        {
            moved.push((
                Source::WorkArea,
                SourceStamp {
                    at_ms: now_ms,
                    desc: reason.into(),
                    tag: "work area",
                },
            ));
        }
        if snap.status_hash.is_some() {
            self.last_status_hash = snap.status_hash;
        }
        self.work_area = Some(snap);
    }

    fn record_source(&mut self, src: Source, stamp: SourceStamp) {
        self.last_moved_ms = self.last_moved_ms.max(stamp.at_ms);
        self.moved_sources.insert(src, stamp);
    }

    pub(crate) fn silence_start_ms(&self) -> u64 {
        self.last_moved_ms.max(self.eligible_since_ms.unwrap_or(0))
    }

    pub(crate) fn effective_limit_ms(&self, limit_ms: u64) -> u64 {
        if self.lock_wait.is_some() {
            limit_ms.saturating_mul(LOCK_WAIT_MULTIPLIER)
        } else {
            limit_ms
        }
    }

    pub(crate) fn is_stalled(&self, now_ms: u64, limit_ms: u64) -> bool {
        self.eligible_since_ms.is_some()
            && now_ms.saturating_sub(self.silence_start_ms()) >= self.effective_limit_ms(limit_ms)
    }

    pub(crate) fn mark_raised(&mut self, raised_ms: u64) {
        self.stall_raised_ms = Some(raised_ms);
    }

    pub(crate) fn clear_raised(&mut self) {
        self.stall_raised_ms = None;
    }

    /// Return the clearing reason if any evidence moved since the open stall began.
    pub(crate) fn clear_reason(&mut self, attn_ts_ms: u64, now_ms: u64) -> Option<&'static str> {
        let cutoff = match self.stall_raised_ms {
            Some(raised) => raised,
            None => {
                if self
                    .moved_sources
                    .get(&Source::Hook)
                    .is_some_and(|s| s.at_ms > attn_ts_ms)
                {
                    return Some(Source::Hook.clear_reason());
                }
                if self.baseline_ledger_ts_ms.is_some_and(|t| t > attn_ts_ms)
                    || self
                        .moved_sources
                        .get(&Source::Ledger)
                        .is_some_and(|s| s.at_ms > attn_ts_ms)
                {
                    return Some(Source::Ledger.clear_reason());
                }
                self.stall_raised_ms = Some(now_ms);
                return None;
            }
        };
        for src in [
            Source::Hook,
            Source::Revision,
            Source::Screen,
            Source::Process,
            Source::WorkArea,
            Source::Ledger,
        ] {
            if let Some(stamp) = self.moved_sources.get(&src)
                && stamp.at_ms > cutoff
            {
                self.stall_raised_ms = None;
                return Some(src.clear_reason());
            }
        }
        None
    }

    /// True when raw screen animated during silence while normalised output did not change.
    pub(crate) fn animates_only(&self) -> bool {
        let start = self.silence_start_ms();
        self.lock_wait.is_none()
            && self.raw_same_since_ms > self.norm_screen_since_ms
            && self.raw_same_since_ms > start
    }

    pub(crate) fn lock_wait(&self) -> Option<(&str, u64)> {
        self.lock_wait
            .as_ref()
            .map(|l| (l.argv.as_str(), l.since_ms))
    }

    pub(crate) fn last_moved_stamp(&self) -> Option<&SourceStamp> {
        self.last_moved.as_ref().map(|(_, s)| s)
    }
}
