//! The machine-local Agent Skill state a stage publishes inside its
//! `backlot` summary. Pure wire data: computing it is the stage's
//! filesystem code, reading it is the client's.

use serde::{Deserialize, Serialize};

/// One agent's installed-copy state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillState {
    /// A backlot-marked copy at the publishing binary's own version.
    Current,
    /// A backlot-marked copy at an older version.
    Stale,
    /// A `backlot/SKILL.md` that does not carry backlot's marker.
    Foreign,
    /// The agent is detected, but no copy exists.
    Missing,
    /// The agent itself is not detected.
    Absent,
}

impl SkillState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Stale => "stale",
            Self::Foreign => "foreign",
            Self::Missing => "missing",
            Self::Absent => "absent",
        }
    }
}

/// One known agent row: the agent, its global skills directory and
/// the state of `<dir>/backlot/SKILL.md` on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillAgentSummary {
    pub agent: String,
    pub dir: String,
    pub state: SkillState,
}

/// The machine's whole Agent Skill state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSkillSummary {
    /// The version the publishing stage would write.
    pub version: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<SkillAgentSummary>,
}

impl AgentSkillSummary {
    /// The agent names an automatic one-key install may refresh:
    /// exactly the `missing` and `stale` rows. A foreign copy is the
    /// person's own file and is never touched from that path.
    pub fn installable_agents(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter(|row| matches!(row.state, SkillState::Missing | SkillState::Stale))
            .map(|row| row.agent.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_summary_field_is_optional_and_round_trips() {
        let skill = AgentSkillSummary {
            version: "0.4.32".into(),
            rows: vec![SkillAgentSummary {
                agent: "codex".into(),
                dir: "~/.agents/skills".into(),
                state: SkillState::Missing,
            }],
        };
        let mut summary: crate::summary::Summary =
            serde_json::from_str(r#"{"schema":"backlot/1","updated_ms":1}"#).unwrap();
        assert_eq!(summary.agent_skill, None, "an old summary parses");
        summary.agent_skill = Some(skill.clone());
        let wire = serde_json::to_value(&summary).unwrap();
        assert_eq!(wire["agent_skill"]["rows"][0]["state"], "missing");
        let back: crate::summary::Summary = serde_json::from_value(wire).unwrap();
        assert_eq!(back.agent_skill, Some(skill));
    }

    #[test]
    fn installable_names_only_missing_and_stale_rows() {
        let skill = AgentSkillSummary {
            version: "0.4.32".into(),
            rows: [
                ("a", SkillState::Missing),
                ("b", SkillState::Stale),
                ("c", SkillState::Foreign),
                ("d", SkillState::Absent),
                ("e", SkillState::Current),
            ]
            .into_iter()
            .map(|(agent, state)| SkillAgentSummary {
                agent: agent.into(),
                dir: "~/skills".into(),
                state,
            })
            .collect(),
        };
        assert_eq!(skill.installable_agents(), vec!["a", "b"]);
    }
}
