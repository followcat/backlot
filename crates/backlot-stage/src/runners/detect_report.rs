//! `backlot runners` (U5): the detector's verdicts as a table —
//! every runner with where its binary was found, or that it was
//! not, its `--version` line, and whether a launch needs the
//! `[runner_env]` PATH write. Read-only: the first thing to run
//! when a role says "not found". `--json` (G4) is the same
//! detection as a document — per runner its models and effort
//! levels too — the New Studio dialog's step-3 source, on
//! whichever machine the command runs.

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;

use super::detect::{self, RUNNERS};
use super::{agy, claude, cline, cmd, codex, cursor, devin, fake, opencode, probe};
use crate::cli::{Env, Fail};

/// Every runner's row: status, version, path, "needs PATH".
pub fn report(env: &Env) -> Result<String, Fail> {
    let scope = detect::scope(env);
    let home = env.get("HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
    let mut rows = Vec::with_capacity(RUNNERS.len());
    for runner in RUNNERS {
        rows.push(match detect::find(runner, &scope) {
            Some(f) => {
                let needs = if f.needs_path() { "yes" } else { "no" }.to_string();
                [
                    (*runner).to_string(),
                    if f.version.is_some() {
                        "found"
                    } else {
                        "not working"
                    }
                    .to_string(),
                    f.version.unwrap_or_else(|| "-".to_string()),
                    detect::tilde(home.as_deref(), &f.path),
                    needs,
                ]
            }
            None => [
                (*runner).to_string(),
                "missing".to_string(),
                "-".to_string(),
                "-".to_string(),
                "-".to_string(),
            ],
        });
    }
    let header = ["runner", "status", "version", "path", "needs PATH"];
    let mut width = [0usize; 5];
    for (i, h) in header.iter().enumerate() {
        width[i] = h.len();
    }
    for row in &rows {
        for (i, c) in row.iter().enumerate() {
            width[i] = width[i].max(c.len());
        }
    }
    let mut out = String::new();
    let mut line = |cells: &[String; 5]| {
        for (i, c) in cells.iter().enumerate() {
            out.push_str(c);
            if i + 1 < cells.len() {
                out.push_str(&" ".repeat(width[i] - c.len() + 2));
            }
        }
        out.push('\n');
    };
    line(&header.map(String::from));
    for row in &rows {
        line(row);
    }
    Ok(out.trim_end().to_string())
}

/// One JSON row — the table's fields plus what the dialog's cast
/// table cycles: `models` (the runner's catalog — free text stays
/// possible for an id it does not know) and `efforts` (empty
/// where the runner carries none).
#[derive(Serialize)]
struct Row {
    runner: String,
    status: &'static str,
    version: Option<String>,
    path: Option<String>,
    needs_path: bool,
    models: Vec<String>,
    efforts: Vec<String>,
}

/// The runner's effort values — its `EFFORTS` const, empty where
/// the argv carries none: `cursor`, `devin` and `opencode` bake
/// the depth into the model or drop it, `cmd`'s flag writes the
/// global config. The dialog shows the model id's depth where a
/// list is empty.
fn efforts(runner: &str) -> Vec<String> {
    let list: &[&str] = match runner {
        "claude" => claude::EFFORTS,
        "codex" => codex::EFFORTS,
        "cursor" => cursor::EFFORTS,
        "devin" => devin::EFFORTS,
        "opencode" => opencode::EFFORTS,
        "cmd" => cmd::EFFORTS,
        "cline" => cline::EFFORTS,
        "agy" => agy::EFFORTS,
        "fake" => fake::EFFORTS,
        _ => &[],
    };
    list.iter().map(|e| (*e).to_string()).collect()
}

/// The runner's model catalog — its `MODELS` const — with the
/// `wizard_cast::PREFERENCE` draft on top when the catalog does
/// not name it: a stale or extra draft id is never silently
/// dropped, and never listed twice.
fn models(runner: &str) -> Vec<String> {
    let catalog: &[&str] = match runner {
        "claude" => claude::MODELS,
        "codex" => codex::MODELS,
        "cursor" => cursor::MODELS,
        "devin" => devin::MODELS,
        "opencode" => opencode::MODELS,
        "cmd" => cmd::MODELS,
        "cline" => cline::MODELS,
        "agy" => agy::MODELS,
        "fake" => fake::MODELS,
        _ => &[],
    };
    let draft = crate::wizard_cast::draft_model(runner);
    let mut out = Vec::with_capacity(catalog.len() + 1);
    if let Some(d) = draft {
        out.push(d.to_string());
    }
    out.extend(
        catalog
            .iter()
            .filter(|m| Some(**m) != draft)
            .map(|m| (*m).to_string()),
    );
    out
}

fn row(scope: &detect::Scope, runner: &str) -> Row {
    let (status, version, path, needs_path) = match detect::find(runner, scope) {
        Some(f) => {
            let needs_path = f.needs_path();
            (
                if f.version.is_some() {
                    "found"
                } else {
                    "not working"
                },
                f.version,
                Some(f.path.to_string_lossy().into_owned()),
                needs_path,
            )
        }
        None => ("missing", None, None, false),
    };
    Row {
        runner: runner.to_string(),
        status,
        version,
        path,
        needs_path,
        models: models(runner),
        efforts: efforts(runner),
    }
}

/// `backlot runners --json` (G4): `RUNNERS`' detection plus `fake`
/// where its script exists — the `BACKLOT_FAKE_RUNNER` override or
/// the shipped `tools/` script. A dev build offers it, a packaged
/// one does not.
pub fn report_json(env: &Env) -> Result<String, Fail> {
    let scope = detect::scope(env);
    let mut rows: Vec<Row> = RUNNERS.iter().map(|r| row(&scope, r)).collect();
    let fake_path = probe::fake_runner_path(env.get(probe::FAKE_RUNNER_ENV));
    if fake_path.is_file() {
        let path = fake_path;
        let version = detect::run_timed(
            &[path.to_string_lossy().into_owned(), "--version".to_string()],
            None,
            Duration::from_secs(3),
        )
        .filter(|o| o.code == 0)
        .and_then(|o| {
            o.stdout
                .lines()
                .chain(o.stderr.lines())
                .map(str::trim)
                .find(|l| !l.is_empty())
                .map(str::to_string)
        });
        rows.push(Row {
            runner: "fake".to_string(),
            status: "found",
            version,
            path: Some(path.to_string_lossy().into_owned()),
            needs_path: false,
            models: models("fake"),
            efforts: efforts("fake"),
        });
    }
    serde_json::to_string_pretty(&rows).map_err(|e| Fail::refused(format!("runners: {e}")))
}
