//! `backlot studio list [--json]` — what studios exist at all, the
//! discovery half of the cross-session lookup `resolve_base` answers
//! for one name. Every core's state directory is read: each studio's
//! name, core id, ledger state, title and bound path. `BACKLOT_STATE_DIR`
//! (and `--session`/`--remote`) pin the read to one directory, so a
//! role terminal lists only its own core. The table's `core` cell
//! names the core (`core.json`'s name, else the short id) and reads
//! `local` for an unbound state directory — never a bare `-`. Rows
//! sort by recency — the last event's clock — newest first.

use std::path::Path;

use serde::Serialize;

use crate::args::Common;
use crate::cli::{Env, Fail};
use crate::store;

/// How many core-id characters the plain table shows.
const SHORT_CORE: usize = 8;

/// One listed studio.
#[derive(Debug, Serialize)]
struct Row {
    name: String,
    /// The core directory's id; absent when the base is pinned — a
    /// `BACKLOT_STATE_DIR` is no core directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    core: Option<String>,
    /// The core cell the plain table prints: the core's own
    /// `core.json` name when one is present, else the short core id,
    /// else `local` — never a bare `-`. Table-only, so the JSON keeps
    /// its documented shape.
    #[serde(skip)]
    core_label: String,
    /// `live`, `finished`, or `failed` when the ledger cannot be read.
    state: &'static str,
    /// The studio's display title, once `studio.created` has landed.
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    /// The bound path recorded at `studio.created`.
    #[serde(skip_serializing_if = "Option::is_none")]
    repo: Option<String>,
    /// The last event's clock — what the list sorts on.
    last_ms: u64,
}

/// The `--json` document.
#[derive(Debug, Serialize)]
struct Listing {
    /// The read was pinned to one directory rather than scanned
    /// across core directories.
    pinned: bool,
    studios: Vec<Row>,
}

/// The core's display name from `<dir>/core.json` — hc's own identity
/// document (`{"id": …, "name": …}`), absent on some cores and on a
/// scratch state directory.
fn core_name(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("core.json")).ok()?;
    let doc: serde_json::Value = serde_json::from_str(&text).ok()?;
    doc.get("name")
        .and_then(|n| n.as_str())
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_owned)
}

/// The table's `core` cell: the core document's name, else the short
/// core id (`core` for a scanned core, `hint` for a session-pinned
/// one), else `local` — a `BACKLOT_STATE_DIR` with no core behind it.
fn core_label(dir: &Path, core: Option<&str>, hint: Option<&str>) -> String {
    core_name(dir)
        .or_else(|| {
            core.or(hint)
                .map(|id| id.chars().take(SHORT_CORE).collect::<String>())
        })
        .unwrap_or_else(|| "local".into())
}

/// Read one directory's studios into `rows`. `core` is the scanned
/// core directory's id (absent when the base is pinned); `hint` names
/// the core a session-pinned base belongs to.
fn scan(dir: &Path, core: Option<&str>, hint: Option<&str>, rows: &mut Vec<Row>) {
    let Ok(names) = store::studios(dir) else {
        return;
    };
    let label = core_label(dir, core, hint);
    for name in names {
        let sdir = store::studio_dir(dir, &name);
        let row = match store::load_events(&sdir) {
            Ok((events, _)) => {
                let state = backlot_model::replay(&events);
                let config = state.studio.as_ref();
                Row {
                    name,
                    core: core.map(String::from),
                    core_label: label.clone(),
                    state: if state.finished.is_some() {
                        "finished"
                    } else {
                        "live"
                    },
                    title: config.map(|c| c.title.clone()),
                    repo: config.map(|c| c.repo.clone()),
                    last_ms: events.last().map(|e| e.ts_ms).unwrap_or(0),
                }
            }
            // A ledger that will not load is still a studio on disk —
            // it is listed, as `failed`, claiming nothing about itself.
            Err(_) => Row {
                name,
                core: core.map(String::from),
                core_label: label.clone(),
                state: "failed",
                title: None,
                repo: None,
                last_ms: 0,
            },
        };
        rows.push(row);
    }
}

/// Every studio the command can see, newest activity first.
fn collect(env: &Env, common: &Common) -> Result<Listing, Fail> {
    let pinned = env.get("BACKLOT_STATE_DIR").is_some()
        || common.session.is_some()
        || common.remote.is_some();
    let mut rows = Vec::new();
    if pinned {
        // A pinned base is the whole world — one directory, no scan.
        let base = crate::cli::base(env, common)?;
        // A `BACKLOT_STATE_DIR` names no core; `--session`/`--remote`
        // pin to a core directory whose file name is the core id.
        let hint = if env.get("BACKLOT_STATE_DIR").is_some() {
            None
        } else {
            base.file_name().and_then(|n| n.to_str()).map(str::to_owned)
        };
        scan(&base, None, hint.as_deref(), &mut rows);
    } else {
        let root = store::root_dir(&env.vars);
        if let Ok(entries) = std::fs::read_dir(&root) {
            for entry in entries.flatten() {
                if !entry.path().is_dir() {
                    continue;
                }
                let core = entry.file_name().to_string_lossy().into_owned();
                scan(&entry.path(), Some(&core), None, &mut rows);
            }
        }
    }
    // Recency first, then a stable name/core order for the ties —
    // including the `failed` rows that carry no clock at all.
    rows.sort_by(|a, b| {
        b.last_ms
            .cmp(&a.last_ms)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.core.cmp(&b.core))
    });
    Ok(Listing {
        pinned,
        studios: rows,
    })
}

/// The plain table: `studio  core  state  title  path`.
fn table(rows: &[Row]) -> String {
    if rows.is_empty() {
        return "no studios\n".into();
    }
    let cells: Vec<[String; 5]> = rows
        .iter()
        .map(|r| {
            [
                r.name.clone(),
                r.core_label.clone(),
                r.state.to_string(),
                r.title.clone().unwrap_or_else(|| "-".into()),
                r.repo.clone().unwrap_or_else(|| "-".into()),
            ]
        })
        .collect();
    let header = ["studio", "core", "state", "title", "path"];
    let mut width = header.map(str::len);
    for row in &cells {
        for (i, cell) in row.iter().enumerate() {
            width[i] = width[i].max(cell.chars().count());
        }
    }
    let mut out = String::new();
    let mut line = |cells: &[String; 5]| {
        for (i, cell) in cells.iter().enumerate() {
            out.push_str(cell);
            if i + 1 < cells.len() {
                out.push_str(&" ".repeat(width[i] - cell.chars().count() + 2));
            }
        }
        out.push('\n');
    };
    line(&header.map(String::from));
    for row in &cells {
        line(row);
    }
    out
}

/// `backlot studio list [--json]`.
pub fn run(env: &Env, common: &Common, json: bool) -> Result<String, Fail> {
    let listing = collect(env, common)?;
    if json {
        return serde_json::to_string_pretty(&listing)
            .map_err(|e| Fail::refused(format!("studio list: {e}")));
    }
    Ok(table(&listing.studios).trim_end().to_string())
}
