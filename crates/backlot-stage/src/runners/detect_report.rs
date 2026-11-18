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
use super::{cline, probe};
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
/// table cycles: `models` (the known list, or just the default the
/// draft would assign — free text stays possible) and `efforts`
/// (empty where the runner carries none).
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

/// The runner's effort values — `cline`'s declared list, the shared
/// `low|medium|high` where an effort flag exists, empty where the
/// cast rule carries none (`cursor`, `devin`, `cmd`).
fn efforts(runner: &str) -> Vec<String> {
    match runner {
        "cline" => cline::EFFORTS.iter().map(|e| (*e).to_string()).collect(),
        "cursor" | "devin" | "cmd" => Vec::new(),
        _ => ["low", "medium", "high"]
            .iter()
            .map(|e| (*e).to_string())
            .collect(),
    }
}

/// The runner's draft model — the one `wizard_cast::PREFERENCE`
/// seats — as a one-element list; the dialog also accepts free text.
fn models(runner: &str) -> Vec<String> {
    crate::wizard_cast::draft_model(runner)
        .map(|m| vec![m.to_string()])
        .unwrap_or_default()
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
/// where its `tools/` script exists — a dev build offers it, a
/// packaged one does not.
pub fn report_json(env: &Env) -> Result<String, Fail> {
    let scope = detect::scope(env);
    let mut rows: Vec<Row> = RUNNERS.iter().map(|r| row(&scope, r)).collect();
    if probe::installed("fake", None) {
        let path = super::tools_dir().join("fake-runner.sh");
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
