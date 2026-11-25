//! Persistent deduplication log for handled `plugin.backlot.req` ids.
//!
//! Stores `(req_id, ts_ms, ack)` lines in `<base>/processed-reqs.jsonl`
//! so a stage restart never executes an already-handled request twice, and
//! prunes entries older than [`DEDUPE_TTL_MS`] (24 h).

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use backlot_model::req_wire::{DEDUPE_TTL_MS, ResAck};
use serde::{Deserialize, Serialize};

use crate::cli::Fail;

const DEDUPE_FILE: &str = "processed-reqs.jsonl";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DedupeEntry {
    req_id: String,
    ts_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ack: Option<ResAck>,
}

/// Loaded deduplication index synced with `<base>/processed-reqs.jsonl`.
#[derive(Debug, Default)]
pub(crate) struct DedupeStore {
    base: PathBuf,
    entries: BTreeMap<String, DedupeEntry>,
    last_prune_ms: u64,
}

impl DedupeStore {
    /// Load handled request ids under `base`, discarding entries past the 24 h
    /// retention window and rewriting the file if any stale entries were pruned.
    pub(crate) fn load(base: &Path, now_ms: u64) -> Self {
        let mut store = DedupeStore {
            base: base.to_path_buf(),
            entries: BTreeMap::new(),
            last_prune_ms: now_ms,
        };
        let path = store.path();
        let Ok(raw) = fs::read_to_string(&path) else {
            return store;
        };
        let mut pruned_any = false;
        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Ok(entry) = serde_json::from_str::<DedupeEntry>(trimmed) else {
                pruned_any = true;
                continue;
            };
            if is_expired(entry.ts_ms, now_ms) {
                pruned_any = true;
                continue;
            }
            store.entries.insert(entry.req_id.clone(), entry);
        }
        if pruned_any {
            let _ = store.rewrite_atomic();
        }
        store
    }

    pub(crate) fn contains(&self, req_id: &str) -> bool {
        self.entries.contains_key(req_id)
    }

    pub(crate) fn cached_ack(&self, req_id: &str) -> Option<&ResAck> {
        self.entries.get(req_id).and_then(|e| e.ack.as_ref())
    }

    /// Record a handled `req_id` durably before draining the request queue.
    pub(crate) fn record(&mut self, ack: ResAck, now_ms: u64) -> Result<(), Fail> {
        let entry = DedupeEntry {
            req_id: ack.req_id.clone(),
            ts_ms: now_ms,
            ack: Some(ack),
        };
        self.entries.insert(entry.req_id.clone(), entry.clone());
        let pruned = self.prune_expired(now_ms);
        if pruned {
            self.rewrite_atomic()
        } else {
            self.append_line(&entry)
        }
    }

    /// Drop entries older than [`DEDUPE_TTL_MS`], returning whether any dropped.
    pub(crate) fn prune_expired(&mut self, now_ms: u64) -> bool {
        if now_ms < self.last_prune_ms {
            return false;
        }
        let before = self.entries.len();
        self.entries.retain(|_, e| !is_expired(e.ts_ms, now_ms));
        self.last_prune_ms = now_ms;
        self.entries.len() < before
    }

    /// Sync pruned snapshot to disk cleanly (used when external time advances).
    pub(crate) fn sync_prune(&mut self, now_ms: u64) -> Result<bool, Fail> {
        if !self.prune_expired(now_ms) {
            return Ok(false);
        }
        self.rewrite_atomic()?;
        Ok(true)
    }

    fn path(&self) -> PathBuf {
        self.base.join(DEDUPE_FILE)
    }

    fn append_line(&self, entry: &DedupeEntry) -> Result<(), Fail> {
        if self.base.as_os_str().is_empty() {
            return Ok(());
        }
        fs::create_dir_all(&self.base)
            .map_err(|e| Fail::refused(format!("processed-reqs dir: {e}")))?;
        let mut line = serde_json::to_string(entry)
            .map_err(|e| Fail::refused(format!("processed-reqs encode: {e}")))?;
        line.push('\n');
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path())
            .map_err(|e| Fail::refused(format!("processed-reqs open: {e}")))?;
        f.write_all(line.as_bytes())
            .map_err(|e| Fail::refused(format!("processed-reqs write: {e}")))
    }

    fn rewrite_atomic(&self) -> Result<(), Fail> {
        if self.base.as_os_str().is_empty() {
            return Ok(());
        }
        fs::create_dir_all(&self.base)
            .map_err(|e| Fail::refused(format!("processed-reqs dir: {e}")))?;
        let mut buf = String::new();
        for entry in self.entries.values() {
            let line = serde_json::to_string(entry)
                .map_err(|e| Fail::refused(format!("processed-reqs encode: {e}")))?;
            buf.push_str(&line);
            buf.push('\n');
        }
        let tmp = self
            .base
            .join(format!(".processed-reqs-{}.tmp", std::process::id()));
        fs::write(&tmp, buf)
            .map_err(|e| Fail::refused(format!("processed-reqs tmp write: {e}")))?;
        fs::rename(&tmp, self.path()).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            Fail::refused(format!("processed-reqs rename: {e}"))
        })
    }
}

fn is_expired(entry_ts_ms: u64, now_ms: u64) -> bool {
    now_ms.saturating_sub(entry_ts_ms) > DEDUPE_TTL_MS
}
