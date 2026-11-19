//! Ledger schema versioning and forward migration (P4a).
//!
//! Every `events.jsonl` line — a studio's and Hollywood's own —
//! carries `"v": <n>` naming the schema version it was stamped at.
//! Lines written before versioning have no `v`; they are v1 lines
//! and read back as such. This module is the pure, I/O-free half of
//! the story: a line's JSON value in, the migrated value out plus
//! whether anything changed. The store runs it on every line it
//! parses, and `backlot studio migrate` uses it to rewrite a log.
//!
//! A migration walks one version at a time through the step table:
//! `v -> v+1 -> ... -> LEDGER_VERSION`. Each step is one function,
//! so a future v2 schema lands as a single `1 -> step` entry here
//! and nowhere else. A line stamped newer than this backlot
//! understands is refused, never silently read or reverted.

use std::fmt;

use serde_json::{Map, Value};

use crate::event::LEDGER_VERSION;

/// Why a ledger line cannot migrate to the current schema version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrateError {
    /// The line parsed as JSON but is not an object.
    MissingObject,
    /// `v` is present but unusable: 0, negative, fractional or not
    /// a number.
    InvalidVersion,
    /// Stamped by a newer backlot than this one understands.
    UnsupportedFuture { found: u32, supported: u32 },
    /// A registered `v -> v+1` step could not run.
    StepFailed { from: u32, reason: String },
}

impl fmt::Display for MigrateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MigrateError::MissingObject => {
                write!(f, "ledger event is not a JSON object")
            }
            MigrateError::InvalidVersion => {
                write!(f, "ledger event carries an invalid schema version")
            }
            MigrateError::UnsupportedFuture { found, supported } => write!(
                f,
                "ledger event version {found} is newer than supported v{supported}; upgrade backlot"
            ),
            MigrateError::StepFailed { from, reason } => {
                write!(
                    f,
                    "ledger migration v{from} -> v{} failed: {reason}",
                    from + 1
                )
            }
        }
    }
}

impl std::error::Error for MigrateError {}

/// The schema version of a line written before versioning existed.
/// Pre-versioning logs are v1 lines *specifically* — stamping
/// `LEDGER_VERSION` on one would skip its `1 -> 2 -> ...` steps once
/// those exist.
const LEGACY_VERSION: u32 = 1;

/// One `v<from> -> v<from+1>` rewrite of a line's object — a plain
/// function pointer so the step tables are `Sync`. A step only
/// edits the object; the pipeline owns the `v` bump.
pub(crate) type MigrateStep = fn(&mut Map<String, Value>) -> Result<(), String>;

/// The studio ledger's steps, in version order — empty while v1 is
/// current. A v2 schema lands here as `1 -> step fn`.
static EVENT_STEPS: &[(u32, MigrateStep)] = &[];

/// Hollywood's own step table. Its envelope version walks with the
/// studio ledger's but its steps stay its own.
static HOLLYWOOD_STEPS: &[(u32, MigrateStep)] = &[];

/// Migrate one studio-ledger line: a pre-versioning line gets
/// `"v": 1` (`changed == true`), a current line passes through
/// (`changed == false`), a broken or future `v` is refused.
pub fn migrate_event_value(val: Value) -> Result<(Value, bool), MigrateError> {
    migrate_with(val, LEDGER_VERSION, EVENT_STEPS)
}

/// The same walk for Hollywood's `events.jsonl`.
pub fn migrate_hollywood_value(val: Value) -> Result<(Value, bool), MigrateError> {
    migrate_with(val, LEDGER_VERSION, HOLLYWOOD_STEPS)
}

/// The pipeline both ledgers share: a missing `v` is filled in as
/// v1 (`changed`), an unusable or future `v` is refused, and every
/// registered step from `v` to `supported` runs in order. The
/// `supported`/`steps` parameters are the test seam — production
/// callers always pass [`LEDGER_VERSION`] and their own table, but
/// tests can walk a synthetic `1 -> 2 -> 3` against them.
pub(crate) fn migrate_with(
    mut val: Value,
    supported: u32,
    steps: &[(u32, MigrateStep)],
) -> Result<(Value, bool), MigrateError> {
    let obj = val.as_object_mut().ok_or(MigrateError::MissingObject)?;
    let mut changed = false;
    let mut version = match obj.get("v") {
        None => {
            obj.insert("v".into(), Value::from(u64::from(LEGACY_VERSION)));
            changed = true;
            LEGACY_VERSION
        }
        Some(v) => {
            let n = v.as_u64().ok_or(MigrateError::InvalidVersion)?;
            u32::try_from(n).map_err(|_| MigrateError::InvalidVersion)?
        }
    };
    if version == 0 {
        return Err(MigrateError::InvalidVersion);
    }
    if version > supported {
        return Err(MigrateError::UnsupportedFuture {
            found: version,
            supported,
        });
    }
    while version < supported {
        let step = steps
            .iter()
            .find(|(from, _)| *from == version)
            .map(|(_, step)| *step)
            .ok_or_else(|| MigrateError::StepFailed {
                from: version,
                reason: "no step registered".into(),
            })?;
        step(obj).map_err(|reason| MigrateError::StepFailed {
            from: version,
            reason,
        })?;
        version += 1;
        obj.insert("v".into(), Value::from(u64::from(version)));
        changed = true;
    }
    Ok((val, changed))
}
