//! The coding agents `backlot skill` can install for, and how one is
//! detected: its binary on `PATH` or its config directory present.

use std::path::PathBuf;

use crate::cli::Env;

/// One coding agent the skill can be installed for.
pub(crate) struct Agent {
    pub name: &'static str,
    /// Programs whose presence on `PATH` detects the agent.
    pub binaries: &'static [&'static str],
    /// The agent's global `skills` directory, relative to `$HOME`.
    pub skills_dir: &'static str,
    /// Configuration directories whose existence detects the agent,
    /// relative to `$HOME`.
    pub config_dirs: &'static [&'static str],
}

/// The agent table, in the order `skill status` lists it. `codex`,
/// `cline` and `pi` share `~/.agents/skills`; one file serves all
/// three.
pub(crate) const AGENTS: &[Agent] = &[
    Agent {
        name: "claude-code",
        binaries: &["claude"],
        skills_dir: "~/.claude/skills",
        config_dirs: &["~/.claude"],
    },
    Agent {
        name: "codex",
        binaries: &["codex"],
        skills_dir: "~/.agents/skills",
        config_dirs: &["~/.agents", "~/.codex"],
    },
    Agent {
        name: "cline",
        binaries: &["cline"],
        skills_dir: "~/.agents/skills",
        config_dirs: &["~/.agents", "~/.cline"],
    },
    Agent {
        name: "pi",
        binaries: &["pi"],
        skills_dir: "~/.agents/skills",
        config_dirs: &["~/.agents", "~/.pi"],
    },
    Agent {
        name: "cursor",
        binaries: &["cursor", "cursor-agent"],
        skills_dir: "~/.cursor/skills",
        config_dirs: &["~/.cursor"],
    },
    Agent {
        name: "devin",
        binaries: &["devin"],
        skills_dir: "~/.config/devin/skills",
        config_dirs: &["~/.config/devin"],
    },
    Agent {
        name: "command-code",
        binaries: &["cmd", "command-code"],
        skills_dir: "~/.commandcode/skills",
        config_dirs: &["~/.commandcode"],
    },
    Agent {
        name: "opencode",
        binaries: &["opencode"],
        skills_dir: "~/.config/opencode/skills",
        config_dirs: &["~/.config/opencode"],
    },
    Agent {
        name: "antigravity",
        binaries: &["antigravity"],
        skills_dir: "~/.gemini/config/skills",
        config_dirs: &["~/.gemini/config"],
    },
    Agent {
        name: "gemini-cli",
        binaries: &["gemini"],
        skills_dir: "~/.gemini/skills",
        config_dirs: &["~/.gemini"],
    },
];

/// Every agent name the table knows, in table order.
pub(crate) fn names() -> Vec<&'static str> {
    AGENTS.iter().map(|a| a.name).collect()
}

/// `$HOME` as the command sees it.
pub(crate) fn home(env: &Env) -> PathBuf {
    env.get("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// A `~/`-relative table path, expanded.
pub(crate) fn expand(path: &str, env: &Env) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home(env).join(rest),
        None => PathBuf::from(path),
    }
}

/// The installed file's path for one agent.
#[cfg(test)]
pub(crate) fn skill_file(agent: &Agent, env: &Env) -> PathBuf {
    expand(agent.skills_dir, env)
        .join("backlot")
        .join("SKILL.md")
}

/// An agent counts as detected when its binary is on `PATH` or one of
/// its config directories exists.
pub(crate) fn detected(agent: &Agent, env: &Env) -> bool {
    let path = env.get("PATH");
    agent
        .binaries
        .iter()
        .any(|b| crate::runners::find_on_path(b, path).is_some())
        || agent.config_dirs.iter().any(|d| expand(d, env).is_dir())
}
