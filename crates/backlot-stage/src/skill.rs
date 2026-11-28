//! `backlot skill`: the open Agent Skills skill that ships in the
//! binary — one `skills/backlot/SKILL.md` for every coding agent the
//! person has, installed with one command. The file in the repository
//! is the same one, so `npx skills add followcat/backlot` works too.
//!
//! The installed copy is never mistaken for the person's own: its
//! frontmatter carries `metadata.generated-by: backlot <version>`, and
//! a file without that marker is left alone unless `--force`. The
//! manifest at `<state root>/skills.json` lists what was written, so a
//! new backlot version refreshes those copies on the next start — and
//! never writes a new place on its own.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::args::SkillCmd;
use crate::cli::{Env, Fail};
use crate::skill_agents::{AGENTS, Agent, detected, expand, home, names};
use crate::store;

/// The skill as the repository carries it — compiled in, so `show`,
/// `install` and `backlot guide agents` all print the same bytes.
pub(crate) const SKILL: &str = include_str!("../../../skills/backlot/SKILL.md");

/// The frontmatter key an installed copy carries.
const MARKER: &str = "  generated-by: backlot ";

/// The version the running binary was built as.
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The skill exactly as `install` writes it: the compiled file with
/// `metadata.generated-by` added to its frontmatter.
pub(crate) fn generated(version: &str) -> String {
    let body = SKILL.strip_prefix("---\n").unwrap_or(SKILL);
    let (front, rest) = body.split_once("\n---\n").unwrap_or((body, ""));
    format!("---\n{front}\nmetadata:\n  generated-by: backlot {version}\n---\n{rest}")
}

/// The version an installed copy names, `None` for a foreign file.
fn marker_version(content: &str) -> Option<&str> {
    content
        .lines()
        .find_map(|l| l.strip_prefix(MARKER))
        .map(str::trim)
}

/// `<state root>/skills.json` — machine-global, because the installed
/// files are.
fn manifest_path(env: &Env) -> PathBuf {
    store::root_dir(&env.vars).join("skills.json")
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Manifest {
    #[serde(default)]
    version: String,
    #[serde(default)]
    paths: Vec<PathBuf>,
}

fn load_manifest(env: &Env) -> Option<Manifest> {
    let text = fs::read_to_string(manifest_path(env)).ok()?;
    serde_json::from_str(&text).ok()
}

fn save_manifest(env: &Env, paths: &[PathBuf]) -> Result<(), Fail> {
    let path = manifest_path(env);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| Fail::refused(format!("{}: {e}", parent.display())))?;
    }
    let manifest = Manifest {
        version: version().into(),
        paths: paths.to_vec(),
    };
    let text = serde_json::to_string_pretty(&manifest)
        .map_err(|e| Fail::refused(format!("skills.json: {e}")))?;
    fs::write(&path, format!("{text}\n"))
        .map_err(|e| Fail::refused(format!("{}: {e}", path.display())))
}

/// The run for one `backlot skill` subcommand.
pub(crate) fn run(cmd: SkillCmd, env: &Env) -> Result<String, Fail> {
    match cmd {
        SkillCmd::Install {
            agents,
            dry_run,
            force,
        } => install(&agents, dry_run, force, env),
        SkillCmd::Status => status(env),
        SkillCmd::Show => Ok(SKILL.to_string()),
        SkillCmd::Uninstall => uninstall(env),
    }
}

/// The chosen agents: every detected one, or exactly the named ones.
fn targets(named: &[String], env: &Env) -> Result<Vec<&'static Agent>, Fail> {
    if named.is_empty() {
        return Ok(AGENTS.iter().filter(|a| detected(a, env)).collect());
    }
    let mut out: Vec<&'static Agent> = Vec::new();
    for name in named {
        let agent = AGENTS.iter().find(|a| a.name == name).ok_or_else(|| {
            Fail::usage(format!(
                "unknown agent '{name}' — known: {}",
                names().join(", ")
            ))
        })?;
        if !out.iter().any(|a| a.name == agent.name) {
            out.push(agent);
        }
    }
    Ok(out)
}

/// Agents grouped by their skills directory, in table order — one
/// written file serves every agent that shares the directory.
fn by_dir<'a>(agents: &[&'a Agent]) -> Vec<(PathBuf, Vec<&'a Agent>)> {
    let mut order: Vec<PathBuf> = Vec::new();
    let mut groups: BTreeMap<PathBuf, Vec<&'a Agent>> = BTreeMap::new();
    for agent in agents {
        let dir = PathBuf::from(agent.skills_dir);
        if !order.contains(&dir) {
            order.push(dir.clone());
        }
        groups.entry(dir).or_default().push(agent);
    }
    order
        .into_iter()
        .filter_map(|dir| groups.remove(&dir).map(|g| (dir, g)))
        .collect()
}

/// `codex, cline, pi` — the agents a row covers.
fn label(group: &[&Agent]) -> String {
    group.iter().map(|a| a.name).collect::<Vec<_>>().join(", ")
}

/// `backlot skill install`: write `<dir>/backlot/SKILL.md` for every
/// detected agent (or only the named ones) and record the paths.
fn install(named: &[String], dry_run: bool, force: bool, env: &Env) -> Result<String, Fail> {
    let agents = targets(named, env)?;
    if agents.is_empty() {
        return Ok(
            "no agent detected — name one with --agent, or install that agent first\n".to_string(),
        );
    }
    let content = generated(version());
    let mut out = String::new();
    let mut written: Vec<PathBuf> = Vec::new();
    let (mut wrote, mut current, mut refreshed, mut skipped) = (0, 0, 0, 0);
    for (dir, group) in by_dir(&agents) {
        let file = expand(&dir.to_string_lossy(), env)
            .join("backlot")
            .join("SKILL.md");
        let shown = display(&file, env);
        let agents = label(&group);
        let verb = |out: &mut String, what: &str| {
            let _ = writeln!(out, "{what} {shown} ({agents})");
        };
        match fs::read_to_string(&file) {
            Ok(existing) => match marker_version(&existing) {
                Some(found) if found == version() => {
                    verb(&mut out, "current");
                    current += 1;
                    written.push(file);
                }
                Some(_) => {
                    if !dry_run {
                        write_skill(&file, &content)?;
                        written.push(file.clone());
                    }
                    verb(
                        &mut out,
                        if dry_run {
                            "would refresh"
                        } else {
                            "refreshed"
                        },
                    );
                    refreshed += 1;
                }
                None if !force => {
                    out.push_str(&format!(
                        "skipped {shown} ({agents}): not backlot's file — use --force\n"
                    ));
                    skipped += 1;
                }
                None => {
                    if !dry_run {
                        write_skill(&file, &content)?;
                        written.push(file.clone());
                    }
                    verb(&mut out, if dry_run { "would replace" } else { "replaced" });
                    wrote += 1;
                }
            },
            Err(_) => {
                if !dry_run {
                    write_skill(&file, &content)?;
                    written.push(file.clone());
                }
                verb(&mut out, if dry_run { "would write" } else { "wrote" });
                wrote += 1;
            }
        }
    }
    if !dry_run && !written.is_empty() {
        let mut paths = load_manifest(env).map(|m| m.paths).unwrap_or_default();
        for path in &written {
            if !paths.contains(path) {
                paths.push(path.clone());
            }
        }
        save_manifest(env, &paths)?;
    }
    let prefix = if dry_run { "would be: " } else { "" };
    let _ = writeln!(
        out,
        "{prefix}{wrote} written, {current} current, {refreshed} refreshed, {skipped} skipped"
    );
    Ok(out)
}

fn write_skill(file: &Path, content: &str) -> Result<(), Fail> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| Fail::refused(format!("{}: {e}", parent.display())))?;
    }
    fs::write(file, content).map_err(|e| Fail::refused(format!("{}: {e}", file.display())))
}

/// A path with `$HOME` shortened to `~`, the way the table names it.
fn display(path: &Path, env: &Env) -> String {
    let home = home(env);
    match path.strip_prefix(&home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// `backlot skill status`: where a copy is installed and whether it is
/// current, stale or foreign.
fn status(env: &Env) -> Result<String, Fail> {
    let mut out = String::new();
    let mut rows = 0;
    for (dir, group) in by_dir(&AGENTS.iter().collect::<Vec<_>>()) {
        let file = expand(&dir.to_string_lossy(), env)
            .join("backlot")
            .join("SKILL.md");
        let Ok(existing) = fs::read_to_string(&file) else {
            continue;
        };
        rows += 1;
        let state = match marker_version(&existing) {
            Some(found) if found == version() => "current",
            Some(_) => "stale",
            None => "foreign",
        };
        let _ = writeln!(out, "{state:<8}{} ({})", display(&file, env), label(&group));
    }
    if rows == 0 {
        return Ok("no installed skill — run `backlot skill install`\n".to_string());
    }
    if out.contains("stale") {
        out.push_str("run `backlot skill install` to refresh stale copies\n");
    }
    Ok(out)
}

/// `backlot skill uninstall`: remove only the copies backlot wrote.
fn uninstall(env: &Env) -> Result<String, Fail> {
    let mut out = String::new();
    let mut removed = 0;
    for (dir, group) in by_dir(&AGENTS.iter().collect::<Vec<_>>()) {
        let file = expand(&dir.to_string_lossy(), env)
            .join("backlot")
            .join("SKILL.md");
        let Ok(existing) = fs::read_to_string(&file) else {
            continue;
        };
        let shown = display(&file, env);
        if marker_version(&existing).is_some() {
            fs::remove_file(&file)
                .map_err(|e| Fail::refused(format!("{}: {e}", file.display())))?;
            if let Some(dir) = file.parent() {
                let _ = fs::remove_dir(dir);
            }
            let _ = writeln!(out, "removed {shown}");
            removed += 1;
        } else {
            let _ = writeln!(out, "left {shown}: not backlot's file ({})", label(&group));
        }
    }
    if let Some(manifest) = load_manifest(env) {
        let kept: Vec<PathBuf> = manifest.paths.into_iter().filter(|p| p.is_file()).collect();
        save_manifest(env, &kept)?;
    }
    if removed == 0 {
        return Ok(format!("{out}nothing to remove\n"));
    }
    let _ = writeln!(out, "{removed} removed");
    Ok(out)
}

/// Rewrite the stale copies the manifest lists, once a new version
/// starts. Never writes a place the manifest does not name and never
/// creates a missing file. Failures are silent: a start must not be
/// held up by a skill.
pub fn refresh(env: &Env) -> Vec<PathBuf> {
    let Some(manifest) = load_manifest(env) else {
        return Vec::new();
    };
    let content = generated(version());
    let mut rewritten = Vec::new();
    let mut kept = Vec::new();
    for path in manifest.paths {
        let Ok(existing) = fs::read_to_string(&path) else {
            continue;
        };
        match marker_version(&existing) {
            Some(found) if found == version() => kept.push(path),
            Some(_) if fs::write(&path, &content).is_ok() => {
                rewritten.push(path.clone());
                kept.push(path);
            }
            _ => {}
        }
    }
    if !rewritten.is_empty() {
        let _ = save_manifest(env, &kept);
    }
    rewritten
}
