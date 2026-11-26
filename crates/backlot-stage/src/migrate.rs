//! `backlot studio migrate [<studio> | --all] [--dry-run]` (P4a):
//! rewrite a ledger's `events.jsonl` at the current schema version —
//! pre-versioning lines get their `v`, a torn tail is dropped —
//! under the same `events.lock` appends take. No event is appended.
//!
//! A real run first copies the log to `events.jsonl.v0.bak` (an
//! existing backup is kept, never overwritten), writes the migrated
//! lines to a temp file in the same directory, fsyncs and renames
//! over `events.jsonl`, then replays the rewritten log and refuses
//! to report success unless the state matches identically.

use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::cli::Fail;
use crate::store::{self, StoreError};

fn store_err(e: StoreError<()>) -> Fail {
    match e {
        StoreError::Io(e) => Fail::refused(e.to_string()),
        StoreError::Corrupt(c) => Fail::refused(c.to_string()),
        _ => Fail::refused("unexpected error"),
    }
}

/// A ledger directory to migrate: its label in output and which
/// replay/migration pair applies.
struct Target {
    name: String,
    dir: PathBuf,
    hollywood: bool,
}

pub(crate) fn run(
    base: &Path,
    named: Option<String>,
    all: bool,
    dry_run: bool,
) -> Result<String, Fail> {
    let mut targets = Vec::new();
    match (named, all) {
        (Some(name), false) => {
            let name = crate::cli::check_studio_name(&name)?;
            let dir = store::studio_dir(base, &name);
            if !dir.join("events.jsonl").is_file() {
                return Err(Fail::refused(format!("no studio '{name}'")));
            }
            targets.push(Target {
                name,
                dir,
                hollywood: false,
            });
        }
        (None, true) => {
            for name in store::studios(base).map_err(|e| Fail::refused(e.to_string()))? {
                let dir = store::studio_dir(base, &name);
                if dir.join("events.jsonl").is_file() {
                    targets.push(Target {
                        name,
                        dir,
                        hollywood: false,
                    });
                }
            }
            let hdir = store::hollywood_dir(base);
            if hdir.join("events.jsonl").is_file() {
                targets.push(Target {
                    name: "hollywood".into(),
                    dir: hdir,
                    hollywood: true,
                });
            }
            if targets.is_empty() {
                return Err(Fail::refused("no ledgers to migrate"));
            }
        }
        _ => return Err(Fail::usage("give a studio or --all, not both")),
    }
    let mut out = String::new();
    for t in &targets {
        let _ = writeln!(out, "{}", migrate_log(t, dry_run)?);
    }
    Ok(out)
}

/// The replayed state used for the pre/post-write equality check —
/// one per ledger type, since the two states are different types.
enum Image {
    Studio(Box<backlot_model::StudioState>),
    Hollywood(Box<backlot_model::hollywood::HollywoodState>),
}

fn image(dir: &Path, hollywood: bool) -> Result<Image, Fail> {
    if hollywood {
        Ok(Image::Hollywood(Box::new(
            store::load_hstate(dir).map_err(store_err)?,
        )))
    } else {
        Ok(Image::Studio(Box::new(
            store::load_state(dir).map_err(store_err)?,
        )))
    }
}

/// Migrate one ledger's `events.jsonl`, returning the report line.
fn migrate_log(t: &Target, dry_run: bool) -> Result<String, Fail> {
    let migrate: store::MigrateFn = if t.hollywood {
        backlot_model::migrate::migrate_hollywood_value
    } else {
        backlot_model::migrate::migrate_event_value
    };
    let _lock = store::take_lock(&t.dir).map_err(|e| Fail::refused(e.to_string()))?;
    let path = t.dir.join("events.jsonl");
    let bytes = fs::read(&path).map_err(|e| Fail::refused(format!("{}: {e}", t.name)))?;

    // Split lines exactly as `store::parse_log` does: empty lines
    // skip, a last line without its newline is a torn tail.
    let terminated = bytes.ends_with(b"\n");
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<String> = Vec::new();
    let mut total = 0usize;
    let mut changed = 0usize;
    let mut torn = false;
    let mut it = text.split('\n').enumerate().peekable();
    while let Some((i, line)) = it.next() {
        if line.is_empty() {
            continue;
        }
        if it.peek().is_none() && !terminated {
            torn = true;
            continue;
        }
        total += 1;
        let val: serde_json::Value = serde_json::from_str(line)
            .map_err(|e| Fail::refused(format!("{}: events.jsonl line {}: {e}", t.name, i + 1)))?;
        let (val, ch) = migrate(val)
            .map_err(|e| Fail::refused(format!("{}: events.jsonl line {}: {e}", t.name, i + 1)))?;
        if ch {
            changed += 1;
            lines.push(serde_json::to_string(&val).map_err(|e| Fail::refused(e.to_string()))?);
        } else {
            lines.push(line.to_string());
        }
    }

    let ver = backlot_model::event::LEDGER_VERSION;
    if changed == 0 && !torn {
        return Ok(format!(
            "{}: already at ledger v{} ({} events)",
            t.name, ver, total
        ));
    }
    if dry_run {
        let tail = if torn { "; a torn tail would drop" } else { "" };
        return Ok(format!(
            "{}: would migrate {}/{} events to ledger v{}{}",
            t.name, changed, total, ver, tail
        ));
    }

    // The before-image the post-write replay must equal.
    let before = image(&t.dir, t.hollywood)?;

    let backup = t.dir.join("events.jsonl.v0.bak");
    if !backup.exists() {
        fs::copy(&path, &backup).map_err(|e| Fail::refused(format!("{}: backup: {e}", t.name)))?;
    }
    let tmp = t.dir.join("events.jsonl.migrate.tmp");
    {
        let mut body = String::new();
        for line in &lines {
            body.push_str(line);
            body.push('\n');
        }
        let mut f =
            fs::File::create(&tmp).map_err(|e| Fail::refused(format!("{}: {e}", t.name)))?;
        f.write_all(body.as_bytes())
            .and_then(|_| f.sync_all())
            .map_err(|e| Fail::refused(format!("{}: {e}", t.name)))?;
    }
    fs::rename(&tmp, &path).map_err(|e| Fail::refused(format!("{}: {e}", t.name)))?;
    // fsync the directory so the rename itself is durable.
    if let Ok(d) = OpenOptions::new().read(true).open(&t.dir) {
        let _ = d.sync_all();
    }

    let same = match (&before, t.hollywood) {
        (Image::Studio(b), false) => store::load_state(&t.dir).map_err(store_err)? == **b,
        (Image::Hollywood(b), true) => store::load_hstate(&t.dir).map_err(store_err)? == **b,
        _ => return Err(Fail::refused(format!("{}: internal mismatch", t.name))),
    };
    if !same {
        return Err(Fail::refused(format!(
            "{}: replay differs after migration; original kept in {}",
            t.name,
            backup.display()
        )));
    }
    let tail = if torn { "; dropped a torn tail" } else { "" };
    Ok(format!(
        "{}: migrated {}/{} events to ledger v{}{}",
        t.name, changed, total, ver, tail
    ))
}
