//! cursor-agent's approval pickers (S12f): what a picker asks —
//! a shell command, a file target, or nothing the stage can name —
//! and whether a file target resolves strictly inside the role's
//! own work area. Detection lives in [`crate::runners::work`];
//! this file reads the picker's question and bounds `y` to paths
//! the stage can vouch for.

use std::path::{Path, PathBuf};

use crate::runners::work;

/// What a cursor approval-picker screen asks for (S12f).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorAsk<'a> {
    /// `Run this command?` — a shell command the block already
    /// shows; the env check reads it like any other.
    Shell,
    /// A file picker — `Write to this file?`, `Write this file?`,
    /// `Read this file?`, `Delete this file?` or `Proceed with
    /// this edit?` — and the path it names when one could be
    /// read. `None` when no complete path is shown: a truncated
    /// `...` one counts unread.
    File(Option<&'a str>),
    /// A picker naming none of the measured questions — an mcp or
    /// web prompt, say — the stage cannot identify what it asks.
    Unknown,
}

/// The question lines the file pickers open with — the S12e
/// capture's `Write to this file?`, the manifest's `Write`/`Read`/
/// `Delete this file?` and `Proceed with this edit?`.
const CURSOR_FILE_QUESTIONS: &[&str] = &[
    "Write to this file?",
    "Write this file?",
    "Read this file?",
    "Delete this file?",
    "Proceed with this edit?",
];

/// The picker region's lines — everything below the last
/// full-width `─` divider, or the whole screen when none is on it.
/// An answered picker's scrollback sits above that divider: the
/// question and target are read where the live picker is.
fn picker_region(screen: &str) -> Vec<&str> {
    let lines: Vec<&str> = screen.lines().collect();
    let start = lines
        .iter()
        .rposition(|l| work::is_full_width_rule(l))
        .map_or(0, |i| i + 1);
    lines[start..].to_vec()
}

/// The question a cursor picker asks, when the screen shows one —
/// `None` off a picker screen. The live picker's question is the
/// recognised one nearest its `→` cursor inside the picker region,
/// so an earlier prompt's line still scrolling above cannot
/// misclassify it.
pub fn cursor_ask(screen: &str) -> Option<CursorAsk<'_>> {
    if !work::cursor_approval_picker(screen) {
        return None;
    }
    let lines = picker_region(screen);
    let Some(cur) = lines.iter().rposition(|l| l.trim_start().starts_with("→ ")) else {
        return Some(CursorAsk::Unknown);
    };
    match lines[..cur].iter().rposition(|l| {
        l.contains("Run this command?") || CURSOR_FILE_QUESTIONS.iter().any(|f| l.contains(f))
    }) {
        None => Some(CursorAsk::Unknown),
        Some(i) if lines[i].contains("Run this command?") => Some(CursorAsk::Shell),
        Some(i) => Some(CursorAsk::File(cursor_file_target(&lines, i))),
    }
}

/// The path a file picker names, as displayed: `Add
/// <Verb>(<path>) to allowlist?` first — cursor's own rule text —
/// else the ` in <path>` line under the question. A truncated
/// `...` path counts as unread.
fn cursor_file_target<'a>(lines: &[&'a str], question: usize) -> Option<&'a str> {
    // The `Add` line belongs to this picker only below its
    // question — an earlier picker's option can still sit in the
    // scrollback above.
    let add = lines[question..].iter().find_map(|l| {
        let rest = l.trim().strip_prefix("Add ")?;
        let open = rest.find('(')?;
        let close = rest[open + 1..].find(") to allowlist")? + open + 1;
        Some(&rest[open + 1..close])
    });
    // The ` in <path>` line sits between the question and the `→`
    // option cursor — `in .` on a shell line never reaches here.
    let end = lines
        .iter()
        .skip(question + 1)
        .position(|l| l.trim_start().starts_with("→ "))
        .map(|i| question + 1 + i)
        .unwrap_or(lines.len());
    let target = add.or_else(|| {
        lines[question + 1..end]
            .iter()
            .find_map(|l| l.trim().strip_prefix("in "))
    })?;
    (!target.is_empty() && !target.contains("...") && !target.contains('…')).then_some(target)
}

/// A cursor file picker's reach is checked against the role's own
/// work area (S12f): `y` may go out only for a target that resolves
/// strictly inside it — never `.git`, `.commandcode` or `cursor`
/// state — while a target the screen cannot be read, or a picker
/// whose question names nothing measured, is the user's outright.
/// Any other runner, or the shell picker, answers `false` — the
/// block's own env check already reads those.
pub fn cursor_needs_user(runner: &str, screen: &str, area: &Path) -> bool {
    if runner != "cursor" {
        return false;
    }
    match cursor_ask(screen) {
        Some(CursorAsk::File(Some(target))) => !inside_area(target, area),
        Some(CursorAsk::File(None)) | Some(CursorAsk::Unknown) => true,
        _ => false,
    }
}

/// `target` strictly inside `area`: the displayed path must be
/// absolute and resolve — symlink-aware — to a descendant of the
/// resolved area naming no protected component (`.git`,
/// `.commandcode`, `.cursor`, `cursor`).
fn inside_area(target: &str, area: &Path) -> bool {
    let path = Path::new(target);
    if !path.is_absolute() {
        return false;
    }
    let resolved = resolve(path);
    let base = resolve(area);
    let Ok(rel) = resolved.strip_prefix(&base) else {
        return false;
    };
    !rel.as_os_str().is_empty()
        && rel.components().all(|c| {
            !matches!(
                c.as_os_str().to_str(),
                Some(".git" | ".commandcode" | ".cursor" | "cursor")
            )
        })
}

/// The path with what the filesystem can answer resolved: the
/// deepest existing ancestor is canonicalised — symlinks and `..`
/// it holds are real — and the not-yet-written tail re-appended. A
/// path that cannot be resolved this way comes back unresolved and
/// fails the prefix check.
fn resolve(path: &Path) -> PathBuf {
    let mut p = path.to_path_buf();
    let mut tail = Vec::new();
    loop {
        match p.canonicalize() {
            Ok(mut c) => {
                for part in tail.iter().rev() {
                    c.push(part);
                }
                return c;
            }
            Err(_) => {
                let Some(name) = p.file_name().map(|n| n.to_os_string()) else {
                    return p;
                };
                p.pop();
                tail.push(name);
            }
        }
    }
}
