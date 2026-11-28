//! The machine-local Agent Skill status, shared by `backlot skill
//! status` and the stage's summary publication. It is deliberately
//! file-stats only: every known agent gets one row, whether or not a
//! copy exists.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use backlot_model::summary::{AgentSkillSummary, SkillAgentSummary, SkillState};

use crate::cli::Env;
use crate::skill::marker_version;
use crate::skill_agents::{AGENTS, Agent, detected, expand, home};

/// The installed copy's path for one agent.
pub(crate) fn skill_file(agent: &Agent, env: &Env) -> PathBuf {
    expand(agent.skills_dir, env)
        .join("backlot")
        .join("SKILL.md")
}

fn file_state(agent: &Agent, env: &Env) -> SkillState {
    if !detected(agent, env) {
        return SkillState::Absent;
    }
    let Ok(existing) = fs::read_to_string(skill_file(agent, env)) else {
        return SkillState::Missing;
    };
    match marker_version(&existing) {
        Some(env!("CARGO_PKG_VERSION")) => SkillState::Current,
        Some(_) => SkillState::Stale,
        None => SkillState::Foreign,
    }
}

/// The report the stage publishes and `status --json` prints: the
/// version this binary would write plus one row per known agent.
pub(crate) fn report(env: &Env) -> AgentSkillSummary {
    AgentSkillSummary {
        version: env!("CARGO_PKG_VERSION").into(),
        rows: AGENTS
            .iter()
            .map(|agent| SkillAgentSummary {
                agent: agent.name.into(),
                dir: agent.skills_dir.into(),
                state: file_state(agent, env),
            })
            .collect(),
    }
}

/// A path with `$HOME` shortened to `~`, the way the table names it.
pub(crate) fn display(path: &Path, env: &Env) -> String {
    let home = home(env);
    match path.strip_prefix(&home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

fn agent_named(name: &str) -> Option<&'static Agent> {
    AGENTS.iter().find(|agent| agent.name == name)
}

/// The human table: file-backed states name the copy itself, while
/// `missing`/`absent` name the directory the row is about.
pub(crate) fn text(report: &AgentSkillSummary, env: &Env) -> String {
    let mut out = String::new();
    for row in &report.rows {
        let Some(agent) = agent_named(&row.agent) else {
            continue;
        };
        let path = match row.state {
            SkillState::Current | SkillState::Stale | SkillState::Foreign => skill_file(agent, env),
            SkillState::Missing | SkillState::Absent => expand(agent.skills_dir, env),
        };
        let _ = writeln!(
            out,
            "{:<8}{} ({})",
            row.state.as_str(),
            display(&path, env),
            row.agent
        );
    }
    if report.rows.iter().any(|row| row.state == SkillState::Stale) {
        out.push_str("run `backlot skill install` to refresh stale copies\n");
    }
    out
}
