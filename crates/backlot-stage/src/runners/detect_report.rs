//! `backlot runners` (U5): the detector's verdicts as a table —
//! every runner with where its binary was found, or that it was
//! not, its `--version` line, and whether a launch needs the
//! `[runner_env]` PATH write. Read-only: the first thing to run
//! when a role says "not found".

use std::path::PathBuf;

use super::detect::{self, RUNNERS};
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
